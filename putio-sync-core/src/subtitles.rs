//! `[subtitles]` configuration (feature 025): wanted languages, per-category off switch and
//! the optional Whisper-style transcription server.
//!
//! The API key is masked in the API (`********`), prints as `<redacted>` in `Debug`, and can
//! come from `PUTIO_SYNC_TRANSCRIPTION_API_KEY`.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

/// What the API shows in place of a stored key
pub const MASK: &str = "********";

/// Most wanted languages
pub const MAX_LANGUAGES: usize = 10;
/// Per-request timeout range, seconds
pub const TIMEOUT_RANGE: std::ops::RangeInclusive<u64> = 10..=7200;
/// Audio chunk length range, seconds
pub const CHUNK_RANGE: std::ops::RangeInclusive<u64> = 60..=1800;

const KEY_VAR: &str = "PUTIO_SYNC_TRANSCRIPTION_API_KEY";

/// `[subtitles]`
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SubtitlesConfig {
    /// Ordered wanted languages (canonical codes); empty turns the stage off
    pub languages: Vec<String>,
    /// Categories that skip the stage (matched case-insensitively)
    pub excluded_categories: Vec<String>,
    /// `[subtitles.transcription]`
    pub transcription: TranscriptionConfig,
}

/// `[subtitles.transcription]`
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct TranscriptionConfig {
    /// Off by default
    pub enabled: bool,
    /// Server URL; the OpenAI path is added when the URL has none
    pub url: String,
    /// Sent as `model` when set
    pub model: String,
    /// Bearer token when set
    pub api_key: String,
    /// Per-request timeout, 10..=7200 seconds
    pub timeout_secs: u64,
    /// Optional language hint
    pub language: String,
    /// Audio part length, 60..=1800 seconds
    pub chunk_secs: u64,
    /// Path to ffmpeg; empty searches `PATH`
    pub ffmpeg_path: String,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            model: String::new(),
            api_key: String::new(),
            timeout_secs: 900,
            language: String::new(),
            chunk_secs: 600,
            ffmpeg_path: String::new(),
        }
    }
}

impl fmt::Debug for TranscriptionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key = if self.api_key.is_empty() {
            ""
        } else {
            "<redacted>"
        };
        f.debug_struct("TranscriptionConfig")
            .field("enabled", &self.enabled)
            .field("url", &crate::redact::url(&self.url))
            .field("model", &self.model)
            .field("api_key", &key)
            .field("timeout_secs", &self.timeout_secs)
            .field("language", &self.language)
            .field("chunk_secs", &self.chunk_secs)
            .field("ffmpeg_path", &self.ffmpeg_path)
            .finish()
    }
}

fn err(msg: String) -> CoreError {
    CoreError::ConfigError(msg)
}

impl TranscriptionConfig {
    /// The key to send: the env override, else the configured one
    pub fn effective_api_key(&self) -> String {
        std::env::var(KEY_VAR)
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| self.api_key.clone())
    }

    /// The URL with `/v1/audio/transcriptions` added when it has no path
    pub fn endpoint(&self) -> String {
        endpoint_for(&self.url)
    }

    /// Check the ranges and, when enabled, the URL
    pub fn validate(&self) -> CoreResult<()> {
        if !TIMEOUT_RANGE.contains(&self.timeout_secs) {
            return Err(err(format!(
                "subtitles.transcription.timeout_secs must be from {} to {}, got {}",
                TIMEOUT_RANGE.start(),
                TIMEOUT_RANGE.end(),
                self.timeout_secs
            )));
        }
        if !CHUNK_RANGE.contains(&self.chunk_secs) {
            return Err(err(format!(
                "subtitles.transcription.chunk_secs must be from {} to {}, got {}",
                CHUNK_RANGE.start(),
                CHUNK_RANGE.end(),
                self.chunk_secs
            )));
        }
        if !self.language.trim().is_empty() && crate::lang::lookup(&self.language).is_none() {
            return Err(err(format!(
                "subtitles.transcription.language: unknown language \"{}\"",
                self.language
            )));
        }
        if self.enabled {
            check_url(&self.url)?;
        }
        Ok(())
    }
}

