//! `[subtitles]` configuration (feature 025): wanted languages, per-category off switch and
//! the optional Whisper-style transcription server.
//!
//! The API key is masked in the API (`********`), prints as `<redacted>` in `Debug`, and can
//! come from `PUTIO_SYNC_TRANSCRIPTION_API_KEY`.
//!
//! Feature 050 adds the source mode (global, per category, per language), the confidence
//! threshold and `[subtitles.opensubtitles]`. Every new key is optional and left out of the
//! saved file while unset, so a 025 file loads and saves unchanged.

use std::collections::BTreeMap;
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

/// OpenSubtitles confidence threshold range
pub const CONFIDENCE_RANGE: std::ops::RangeInclusive<u8> = 0..=100;
/// Default for `min_confidence`: an ID match alone (60) is not enough, with one release part it is
pub const DEFAULT_MIN_CONFIDENCE: u8 = 65;

const KEY_VAR: &str = "PUTIO_SYNC_TRANSCRIPTION_API_KEY";
const OS_KEY_VAR: &str = "PUTIO_SYNC_OPENSUBTITLES_API_KEY";
const OS_PASSWORD_VAR: &str = "PUTIO_SYNC_OPENSUBTITLES_PASSWORD";

/// Where subtitles come from (feature 050)
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubtitleMode {
    /// Put.io and OpenSubtitles only; never the transcription server
    DownloadOnly,
    /// Download; transcribe only when no wanted language was found (the 025 behaviour)
    #[default]
    DownloadThenWhisper,
    /// Transcribe always; never ask Put.io or OpenSubtitles
    WhisperOnly,
    /// Download, and also transcribe into `<lang>.whisper.srt`
    Both,
}

impl SubtitleMode {
    /// All modes, in the order the UI lists them
    pub const ALL: [SubtitleMode; 4] = [
        SubtitleMode::DownloadOnly,
        SubtitleMode::DownloadThenWhisper,
        SubtitleMode::WhisperOnly,
        SubtitleMode::Both,
    ];

    /// The config name
    pub fn as_str(self) -> &'static str {
        match self {
            SubtitleMode::DownloadOnly => "download_only",
            SubtitleMode::DownloadThenWhisper => "download_then_whisper",
            SubtitleMode::WhisperOnly => "whisper_only",
            SubtitleMode::Both => "both",
        }
    }

    /// Whether this mode asks Put.io and OpenSubtitles
    pub fn downloads(self) -> bool {
        self != SubtitleMode::WhisperOnly
    }

    /// Whether this mode always transcribes
    pub fn always_transcribes(self) -> bool {
        matches!(self, SubtitleMode::WhisperOnly | SubtitleMode::Both)
    }

    /// Whether this mode needs the transcription server configured
    pub fn needs_transcription(self) -> bool {
        self.always_transcribes()
    }
}

/// `[subtitles]`
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct SubtitlesConfig {
    /// Ordered wanted languages (canonical codes); empty turns the stage off
    pub languages: Vec<String>,
    /// Categories that skip the stage (matched case-insensitively)
    pub excluded_categories: Vec<String>,
    /// Global source mode; none means Download then Whisper (025)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<SubtitleMode>,
    /// OpenSubtitles matches scoring below this are not saved, 0..=100
    #[serde(skip_serializing_if = "is_default_confidence")]
    pub min_confidence: u8,
    /// `[subtitles.transcription]`
    pub transcription: TranscriptionConfig,
    /// `[subtitles.category_modes]`: category (lowercase) to mode
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub category_modes: BTreeMap<String, SubtitleMode>,
    /// `[subtitles.language_modes]`: language code to mode, the most specific
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub language_modes: BTreeMap<String, SubtitleMode>,
    /// `[subtitles.opensubtitles]`
    #[serde(skip_serializing_if = "OpenSubtitlesConfig::is_default")]
    pub opensubtitles: OpenSubtitlesConfig,
}

impl Default for SubtitlesConfig {
    fn default() -> Self {
        Self {
            languages: Vec::new(),
            excluded_categories: Vec::new(),
            mode: None,
            min_confidence: DEFAULT_MIN_CONFIDENCE,
            transcription: TranscriptionConfig::default(),
            category_modes: BTreeMap::new(),
            language_modes: BTreeMap::new(),
            opensubtitles: OpenSubtitlesConfig::default(),
        }
    }
}

fn is_default_confidence(v: &u8) -> bool {
    *v == DEFAULT_MIN_CONFIDENCE
}

