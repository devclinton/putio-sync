//! Source modes for one video (feature 050): which wanted languages are downloaded, whether the
//! transcription server is used, and how its file is named.
//!
//! The plan is computed once per video from [`SubtitlesConfig::mode_for`] and read by the
//! engine, so the rules sit in one table-tested place.

use putio_sync_core::subtitles::{SubtitleMode, SubtitlesConfig};

/// The effective mode of each wanted language, plus the mode for a language that Whisper hears
/// but nobody asked for
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModePlan {
    per_language: Vec<(String, SubtitleMode)>,
    other: SubtitleMode,
}

impl ModePlan {
    /// The plan for `languages` (canonical codes) of an item in `category`
    pub fn new(config: &SubtitlesConfig, category: Option<&str>, languages: &[String]) -> Self {
        Self {
            per_language: languages
                .iter()
                .map(|l| (l.clone(), config.mode_for(category, l)))
                .collect(),
            other: config.category_mode(category),
        }
    }

    /// Every language in the same `mode`
    pub fn uniform(languages: &[String], mode: SubtitleMode) -> Self {
        Self {
            per_language: languages.iter().map(|l| (l.clone(), mode)).collect(),
            other: mode,
        }
    }

    /// The 025 behaviour: Download then Whisper for everything
    pub fn legacy(languages: &[String]) -> Self {
        Self::uniform(languages, SubtitleMode::DownloadThenWhisper)
    }

    /// The mode of `language`: its own when wanted, else the mode for other languages
    pub fn mode(&self, language: &str) -> SubtitleMode {
        self.per_language
            .iter()
            .find(|(l, _)| l == language)
            .map(|(_, m)| *m)
            .unwrap_or(self.other)
    }

    /// Whether `language` is wanted and its mode asks Put.io and OpenSubtitles
    pub fn downloads(&self, language: &str) -> bool {
        self.per_language
            .iter()
            .any(|(l, m)| l == language && m.downloads())
    }

    /// The wanted languages that download, in order
    pub fn download_languages(&self) -> Vec<String> {
        self.per_language
            .iter()
            .filter(|(_, m)| m.downloads())
            .map(|(l, _)| l.clone())
            .collect()
    }

    /// Whether any wanted language is `whisper_only` or `both`: transcribe whatever Put.io says
    pub fn always_transcribe(&self) -> bool {
        self.per_language
            .iter()
            .any(|(_, m)| m.always_transcribes())
    }

    /// Whether any wanted language is `download_then_whisper`: transcribe when nothing was found
    pub fn fallback_transcribe(&self) -> bool {
        self.per_language
            .iter()
            .any(|(_, m)| *m == SubtitleMode::DownloadThenWhisper)
    }

    /// Whether the file for the language Whisper `detected` carries the `whisper` token
    pub fn whisper_token(&self, detected: &str) -> bool {
        self.mode(detected) == SubtitleMode::Both
    }

    /// A short word for the step message when every language has one mode, else `mixed`
    pub fn label(&self) -> &'static str {
        let mut modes = self.per_language.iter().map(|(_, m)| *m);
        let Some(first) = modes.next() else {
            return self.other.as_str();
        };
        if modes.all(|m| m == first) {
            first.as_str()
        } else {
            "mixed"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SubtitleMode::*;

    fn langs(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| s.to_string()).collect()
    }

    fn cfg(global: Option<SubtitleMode>) -> SubtitlesConfig {
        SubtitlesConfig {
            mode: global,
            ..Default::default()
        }
    }

    #[test]
    fn each_mode_alone_decides_downloads_and_transcription() {
        // (mode, downloads, always, fallback, token)
        let table = [
            (DownloadOnly, true, false, false, false),
            (DownloadThenWhisper, true, false, true, false),
            (WhisperOnly, false, true, false, false),
            (Both, true, true, false, true),
        ];
        for (mode, dl, always, fallback, token) in table {
            let p = ModePlan::uniform(&langs(&["en"]), mode);
            assert_eq!(p.downloads("en"), dl, "{mode:?}");
            assert_eq!(p.always_transcribe(), always, "{mode:?}");
            assert_eq!(p.fallback_transcribe(), fallback, "{mode:?}");
            assert_eq!(p.whisper_token("en"), token, "{mode:?}");
            assert_eq!(p.whisper_token("ja"), token, "other follows the same mode");
        }
    }

    #[test]
    fn the_plan_follows_language_then_category_then_global() {
        let mut c = cfg(Some(DownloadOnly));
        c.category_modes.insert("anime".into(), WhisperOnly);
        c.language_modes.insert("ja".into(), Both);
        let p = ModePlan::new(&c, Some("Anime"), &langs(&["en", "ja"]));
        assert_eq!(p.mode("en"), WhisperOnly);
        assert_eq!(p.mode("ja"), Both);
        assert_eq!(
            p.mode("fr"),
            WhisperOnly,
            "an unwanted language takes the category mode"
        );
        assert_eq!(p.download_languages(), ["ja"]);
        assert!(p.always_transcribe());
        let p = ModePlan::new(&c, Some("movies"), &langs(&["en", "ja"]));
        assert_eq!(p.mode("en"), DownloadOnly);
        assert_eq!(p.mode("ja"), Both);
        assert_eq!(p.download_languages(), ["en", "ja"]);
    }

    #[test]
    fn no_mode_set_is_the_025_behaviour() {
        let p = ModePlan::new(&cfg(None), None, &langs(&["en", "es"]));
        assert_eq!(p, ModePlan::legacy(&langs(&["en", "es"])));
        assert!(p.fallback_transcribe() && !p.always_transcribe());
        assert_eq!(p.label(), "download_then_whisper");
    }

    #[test]
    fn mixed_plans_say_so() {
        let mut c = cfg(Some(DownloadOnly));
        c.language_modes.insert("es".into(), Both);
        let p = ModePlan::new(&c, None, &langs(&["en", "es"]));
        assert_eq!(p.label(), "mixed");
        assert!(
            !p.downloads("fr"),
            "a language nobody wants is not downloaded"
        );
        assert!(!p.whisper_token("en"));
        assert!(p.whisper_token("es"));
    }
}
