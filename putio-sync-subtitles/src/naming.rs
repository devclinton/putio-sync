//! Sidecar names, existing-sidecar detection and the no-overwrite write.

use std::io::Write;
use std::path::{Path, PathBuf};

use putio_sync_core::lang;

/// Which of a language's files a subtitle fills
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlotKind {
    /// `<stem>.<lang>.srt`
    Main,
    /// `<stem>.<lang>.forced.srt`
    Forced,
    /// `<stem>.<lang>.sdh.srt`
    Sdh,
    /// `<stem>.<lang>.whisper.srt`: a transcription kept beside a download (feature 050)
    Whisper,
}

impl SlotKind {
    fn infix(self) -> &'static str {
        match self {
            SlotKind::Main => "",
            SlotKind::Forced => ".forced",
            SlotKind::Sdh => ".sdh",
            SlotKind::Whisper => ".whisper",
        }
    }
}

/// A language and the kind of file for it
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Slot {
    /// Canonical language code
    pub language: String,
    /// Main, forced or SDH
    pub kind: SlotKind,
}

impl Slot {
    /// A main slot
    pub fn main(language: &str) -> Self {
        Self {
            language: language.to_string(),
            kind: SlotKind::Main,
        }
    }

    /// A transcription slot (`.whisper`)
    pub fn whisper(language: &str) -> Self {
        Self {
            language: language.to_string(),
            kind: SlotKind::Whisper,
        }
    }
}

/// A subtitle file already next to the video
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Existing {
    /// Canonical language code
    pub language: String,
    /// Main, forced or SDH
    pub kind: SlotKind,
    /// Where it is
    pub path: PathBuf,
}

/// Extensions that count as a subtitle sidecar
const SUB_EXTS: [&str; 5] = ["srt", "ass", "ssa", "vtt", "sub"];