/// `[subtitles.opensubtitles]`: your own consumer key, and optionally a login for a larger
/// quota. An empty key means this part is off.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct OpenSubtitlesConfig {
    /// Consumer key; env `PUTIO_SYNC_OPENSUBTITLES_API_KEY` wins
    pub api_key: String,
    /// Optional account name
    pub username: String,
    /// Optional account password; needs a username
    pub password: String,
    /// Sent as `User-Agent`; empty means `putio-sync <version>`
    pub user_agent: String,
}

impl fmt::Debug for OpenSubtitlesConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hide = |v: &str| if v.is_empty() { "" } else { "<redacted>" };
        f.debug_struct("OpenSubtitlesConfig")
            .field("api_key", &hide(&self.api_key))
            .field("username", &self.username)
            .field("password", &hide(&self.password))
            .field("user_agent", &self.user_agent)
            .finish()
    }
}

impl OpenSubtitlesConfig {
    /// Nothing set (so nothing is written to the file)
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The key to send: the env override, else the configured one
    pub fn effective_api_key(&self) -> String {
        env_or(OS_KEY_VAR, &self.api_key)
    }

    /// The password to send: the env override, else the configured one
    pub fn effective_password(&self) -> String {
        env_or(OS_PASSWORD_VAR, &self.password)
    }

    /// Whether a key is available, so the OpenSubtitles levels may run
    pub fn is_active(&self) -> bool {
        !self.effective_api_key().trim().is_empty()
    }

    /// The `User-Agent` to send
    pub fn effective_user_agent(&self) -> String {
        let ua = self.user_agent.trim();
        if ua.is_empty() {
            format!("putio-sync {}", env!("CARGO_PKG_VERSION"))
        } else {
            ua.to_string()
        }
    }

    fn validate(&self) -> CoreResult<()> {
        if !self.password.is_empty() && self.username.trim().is_empty() {
            return Err(err(
                "subtitles.opensubtitles.password needs a username".to_string()
            ));
        }
        Ok(())
    }
}

