//! What Put.io offers, made comparable: language, source rank, forced and SDH flags, and the
//! order to try candidates in.

use putio_sync_client::{PutioSubtitle, SubtitleList};
use putio_sync_core::lang;

use crate::naming::SlotKind;

/// Where Put.io got a subtitle
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// A track inside the video file
    Embedded,
    /// A file next to the video
    Folder,
    /// OpenSubtitles, through Put.io's list
    OpenSubtitles,
    /// OpenSubtitles API, found by the video's file fingerprint (feature 050)
    OsHash,
    /// OpenSubtitles API, found by IMDb or TMDb id
    OsIds,
    /// OpenSubtitles API, found by the parsed file name
    OsName,
    /// A source string we do not know
    Unknown,
}

impl SourceKind {
    /// Map Put.io's `source` string (case-insensitive substring match; the exact strings are
    /// unconfirmed)
    pub fn from_source(source: &str) -> Self {
        let s = source.to_lowercase();
        if s.contains("opensubtitles") {
            SourceKind::OpenSubtitles
        } else if ["mkv", "embedded", "track", "stream"]
            .iter()
            .any(|k| s.contains(k))
        {
            SourceKind::Embedded
        } else if ["folder", "file", "sidecar"].iter().any(|k| s.contains(k)) {
            SourceKind::Folder
        } else {
            SourceKind::Unknown
        }
    }

    /// Lower is better: folder and embedded match the release, then OpenSubtitles, then rest
    pub fn rank(self) -> u8 {
        match self {
            SourceKind::Embedded | SourceKind::Folder => 0,
            SourceKind::OpenSubtitles
            | SourceKind::OsHash
            | SourceKind::OsIds
            | SourceKind::OsName => 1,
            SourceKind::Unknown => 2,
        }
    }

    /// The metrics `source` label. An unknown source is counted as `opensubtitles` so the
    /// label set stays fixed.
    pub fn as_metric(self) -> &'static str {
        match self {
            SourceKind::Embedded => "putio_embedded",
            SourceKind::Folder => "putio_folder",
            SourceKind::OsHash => "os_hash",
            SourceKind::OsIds => "os_ids",
            SourceKind::OsName => "os_name",
            SourceKind::OpenSubtitles | SourceKind::Unknown => "opensubtitles",
        }
    }
}

/// One subtitle Put.io offers, with what we derived from it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Put.io's subtitle
    pub raw: PutioSubtitle,
    /// Canonical language
    pub language: String,
    /// Where it came from
    pub source: SourceKind,
    /// Forced (foreign parts only)
    pub forced: bool,
    /// For the deaf and hard of hearing
    pub sdh: bool,
    /// Put.io named it the default
    pub is_default: bool,
    /// Position in Put.io's list
    pub order: usize,
}

impl Candidate {
    /// Which file this candidate can fill
    pub fn kind(&self) -> SlotKind {
        if self.forced {
            SlotKind::Forced
        } else if self.sdh {
            SlotKind::Sdh
        } else {
            SlotKind::Main
        }
    }
}

/// `(forced, sdh)` from a subtitle's display name: whole-word, case-insensitive matches on
/// `forced`, `foreign parts only`, `sdh`, `hearing impaired`, `hi`, `cc`
pub fn detect_flags(name: &str) -> (bool, bool) {
    let lower = name.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let has = |w: &str| words.contains(&w);
    let forced = has("forced") || (has("foreign") && has("parts"));
    let sdh = has("sdh") || has("hi") || has("cc") || (has("hearing") && has("impaired"));
    (forced, sdh)
}

/// Turn Put.io's list into candidates, in Put.io's order. The language comes from
/// `language_code`, else from the language name.
pub fn from_list(list: &SubtitleList) -> Vec<Candidate> {
    list.subtitles
        .iter()
        .enumerate()
        .map(|(order, s)| {
            let language = if !s.language_code.trim().is_empty() {
                lang::map_lenient(&s.language_code)
            } else {
                lang::map_lenient(&s.language)
            };
            let (forced, sdh) = detect_flags(&s.name);
            Candidate {
                raw: s.clone(),
                language,
                source: SourceKind::from_source(&s.source),
                forced,
                sdh,
                is_default: list.default.as_deref() == Some(s.key.as_str()),
                order,
            }
        })
        .collect()
}