/// `http`/`https` with a host, else a config error
pub fn check_url(url: &str) -> CoreResult<()> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or_else(|| {
            err(format!(
                "subtitles.transcription.url must start with http:// or https://, got \"{}\"",
                crate::redact::url(url)
            ))
        })?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit('@').next().unwrap_or_default();
    if host.is_empty() || host.starts_with(':') {
        return Err(err(
            "subtitles.transcription.url needs a host (for example http://nas:8000)".to_string(),
        ));
    }
    Ok(())
}

/// See [`TranscriptionConfig::endpoint`]
pub fn endpoint_for(url: &str) -> String {
    let url = url.trim();
    let after_scheme = url.find("://").map(|i| i + 3).unwrap_or(0);
    let path_start = url[after_scheme..]
        .find(['/', '?', '#'])
        .map(|i| i + after_scheme);
    match path_start {
        None => format!("{url}/v1/audio/transcriptions"),
        Some(i) if url.as_bytes()[i] == b'/' && url[i..].trim_end_matches('/').is_empty() => {
            format!("{}/v1/audio/transcriptions", &url[..i])
        }
        Some(_) => url.to_string(),
    }
}

impl SubtitlesConfig {
    /// The stage runs only when a language is wanted
    pub fn is_configured(&self) -> bool {
        !self.languages.is_empty()
    }

    /// Whether `category` is turned off (case-insensitive)
    pub fn excludes(&self, category: &str) -> bool {
        let c = category.trim().to_lowercase();
        !c.is_empty()
            && self
                .excluded_categories
                .iter()
                .any(|e| e.trim().to_lowercase() == c)
    }

    /// The wanted languages as canonical codes, deduplicated in order. Unknown entries are
    /// kept as they map leniently, so a hand-edited file never stops a running daemon.
    pub fn wanted(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for l in &self.languages {
            let code = crate::lang::map_lenient(l);
            if code != "und" && !out.contains(&code) {
                out.push(code);
            }
        }
        out
    }

    /// Map the languages to canonical codes (deduplicated, order kept) and check everything.
    /// An unknown language or more than [`MAX_LANGUAGES`] entries is an error.
    pub fn normalize(&mut self) -> CoreResult<()> {
        let mut out: Vec<String> = Vec::new();
        for l in &self.languages {
            let code = crate::lang::lookup(l)
                .ok_or_else(|| err(format!("subtitles.languages: unknown language \"{}\"", l)))?;
            if !out.contains(&code) {
                out.push(code);
            }
        }
        if out.len() > MAX_LANGUAGES {
            return Err(err(format!(
                "subtitles.languages has at most {MAX_LANGUAGES} entries, got {}",
                out.len()
            )));
        }
        self.languages = out;
        self.excluded_categories = self
            .excluded_categories
            .iter()
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();
        self.transcription.validate()
    }

    /// Check without changing anything
    pub fn validate(&self) -> CoreResult<()> {
        self.clone().normalize()
    }

    /// A copy with the key replaced by [`MASK`] when one is set
    pub fn masked(&self) -> Self {
        let mut c = self.clone();
        if !c.transcription.api_key.is_empty() {
            c.transcription.api_key = MASK.to_string();
        }
        c
    }