fn env_or(var: &str, stored: &str) -> String {
    std::env::var(var)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| stored.to_string())
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

    /// The mode for a video: the language's, else the category's (case-insensitive), else the
    /// global one, else Download then Whisper. `excluded_categories` is checked elsewhere and wins.
    pub fn mode_for(&self, category: Option<&str>, language: &str) -> SubtitleMode {
        if let Some(m) = self.language_modes.get(language) {
            return *m;
        }
        self.category_mode(category)
    }

    /// The mode ignoring any language override: category, else global, else the default
    pub fn category_mode(&self, category: Option<&str>) -> SubtitleMode {
        let c = category.unwrap_or_default().trim().to_lowercase();
        if !c.is_empty() {
            if let Some(m) = self
                .category_modes
                .iter()
                .find(|(k, _)| k.trim().to_lowercase() == c)
                .map(|(_, m)| *m)
            {
                return m;
            }
        }
        self.mode.unwrap_or_default()
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
        self.normalize_modes()?;
        self.transcription.validate()?;
        self.opensubtitles.validate()?;
        if !CONFIDENCE_RANGE.contains(&self.min_confidence) {
            return Err(err(format!(
                "subtitles.min_confidence must be from {} to {}, got {}",
                CONFIDENCE_RANGE.start(),
                CONFIDENCE_RANGE.end(),
                self.min_confidence
            )));
        }
        Ok(())
    }

    /// Canonical keys for the mode maps, and the check that a Whisper mode has a server
    fn normalize_modes(&mut self) -> CoreResult<()> {
        let mut categories = BTreeMap::new();
        for (k, m) in &self.category_modes {
            let key = k.trim().to_lowercase();
            if key.is_empty() {
                return Err(err(
                    "subtitles.category_modes has an empty category name".to_string()
                ));
            }
            categories.insert(key, *m);
        }
        self.category_modes = categories;
        let mut languages = BTreeMap::new();
        for (k, m) in &self.language_modes {
            let code = crate::lang::lookup(k).ok_or_else(|| {
                err(format!(
                    "subtitles.language_modes: unknown language \"{k}\""
                ))
            })?;
            languages.insert(code, *m);
        }
        self.language_modes = languages;
        if self.transcription.enabled && check_url(&self.transcription.url).is_ok() {
            return Ok(());
        }
        let whisper = self
            .mode
            .map(|m| ("mode".to_string(), m))
            .into_iter()
            .chain(
                self.category_modes
                    .iter()
                    .map(|(k, m)| (format!("category_modes.{k}"), *m)),
            )
            .chain(
                self.language_modes
                    .iter()
                    .map(|(k, m)| (format!("language_modes.{k}"), *m)),
            )
            .find(|(_, m)| m.needs_transcription());
        match whisper {
            Some((key, m)) => Err(err(format!(
                "mode \"{}\" ({key}) needs transcription turned on with a server URL",
                m.as_str()
            ))),
            None => Ok(()),
        }
    }

    /// Check without changing anything
    pub fn validate(&self) -> CoreResult<()> {
        self.clone().normalize()
    }

    /// A copy with the keys and the password replaced by [`MASK`] when set
    pub fn masked(&self) -> Self {
        let mut c = self.clone();
        if !c.transcription.api_key.is_empty() {
            c.transcription.api_key = MASK.to_string();
        }
        if !c.opensubtitles.api_key.is_empty() {
            c.opensubtitles.api_key = MASK.to_string();
        }
        if !c.opensubtitles.password.is_empty() {
            c.opensubtitles.password = MASK.to_string();
        }
        c
    }

    /// Put the stored secrets back where `self` carries [`MASK`]
    pub fn unmasked(mut self, current: &SubtitlesConfig) -> Self {
        if self.transcription.api_key == MASK {
            self.transcription.api_key = current.transcription.api_key.clone();
        }
        if self.opensubtitles.api_key == MASK {
            self.opensubtitles.api_key = current.opensubtitles.api_key.clone();
        }
        if self.opensubtitles.password == MASK {
            self.opensubtitles.password = current.opensubtitles.password.clone();
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

    fn with_whisper(mut c: SubtitlesConfig) -> SubtitlesConfig {
        c.transcription.enabled = true;
        c.transcription.url = "http://nas:8000".to_string();
        c
    }

    #[test]
    fn modes_use_snake_case() {
        for (m, name) in [
            (SubtitleMode::DownloadOnly, "download_only"),
            (SubtitleMode::DownloadThenWhisper, "download_then_whisper"),
            (SubtitleMode::WhisperOnly, "whisper_only"),
            (SubtitleMode::Both, "both"),
        ] {
            let c: SubtitlesConfig = toml::from_str(&format!("mode = \"{name}\"")).unwrap();
            assert_eq!(c.mode, Some(m));
            assert_eq!(m.as_str(), name);
        }
        assert!(toml::from_str::<SubtitlesConfig>("mode = \"sometimes\"").is_err());
    }

    #[test]
    fn language_beats_category_beats_global_beats_default() {
        let mut c = cfg(&["en", "ja"]);
        assert_eq!(
            c.mode_for(Some("anime"), "ja"),
            SubtitleMode::DownloadThenWhisper
        );
        c.mode = Some(SubtitleMode::DownloadOnly);
        assert_eq!(c.mode_for(None, "en"), SubtitleMode::DownloadOnly);
        c.category_modes
            .insert("anime".to_string(), SubtitleMode::WhisperOnly);
        assert_eq!(c.mode_for(Some("Anime"), "en"), SubtitleMode::WhisperOnly);
        assert_eq!(c.mode_for(Some("movies"), "en"), SubtitleMode::DownloadOnly);
        c.language_modes
            .insert("ja".to_string(), SubtitleMode::Both);
        assert_eq!(c.mode_for(Some("anime"), "ja"), SubtitleMode::Both);
        assert_eq!(c.mode_for(Some("anime"), "en"), SubtitleMode::WhisperOnly);
        assert_eq!(c.category_mode(Some("anime")), SubtitleMode::WhisperOnly);
    }

    #[test]
    fn a_whisper_mode_anywhere_needs_the_server() {
        for (key, c) in [
            (
                "mode",
                SubtitlesConfig {
                    mode: Some(SubtitleMode::Both),
                    ..Default::default()
                },
            ),
            ("category_modes.anime", {
                let mut c = SubtitlesConfig::default();
                c.category_modes
                    .insert("Anime".into(), SubtitleMode::WhisperOnly);
                c
            }),
            ("language_modes.ja", {
                let mut c = SubtitlesConfig::default();
                c.language_modes.insert("jpn".into(), SubtitleMode::Both);
                c
            }),
        ] {
            let e = c.validate().unwrap_err().to_string();
            assert!(e.contains(key) && e.contains("transcription"), "{e}");
            assert!(with_whisper(c).validate().is_ok(), "{key}");
        }
        // Download then Whisper degrades to Download only, so it never needs the server
        let c = SubtitlesConfig {
            mode: Some(SubtitleMode::DownloadThenWhisper),
            ..Default::default()
        };
        assert!(c.validate().is_ok());
    }

    #[test]
    fn mode_maps_are_normalized_and_checked() {
        let mut c = SubtitlesConfig::default();
        c.category_modes
            .insert("  Anime ".into(), SubtitleMode::DownloadOnly);
        c.language_modes
            .insert("Spanish".into(), SubtitleMode::DownloadOnly);
        c.normalize().unwrap();
        assert!(c.category_modes.contains_key("anime"));
        assert!(c.language_modes.contains_key("es"));
        c.language_modes
            .insert("klingon".into(), SubtitleMode::Both);
        assert!(c.validate().unwrap_err().to_string().contains("klingon"));
        let mut c = SubtitlesConfig::default();
        c.category_modes.insert("  ".into(), SubtitleMode::Both);
        assert!(c.validate().is_err());
    }

    #[test]
    fn min_confidence_is_a_percentage() {
        let mut c = SubtitlesConfig::default();
        assert_eq!(c.min_confidence, 65);
        for (v, ok) in [(0, true), (100, true), (101, false), (255, false)] {
            c.min_confidence = v;
            assert_eq!(c.validate().is_ok(), ok, "{v}");
        }
    }

    #[test]
    fn a_025_file_loads_and_saves_unchanged() {
        let text = "languages = [\"en\"]\nexcluded_categories = [\"kids\"]\n\n[transcription]\nenabled = true\nurl = \"http://x\"\ntimeout_secs = 900\nlanguage = \"\"\nchunk_secs = 600\nffmpeg_path = \"\"\nmodel = \"\"\napi_key = \"\"\n";
        let c: SubtitlesConfig = toml::from_str(text).unwrap();
        assert_eq!(c.mode, None);
        let out = toml::to_string(&c).unwrap();
        for key in [
            "mode",
            "min_confidence",
            "category_modes",
            "language_modes",
            "opensubtitles",
        ] {
            let set = out
                .lines()
                .any(|l| l.starts_with(&format!("{key} ")) || l.contains(&format!("[{key}]")));
            assert!(!set, "{key} in {out}");
        }
        let back: SubtitlesConfig = toml::from_str(&out).unwrap();
        assert_eq!(back, c);
        // and a normalized default config adds nothing either
        let mut n = c.clone();
        n.normalize().unwrap();
        assert_eq!(toml::to_string(&n).unwrap(), out);
    }

    #[test]
    fn the_new_keys_round_trip() {
        let text = "languages = [\"en\"]\nmode = \"both\"\nmin_confidence = 70\n\n[transcription]\nenabled = true\nurl = \"http://x\"\n\n[category_modes]\nanime = \"whisper_only\"\n\n[language_modes]\nja = \"both\"\n\n[opensubtitles]\napi_key = \"k\"\nusername = \"me\"\n";
        let c: SubtitlesConfig = toml::from_str(text).unwrap();
        assert_eq!(c.min_confidence, 70);
        assert_eq!(c.opensubtitles.username, "me");
        let back: SubtitlesConfig = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn opensubtitles_secrets_are_masked_redacted_and_restored() {
        let mut stored = cfg(&["en"]);
        stored.opensubtitles.api_key = "key-123".into();
        stored.opensubtitles.password = "pw-456".into();
        stored.opensubtitles.username = "me".into();
        let text = format!("{stored:?}");
        assert!(
            !text.contains("key-123") && !text.contains("pw-456"),
            "{text}"
        );
        let shown = stored.masked();
        assert_eq!(shown.opensubtitles.api_key, MASK);
        assert_eq!(shown.opensubtitles.password, MASK);
        assert_eq!(shown.opensubtitles.username, "me");
        let back = shown.clone().unmasked(&stored);
        assert_eq!(back.opensubtitles, stored.opensubtitles);
        let mut typed = shown;
        typed.opensubtitles.api_key = "new".into();
        assert_eq!(typed.unmasked(&stored).opensubtitles.api_key, "new");
        assert_eq!(cfg(&[]).masked().opensubtitles.api_key, "");
    }

    #[test]
    fn opensubtitles_needs_a_key_to_be_active_and_a_username_for_a_password() {
        let mut o = OpenSubtitlesConfig::default();
        assert!(!o.is_active() && o.is_default());
        o.api_key = "k".into();
        assert!(o.is_active());
        assert!(o.effective_user_agent().starts_with("putio-sync "));
        o.user_agent = " my-app v1 ".into();
        assert_eq!(o.effective_user_agent(), "my-app v1");
        let mut c = SubtitlesConfig::default();
        c.opensubtitles.password = "p".into();
        assert!(c.validate().unwrap_err().to_string().contains("username"));
        c.opensubtitles.username = "u".into();
        assert!(c.validate().is_ok());
    }
}