/// Candidates for a slot, best first: source rank, then Put.io's default, then Put.io's order.
/// Forced and SDH candidates only ever fill their own slot.
pub fn select<'a>(all: &'a [Candidate], language: &str, kind: SlotKind) -> Vec<&'a Candidate> {
    let mut v: Vec<&Candidate> = all
        .iter()
        .filter(|c| c.language == language && c.kind() == kind)
        .collect();
    v.sort_by_key(|c| (c.source.rank(), !c.is_default, c.order));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(key: &str, code: &str, name: &str, source: &str) -> PutioSubtitle {
        PutioSubtitle {
            key: key.into(),
            language_code: code.into(),
            name: name.into(),
            source: source.into(),
            ..Default::default()
        }
    }

    #[test]
    fn flags_come_from_the_name() {
        for (name, forced, sdh) in [
            ("English", false, false),
            ("English (Forced)", true, false),
            ("English - Foreign Parts Only", true, false),
            ("English SDH", false, true),
            ("English [hearing impaired]", false, true),
            ("English HI", false, true),
            ("English CC", false, true),
            ("Hindi", false, false),
            ("Chinese (Traditional)", false, false),
        ] {
            assert_eq!(detect_flags(name), (forced, sdh), "{name}");
        }
    }

    #[test]
    fn source_strings_map_by_substring_with_unknown_last() {
        use SourceKind::*;
        for (s, k) in [
            ("opensubtitles", OpenSubtitles),
            ("OpenSubtitles.org", OpenSubtitles),
            ("mkv", Embedded),
            ("Embedded track", Embedded),
            ("folder", Folder),
            ("sidecar file", Folder),
            ("something-new", Unknown),
            ("", Unknown),
        ] {
            assert_eq!(SourceKind::from_source(s), k, "{s}");
        }
        assert_eq!(Unknown.as_metric(), "opensubtitles");
        assert_eq!(Embedded.as_metric(), "putio_embedded");
        assert_eq!(Folder.as_metric(), "putio_folder");
        assert!(Folder.rank() < OpenSubtitles.rank() && OpenSubtitles.rank() < Unknown.rank());
    }

    #[test]
    fn language_from_code_then_name_and_three_letter_codes() {
        let list = SubtitleList {
            default: None,
            subtitles: vec![
                sub("a", "eng", "English", "opensubtitles"),
                PutioSubtitle {
                    key: "b".into(),
                    language: "Spanish".into(),
                    ..Default::default()
                },
                sub("c", "", "x", ""),
            ],
        };
        let c = from_list(&list);
        assert_eq!(c[0].language, "en");
        assert_eq!(c[1].language, "es");
        assert_eq!(c[2].language, "und");
    }

    #[test]
    fn selection_ranks_folder_and_embedded_over_opensubtitles() {
        let list = SubtitleList {
            default: Some("os2".into()),
            subtitles: vec![
                sub("os1", "en", "English", "opensubtitles"),
                sub("os2", "en", "English", "opensubtitles"),
                sub("emb", "en", "English", "mkv"),
                sub("odd", "en", "English", "weird"),
                sub("fr", "fr", "French", "folder"),
                sub("forced", "en", "English forced", "folder"),
                sub("sdh", "en", "English SDH", "opensubtitles"),
            ],
        };
        let c = from_list(&list);
        let keys = |l: &str, k| -> Vec<String> {
            select(&c, l, k).iter().map(|c| c.raw.key.clone()).collect()
        };
        assert_eq!(keys("en", SlotKind::Main), ["emb", "os2", "os1", "odd"]);
        assert_eq!(keys("en", SlotKind::Forced), ["forced"]);
        assert_eq!(keys("en", SlotKind::Sdh), ["sdh"]);
        assert_eq!(keys("fr", SlotKind::Main), ["fr"]);
        assert!(keys("es", SlotKind::Main).is_empty());
    }
}