    /// Put the stored key back where `self` carries [`MASK`]
    pub fn unmasked(mut self, current: &SubtitlesConfig) -> Self {
        if self.transcription.api_key == MASK {
            self.transcription.api_key = current.transcription.api_key.clone();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(langs: &[&str]) -> SubtitlesConfig {
        SubtitlesConfig {
            languages: langs.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn defaults_are_off() {
        let c = SubtitlesConfig::default();
        assert!(!c.is_configured());
        assert!(!c.transcription.enabled);
        assert_eq!(c.transcription.timeout_secs, 900);
        assert_eq!(c.transcription.chunk_secs, 600);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn languages_map_to_codes_and_dedupe_in_order() {
        let mut c = cfg(&["English", "spa", "en", "Spanish", "pt-BR"]);
        c.normalize().unwrap();
        assert_eq!(c.languages, vec!["en", "es", "pt-br"]);
    }

    #[test]
    fn unknown_language_names_the_value() {
        let e = cfg(&["en", "klingon"]).validate().unwrap_err().to_string();
        assert!(e.contains("klingon"), "{e}");
    }

    #[test]
    fn more_than_ten_languages_is_an_error() {
        let many = [
            "en", "es", "fr", "de", "it", "pt", "nl", "sv", "da", "fi", "no",
        ];
        assert!(cfg(&many).validate().is_err());
        assert!(cfg(&many[..10]).validate().is_ok());
    }

    #[test]
    fn enabled_transcription_needs_an_http_url_with_a_host() {
        let mut c = SubtitlesConfig::default();
        c.transcription.enabled = true;
        for bad in [
            "",
            "nas:8000",
            "ftp://nas",
            "http://",
            "http:///x",
            "http://:80",
        ] {
            c.transcription.url = bad.to_string();
            assert!(c.validate().is_err(), "{bad}");
        }
        for good in [
            "http://nas:8000",
            "https://w.example.com/inference",
            "http://u:p@h/x",
        ] {
            c.transcription.url = good.to_string();
            assert!(c.validate().is_ok(), "{good}");
        }
        // an unused bad URL is fine while disabled
        c.transcription.enabled = false;
        c.transcription.url = "junk".to_string();
        assert!(c.validate().is_ok());
    }

    #[test]
    fn ranges_are_checked() {
        for (t, ch, ok) in [
            (9, 600, false),
            (10, 60, true),
            (7200, 1800, true),
            (7201, 600, false),
            (900, 59, false),
            (900, 1801, false),
        ] {
            let mut c = SubtitlesConfig::default();
            c.transcription.timeout_secs = t;
            c.transcription.chunk_secs = ch;
            assert_eq!(c.validate().is_ok(), ok, "{t} {ch}");
        }
    }

    #[test]
    fn language_hint_must_be_known_when_set() {
        let mut c = SubtitlesConfig::default();
        c.transcription.language = "eng".to_string();
        assert!(c.validate().is_ok());
        c.transcription.language = "zzzz".to_string();
        assert!(c.validate().is_err());
    }

    #[test]
    fn endpoint_adds_the_path_only_when_there_is_none() {
        let cases = [
            ("http://nas:8000", "http://nas:8000/v1/audio/transcriptions"),
            (
                "http://nas:8000/",
                "http://nas:8000/v1/audio/transcriptions",
            ),
            ("http://nas:8080/inference", "http://nas:8080/inference"),
            (
                "https://h/v1/audio/transcriptions",
                "https://h/v1/audio/transcriptions",
            ),
        ];
        for (url, want) in cases {
            assert_eq!(endpoint_for(url), want, "{url}");
        }
    }

    #[test]
    fn excluded_categories_match_case_insensitively() {
        let c = SubtitlesConfig {
            excluded_categories: vec!["Radarr".to_string()],
            ..Default::default()
        };
        assert!(c.excludes("radarr"));
        assert!(!c.excludes("sonarr"));
        assert!(!c.excludes(""));
    }

    #[test]
    fn mask_and_unmask_keep_the_stored_key() {
        let mut stored = cfg(&["en"]);
        stored.transcription.api_key = "sekrit".to_string();
        let shown = stored.masked();
        assert_eq!(shown.transcription.api_key, MASK);
        let back = shown.clone().unmasked(&stored);
        assert_eq!(back.transcription.api_key, "sekrit");
        // a typed key wins
        let mut typed = shown;
        typed.transcription.api_key = "new".to_string();
        assert_eq!(typed.unmasked(&stored).transcription.api_key, "new");
        // empty stays empty (nothing to mask)
        assert_eq!(cfg(&[]).masked().transcription.api_key, "");
    }

    #[test]
    fn debug_never_prints_the_key() {
        let mut c = SubtitlesConfig::default();
        c.transcription.api_key = "sekrit-value".to_string();
        c.transcription.url = "http://user:pw@nas:8000".to_string();
        let text = format!("{c:?}");
        assert!(!text.contains("sekrit-value"), "{text}");
        assert!(!text.contains("pw@"), "{text}");
        assert!(text.contains("<redacted>"), "{text}");
    }

    #[test]
    fn toml_round_trip() {
        let c: SubtitlesConfig = toml::from_str(
            "languages = [\"en\"]\n[transcription]\nenabled = true\nurl = \"http://x\"\nchunk_secs = 120\n",
        )
        .unwrap();
        assert_eq!(c.transcription.chunk_secs, 120);
        assert_eq!(c.transcription.timeout_secs, 900);
        assert_eq!(c.languages, vec!["en"]);
    }
}