/// The video's file name without its extension
pub fn stem(video: &Path) -> String {
    video
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `<stem>.<lang>[.forced|.sdh|.whisper].srt`
pub fn sidecar_name(video: &Path, slot: &Slot) -> String {
    format!("{}.{}{}.srt", stem(video), slot.language, slot.kind.infix())
}

/// The sidecar's path, in the video's directory
pub fn sidecar_path(video: &Path, slot: &Slot) -> PathBuf {
    video
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(sidecar_name(video, slot))
}

/// Subtitle sidecars of `video`: same directory, name starts with the video stem and a dot,
/// a subtitle extension, and a dot-separated token that is a language. `forced` and
/// `sdh`/`hi`/`cc` tokens make it a forced or SDH file, and a `whisper` token a transcription
/// kept beside a download; none of them counts as the main one. (`hi` alone is Hindi.)
pub fn existing_sidecars(video: &Path) -> Vec<Existing> {
    let Some(dir) = video.parent() else {
        return Vec::new();
    };
    let prefix = format!("{}.", stem(video).to_lowercase());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let lower = name.to_lowercase();
        let Some(ext) = path.extension().map(|e| e.to_string_lossy().to_lowercase()) else {
            continue;
        };
        if !SUB_EXTS.contains(&ext.as_str()) || !lower.starts_with(&prefix) {
            continue;
        }
        let end = lower.len() - ext.len() - 1;
        if end < prefix.len() {
            continue;
        }
        let middle = &lower[prefix.len()..end];
        if let Some((language, kind)) = classify(middle) {
            out.push(Existing {
                language,
                kind,
                path,
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn classify(tokens: &str) -> Option<(String, SlotKind)> {
    let mut kind = SlotKind::Main;
    let mut whisper = false;
    let mut langs: Vec<String> = Vec::new();
    for t in tokens.split('.') {
        match t {
            "whisper" => whisper = true,
            "forced" => kind = SlotKind::Forced,
            "sdh" | "cc" => {
                if kind != SlotKind::Forced {
                    kind = SlotKind::Sdh
                }
            }
            _ => {
                if let Some(code) = lang::lookup(t) {
                    langs.push(code);
                }
            }
        }
    }
    // `hi` next to another language is the hearing-impaired marker, alone it is Hindi
    if langs.iter().any(|l| l != "hi") && langs.contains(&"hi".to_string()) {
        langs.retain(|l| l != "hi");
        if kind == SlotKind::Main {
            kind = SlotKind::Sdh;
        }
    }
    if whisper {
        kind = SlotKind::Whisper;
    }
    langs.first().map(|l| (l.clone(), kind))
}

/// Whether `path` is named like a transcription: `<name>.<lang>.whisper.<ext>`
pub fn is_transcription(path: &Path) -> bool {
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mut tokens = name.rsplit('.');
    tokens.next() == Some("whisper") && tokens.next().is_some_and(|t| lang::lookup(t).is_some())
}

/// The language token of a subtitle file name, looking from the end past `forced`, `sdh`, `cc`
/// and `whisper` (`Movie.en.whisper.srt` gives `en`)
pub fn language_of(path: &Path) -> Option<String> {
    let name = path.file_stem()?.to_string_lossy().to_lowercase();
    name.rsplit('.')
        .filter(|t| !matches!(*t, "forced" | "sdh" | "cc" | "whisper"))
        .find_map(lang::lookup)
}

/// Whether a file for this slot already exists next to the video
pub fn has_slot(existing: &[Existing], slot: &Slot) -> bool {
    existing
        .iter()
        .any(|e| e.language == slot.language && e.kind == slot.kind)
}

/// How [`write_new`] ended
#[derive(Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    /// The file was created
    Written,
    /// The target existed already and was left alone
    AlreadyExists,
}

/// Write `data` to `target` without ever replacing a file that exists: write `.<name>.part`
/// beside it, then hard-link it into place (which fails when the target exists), falling back
/// to an exists-check and rename where links are not available. The part file is always
/// removed.
pub fn write_new(target: &Path, data: &[u8]) -> std::io::Result<WriteOutcome> {
    if target.exists() {
        return Ok(WriteOutcome::AlreadyExists);
    }
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let part = target.with_file_name(format!(".{name}.part"));
    let result = (|| {
        let mut f = std::fs::File::create(&part)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        match std::fs::hard_link(&part, target) {
            Ok(()) => Ok(WriteOutcome::Written),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                Ok(WriteOutcome::AlreadyExists)
            }
            Err(_) => {
                if target.exists() {
                    return Ok(WriteOutcome::AlreadyExists);
                }
                std::fs::rename(&part, target)?;
                Ok(WriteOutcome::Written)
            }
        }
    })();
    let _ = std::fs::remove_file(&part);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, name: &str) {
        std::fs::write(dir.join(name), "x").unwrap();
    }

    #[test]
    fn names_follow_the_slot() {
        let v = Path::new("/m/Movie (2024)/Movie.2024.mkv");
        assert_eq!(sidecar_name(v, &Slot::main("en")), "Movie.2024.en.srt");
        let forced = Slot {
            language: "es".into(),
            kind: SlotKind::Forced,
        };
        let sdh = Slot {
            language: "pt-br".into(),
            kind: SlotKind::Sdh,
        };
        assert_eq!(sidecar_name(v, &forced), "Movie.2024.es.forced.srt");
        assert_eq!(sidecar_name(v, &sdh), "Movie.2024.pt-br.sdh.srt");
        assert_eq!(
            sidecar_path(v, &Slot::main("en")),
            Path::new("/m/Movie (2024)/Movie.2024.en.srt")
        );
    }

    #[test]
    fn finds_sidecars_by_stem_extension_and_language() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        for n in [
            "Movie.mkv",
            "Movie.en.srt",
            "Movie.eng.srt",
            "Movie.Spanish.ass",
            "Movie.fr.forced.srt",
            "Movie.en.sdh.srt",
            "Movie.de.hi.srt",
            "Movie.hi.srt",
            "Movie.srt",
            "Movie.nfo",
            "Movie.xx.srt",
            "Other.en.srt",
            "Movie2.en.srt",
        ] {
            touch(d, n);
        }
        let found = existing_sidecars(&d.join("Movie.mkv"));
        let mut got: Vec<(String, SlotKind)> =
            found.iter().map(|e| (e.language.clone(), e.kind)).collect();
        got.sort_by_key(|(l, k)| (l.clone(), *k as u8));
        assert_eq!(
            got,
            vec![
                ("de".to_string(), SlotKind::Sdh),
                ("en".to_string(), SlotKind::Main),
                ("en".to_string(), SlotKind::Main),
                ("en".to_string(), SlotKind::Sdh),
                ("es".to_string(), SlotKind::Main),
                ("fr".to_string(), SlotKind::Forced),
                ("hi".to_string(), SlotKind::Main),
            ]
        );
        assert!(has_slot(&found, &Slot::main("en")));
        assert!(!has_slot(&found, &Slot::main("fr")));
        assert!(has_slot(
            &found,
            &Slot {
                language: "fr".into(),
                kind: SlotKind::Forced
            }
        ));
    }

    #[test]
    fn stem_match_is_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "movie.EN.SRT");
        let found = existing_sidecars(&dir.path().join("Movie.mkv"));
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn missing_directory_gives_nothing() {
        assert!(existing_sidecars(Path::new("/no/such/dir/v.mkv")).is_empty());
    }

    #[test]
    fn write_creates_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Movie.en.srt");
        assert_eq!(write_new(&target, b"first").unwrap(), WriteOutcome::Written);
        assert_eq!(std::fs::read(&target).unwrap(), b"first");
        assert_eq!(
            write_new(&target, b"second").unwrap(),
            WriteOutcome::AlreadyExists
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"first");
        // no part file left behind
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["Movie.en.srt"]);
    }

    #[test]
    fn write_into_a_missing_directory_is_an_error_and_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("gone").join("Movie.en.srt");
        assert!(write_new(&target, b"x").is_err());
    }

    #[test]
    fn whisper_names_are_their_own_slot() {
        let v = Path::new("/m/Movie.mkv");
        assert_eq!(
            sidecar_name(v, &Slot::whisper("en")),
            "Movie.en.whisper.srt"
        );
        assert_eq!(
            sidecar_path(v, &Slot::whisper("pt-br")),
            Path::new("/m/Movie.pt-br.whisper.srt")
        );
    }

    #[test]
    fn a_whisper_file_is_never_the_main_slot() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        for n in [
            "Movie.en.whisper.srt",
            "Movie.fr.srt",
            "Movie.fr.whisper.srt",
            "Movie.de.forced.srt",
            "Movie.de.sdh.srt",
            "Movie.de.whisper.srt",
            "Movie.ja.whisper.srt",
        ] {
            touch(d, n);
        }
        let found = existing_sidecars(&d.join("Movie.mkv"));
        // a transcription alone leaves the main slot empty, so a download still goes ahead
        assert!(!has_slot(&found, &Slot::main("en")));
        assert!(has_slot(&found, &Slot::whisper("en")));
        assert!(has_slot(&found, &Slot::main("fr")));
        assert!(has_slot(&found, &Slot::whisper("fr")));
        assert!(!has_slot(&found, &Slot::main("de")));
        assert!(has_slot(&found, &Slot::whisper("de")));
        assert!(has_slot(&found, &Slot::whisper("ja")));
        let kinds_of_de: Vec<_> = found
            .iter()
            .filter(|e| e.language == "de")
            .map(|e| e.kind)
            .collect();
        assert_eq!(
            kinds_of_de.len(),
            3,
            "forced, sdh and whisper are independent"
        );
    }

    #[test]
    fn transcriptions_are_recognized_by_name() {
        for yes in [
            "/m/Movie.en.whisper.srt",
            "/m/Movie.2024.PT-BR.Whisper.srt",
            "x.ja.whisper.ass",
        ] {
            assert!(is_transcription(Path::new(yes)), "{yes}");
        }
        for no in [
            "/m/Movie.en.srt",
            "/m/Movie.whisper.srt",
            "/m/Movie.en.forced.srt",
            "/m/whisper.en.srt",
        ] {
            assert!(!is_transcription(Path::new(no)), "{no}");
        }
        assert_eq!(
            language_of(Path::new("/m/Movie.en.whisper.srt")).as_deref(),
            Some("en")
        );
        assert_eq!(
            language_of(Path::new("/m/Movie.es.forced.srt")).as_deref(),
            Some("es")
        );
        assert_eq!(language_of(Path::new("/m/Movie.srt")), None);
    }
}
