//! `[translation]` configuration (feature 026): how a missing subtitle language is translated,
//! by an OpenAI-compatible chat model or by the user's own callback API.
//!
//! The chat API key and the callback header values are masked in the API (`********`), print
//! as `<redacted>` in `Debug`, and the key can come from `PUTIO_SYNC_TRANSLATION_API_KEY`.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

/// What the API shows in place of a stored secret
pub const MASK: &str = "********";

/// Requests to the translator at once
pub const IN_FLIGHT_RANGE: std::ops::RangeInclusive<usize> = 1..=8;
/// Per HTTP request, seconds
pub const TIMEOUT_RANGE: std::ops::RangeInclusive<u64> = 10..=1800;
/// Cues per chat request
pub const CHUNK_RANGE: std::ops::RangeInclusive<usize> = 5..=200;
/// How long to wait for a callback, seconds
pub const CALLBACK_TIMEOUT_RANGE: std::ops::RangeInclusive<u64> = 60..=86400;
/// Longest extra instruction, characters
pub const MAX_INSTRUCTION: usize = 500;
/// Most callback headers
pub const MAX_HEADERS: usize = 20;

const KEY_VAR: &str = "PUTIO_SYNC_TRANSLATION_API_KEY";

/// Which translator runs
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TranslatorKind {
    /// The stage is off
    #[default]
    None,
    /// An OpenAI-compatible chat model
    Chat,
    /// The user's own API, with an optional callback
    Callback,
}

impl TranslatorKind {
    /// Lowercase name used in metrics, logs and the API
    pub fn as_str(self) -> &'static str {
        match self {
            TranslatorKind::None => "none",
            TranslatorKind::Chat => "chat",
            TranslatorKind::Callback => "callback",
        }
    }
}

/// `[translation]`
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct TranslationConfig {
    /// `none` turns the stage off
    pub kind: TranslatorKind,
    /// Categories that skip the stage (matched case-insensitively)
    pub excluded_categories: Vec<String>,
    /// Requests to the translator at once, 1..=8
    pub max_in_flight: usize,
    /// Per HTTP request, 10..=1800 seconds
    pub timeout_secs: u64,
    /// Cues per chat request, 5..=200
    pub chunk_cues: usize,
    /// Extra instruction for the model, at most 500 characters
    pub instruction: String,
    /// `[translation.chat]`
    pub chat: ChatConfig,
    /// `[translation.callback]`
    pub callback: CallbackConfig,
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            kind: TranslatorKind::None,
            excluded_categories: Vec::new(),
            max_in_flight: 2,
            timeout_secs: 180,
            chunk_cues: 40,
            instruction: String::new(),
            chat: ChatConfig::default(),
            callback: CallbackConfig::default(),
        }
    }
}

/// `[translation.chat]`
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct ChatConfig {
    /// Server base, for example `http://ollama.lan:11434/v1`
    pub base_url: String,
    /// Model name
    pub model: String,
    /// Bearer token when set
    pub api_key: String,
}

impl fmt::Debug for ChatConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key = if self.api_key.is_empty() {
            ""
        } else {
            "<redacted>"
        };
        f.debug_struct("ChatConfig")
            .field("base_url", &crate::redact::url(&self.base_url))
            .field("model", &self.model)
            .field("api_key", &key)
            .finish()
    }
}

impl ChatConfig {
    /// The key to send: the env override, else the configured one
    pub fn effective_api_key(&self) -> String {
        std::env::var(KEY_VAR)
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| self.api_key.clone())
    }
}

/// `[translation.callback]`
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct CallbackConfig {
    /// Where the subtitle is POSTed
    pub url: String,
    /// How the service reaches putio-sync
    pub callback_base_url: String,
    /// How long to wait for the callback, 60..=86400 seconds
    pub timeout_secs: u64,
    /// Extra request headers; values are secrets
    pub headers: BTreeMap<String, String>,
}

impl Default for CallbackConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            callback_base_url: String::new(),
            timeout_secs: 3600,
            headers: BTreeMap::new(),
        }
    }
}

impl fmt::Debug for CallbackConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: BTreeMap<&String, &str> =
            self.headers.keys().map(|k| (k, "<redacted>")).collect();
        f.debug_struct("CallbackConfig")
            .field("url", &crate::redact::url(&self.url))
            .field(
                "callback_base_url",
                &crate::redact::url(&self.callback_base_url),
            )
            .field("timeout_secs", &self.timeout_secs)
            .field("headers", &headers)
            .finish()
    }
}

fn err(msg: String) -> CoreError {
    CoreError::ConfigError(msg)
}

/// `http`/`https` with a host, else a config error naming `field`
pub fn check_http_url(field: &str, url: &str) -> CoreResult<()> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or_else(|| {
            err(format!(
                "{field} must start with http:// or https://, got \"{}\"",
                crate::redact::url(url)
            ))
        })?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit('@').next().unwrap_or_default();
    if host.is_empty() || host.starts_with(':') {
        return Err(err(format!(
            "{field} needs a host (for example http://nas:8088)"
        )));
    }
    Ok(())
}

impl TranslationConfig {
    /// Whether a translator is selected
    pub fn is_configured(&self) -> bool {
        self.kind != TranslatorKind::None
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

    /// Tidy the values (newlines in the instruction, categories, trailing `/` on the base
    /// URL) and check everything
    pub fn normalize(&mut self) -> CoreResult<()> {
        self.instruction = self
            .instruction
            .split(['\r', '\n'])
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        self.excluded_categories = self
            .excluded_categories
            .iter()
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();
        self.chat.base_url = self.chat.base_url.trim().to_string();
        self.chat.model = self.chat.model.trim().to_string();
        self.callback.url = self.callback.url.trim().to_string();
        self.callback.callback_base_url = self
            .callback
            .callback_base_url
            .trim()
            .trim_end_matches('/')
            .to_string();
        if !IN_FLIGHT_RANGE.contains(&self.max_in_flight) {
            return Err(err(format!(
                "translation.max_in_flight must be from {} to {}, got {}",
                IN_FLIGHT_RANGE.start(),
                IN_FLIGHT_RANGE.end(),
                self.max_in_flight
            )));
        }
        if !TIMEOUT_RANGE.contains(&self.timeout_secs) {
            return Err(err(format!(
                "translation.timeout_secs must be from {} to {}, got {}",
                TIMEOUT_RANGE.start(),
                TIMEOUT_RANGE.end(),
                self.timeout_secs
            )));
        }
        if !CHUNK_RANGE.contains(&self.chunk_cues) {
            return Err(err(format!(
                "translation.chunk_cues must be from {} to {}, got {}",
                CHUNK_RANGE.start(),
                CHUNK_RANGE.end(),
                self.chunk_cues
            )));
        }
        if self.instruction.chars().count() > MAX_INSTRUCTION {
            return Err(err(format!(
                "translation.instruction has at most {MAX_INSTRUCTION} characters, got {}",
                self.instruction.chars().count()
            )));
        }
        match self.kind {
            TranslatorKind::None => {}
            TranslatorKind::Chat => {
                check_http_url("translation.chat.base_url", &self.chat.base_url)?;
                if self.chat.model.is_empty() {
                    return Err(err("translation.chat.model is required".to_string()));
                }
            }
            TranslatorKind::Callback => {
                let c = &self.callback;
                check_http_url("translation.callback.url", &c.url)?;
                if c.callback_base_url.is_empty() {
                    return Err(err(
                        "translation.callback.callback_base_url is required: the address the \
                         service uses to reach putio-sync (for example http://nas.lan:8088)"
                            .to_string(),
                    ));
                }
                check_http_url(
                    "translation.callback.callback_base_url",
                    &c.callback_base_url,
                )?;
                let rest = c.callback_base_url.split("://").nth(1).unwrap_or_default();
                let authority = rest.split('/').next().unwrap_or_default();
                if authority.contains('@') {
                    return Err(err(
                        "translation.callback.callback_base_url must not contain a user name or \
                         password"
                            .to_string(),
                    ));
                }
                if c.callback_base_url.contains(['?', '#']) {
                    return Err(err(
                        "translation.callback.callback_base_url must not have a query or fragment"
                            .to_string(),
                    ));
                }
                if !CALLBACK_TIMEOUT_RANGE.contains(&c.timeout_secs) {
                    return Err(err(format!(
                        "translation.callback.timeout_secs must be from {} to {}, got {}",
                        CALLBACK_TIMEOUT_RANGE.start(),
                        CALLBACK_TIMEOUT_RANGE.end(),
                        c.timeout_secs
                    )));
                }
            }
        }
        if self.callback.headers.len() > MAX_HEADERS {
            return Err(err(format!(
                "translation.callback.headers has at most {MAX_HEADERS} entries"
            )));
        }
        for (name, value) in &self.callback.headers {
            if reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_err() {
                return Err(err(format!(
                    "translation.callback.headers: \"{name}\" is not a valid header name"
                )));
            }
            if value == MASK {
                return Err(err(format!(
                    "translation.callback.headers.{name}: still masked; type the value again"
                )));
            }
            if reqwest::header::HeaderValue::from_str(value).is_err() {
                return Err(err(format!(
                    "translation.callback.headers.{name}: the value has characters a header \
                     cannot carry"
                )));
            }
        }
        Ok(())
    }

    /// Check without changing anything
    pub fn validate(&self) -> CoreResult<()> {
        self.clone().normalize()
    }

    /// A copy with the key and every header value replaced by [`MASK`]
    pub fn masked(&self) -> Self {
        let mut c = self.clone();
        if !c.chat.api_key.is_empty() {
            c.chat.api_key = MASK.to_string();
        }
        c.callback
            .headers
            .values_mut()
            .for_each(|v| *v = MASK.to_string());
        c
    }

    /// Put stored secrets back where `self` carries [`MASK`]. A masked header with nothing
    /// stored under its name stays masked, which [`Self::validate`] then refuses.
    pub fn unmasked(mut self, current: &TranslationConfig) -> Self {
        if self.chat.api_key == MASK {
            self.chat.api_key = current.chat.api_key.clone();
        }
        for (name, value) in self.callback.headers.iter_mut() {
            if value == MASK {
                if let Some(stored) = current.callback.headers.get(name) {
                    *value = stored.clone();
                }
            }
        }
        self
    }

    /// Every secret value, for the log redactor
    pub fn secret_values(&self) -> Vec<String> {
        let mut out = vec![self.chat.effective_api_key()];
        out.extend(self.callback.headers.values().cloned());
        out.retain(|v| !v.is_empty());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat() -> TranslationConfig {
        let mut c = TranslationConfig {
            kind: TranslatorKind::Chat,
            ..Default::default()
        };
        c.chat.base_url = "http://ollama.lan:11434/v1".to_string();
        c.chat.model = "qwen2.5:14b".to_string();
        c
    }

    fn callback() -> TranslationConfig {
        let mut c = TranslationConfig {
            kind: TranslatorKind::Callback,
            ..Default::default()
        };
        c.callback.url = "http://svc.lan/translate".to_string();
        c.callback.callback_base_url = "http://nas.lan:8088".to_string();
        c
    }

    #[test]
    fn defaults_are_off_and_valid() {
        let c = TranslationConfig::default();
        assert!(!c.is_configured());
        assert_eq!(
            (c.max_in_flight, c.timeout_secs, c.chunk_cues),
            (2, 180, 40)
        );
        assert_eq!(c.callback.timeout_secs, 3600);
        assert!(c.validate().is_ok());
        let parsed: TranslationConfig = toml::from_str("").unwrap();
        assert_eq!(parsed, c);
    }

    #[test]
    fn chat_needs_a_url_and_a_model() {
        assert!(chat().validate().is_ok());
        for bad in ["", "ollama:11434", "ftp://x", "http://", "http://:80"] {
            let mut c = chat();
            c.chat.base_url = bad.to_string();
            assert!(c.validate().is_err(), "{bad}");
        }
        let mut c = chat();
        c.chat.model = " ".to_string();
        assert!(c.validate().unwrap_err().to_string().contains("model"));
        // an empty key is fine
        assert!(chat().chat.api_key.is_empty());
    }

    #[test]
    fn callback_needs_a_url_and_a_base_url() {
        assert!(callback().validate().is_ok());
        let mut c = callback();
        c.callback.callback_base_url.clear();
        let e = c.validate().unwrap_err().to_string();
        assert!(e.contains("callback_base_url"), "{e}");
        let mut c = callback();
        c.callback.url = "svc".to_string();
        assert!(c.validate().is_err());
        for bad in [
            "nas.lan:8088",
            "http://u:p@nas",
            "http://nas/?a=1",
            "http://nas/#x",
            "http://",
        ] {
            let mut c = callback();
            c.callback.callback_base_url = bad.to_string();
            assert!(c.validate().is_err(), "{bad}");
        }
    }

    #[test]
    fn base_url_loses_a_trailing_slash_and_keeps_a_prefix() {
        let mut c = callback();
        c.callback.callback_base_url = "https://nas.lan/putio/".to_string();
        c.normalize().unwrap();
        assert_eq!(c.callback.callback_base_url, "https://nas.lan/putio");
    }

    #[test]
    fn only_the_selected_kind_is_checked() {
        let mut c = TranslationConfig::default();
        c.chat.base_url = "junk".to_string();
        c.callback.url = "junk".to_string();
        assert!(c.validate().is_ok());
        c.kind = TranslatorKind::Callback;
        assert!(c.validate().is_err());
        c.kind = TranslatorKind::Chat;
        assert!(c.validate().is_err());
    }

    #[test]
    fn ranges_are_enforced() {
        type Edit = Box<dyn Fn(&mut TranslationConfig)>;
        let cases: Vec<Edit> = vec![
            Box::new(|c| c.max_in_flight = 0),
            Box::new(|c| c.max_in_flight = 9),
            Box::new(|c| c.timeout_secs = 9),
            Box::new(|c| c.timeout_secs = 1801),
            Box::new(|c| c.chunk_cues = 4),
            Box::new(|c| c.chunk_cues = 201),
            Box::new(|c| c.instruction = "x".repeat(501)),
        ];
        for mutate in cases {
            let mut c = chat();
            mutate(&mut c);
            assert!(c.validate().is_err());
        }
        for mutate in [
            (|c: &mut TranslationConfig| c.max_in_flight = 8) as fn(&mut TranslationConfig),
            |c| c.timeout_secs = 1800,
            |c| c.chunk_cues = 5,
            |c| c.instruction = "x".repeat(500),
        ] {
            let mut c = chat();
            mutate(&mut c);
            assert!(c.validate().is_ok());
        }
        let mut c = callback();
        c.callback.timeout_secs = 59;
        assert!(c.validate().is_err());
        c.callback.timeout_secs = 86401;
        assert!(c.validate().is_err());
        c.callback.timeout_secs = 86400;
        assert!(c.validate().is_ok());
        // the callback timeout does not matter for chat
        let mut c = chat();
        c.callback.timeout_secs = 1;
        assert!(c.validate().is_ok());
    }

    #[test]
    fn instruction_newlines_become_spaces() {
        let mut c = chat();
        c.instruction = "informal\r\nSpanish\n\n(Spain) ".to_string();
        c.normalize().unwrap();
        assert_eq!(c.instruction, "informal Spanish (Spain)");
    }

    #[test]
    fn headers_are_checked() {
        let mut c = callback();
        c.callback.headers.insert("Bad Name".into(), "x".into());
        assert!(c.validate().is_err());
        let mut c = callback();
        c.callback.headers.insert("X-Key".into(), "a\nb".into());
        assert!(c.validate().is_err());
        let mut c = callback();
        for i in 0..21 {
            c.callback.headers.insert(format!("X-H{i}"), "v".into());
        }
        assert!(c.validate().is_err());
    }

    #[test]
    fn mask_and_unmask_keep_secrets() {
        let mut stored = chat();
        stored.chat.api_key = "sk-secret".to_string();
        stored
            .callback
            .headers
            .insert("Authorization".into(), "Bearer abc".into());
        let masked = stored.masked();
        assert_eq!(masked.chat.api_key, MASK);
        assert_eq!(masked.callback.headers["Authorization"], MASK);
        // empty key stays empty
        assert_eq!(chat().masked().chat.api_key, "");
        let back = masked.unmasked(&stored);
        assert_eq!(back, stored);
        // a new value replaces the stored one
        let mut edited = stored.masked();
        edited.chat.api_key = "sk-new".to_string();
        assert_eq!(edited.unmasked(&stored).chat.api_key, "sk-new");
    }

    #[test]
    fn a_leftover_mask_for_a_new_header_is_an_error() {
        let stored = callback();
        let mut new = callback();
        new.callback.headers.insert("X-New".into(), MASK.into());
        let resolved = new.unmasked(&stored);
        let e = resolved.validate().unwrap_err().to_string();
        assert!(e.contains("X-New") && e.contains("masked"), "{e}");
    }

    #[test]
    fn debug_never_prints_secrets() {
        let mut c = callback();
        c.chat.api_key = "sk-very-secret".to_string();
        c.callback
            .headers
            .insert("Authorization".into(), "Bearer hunter2".into());
        let text = format!("{c:?}");
        assert!(!text.contains("sk-very-secret"), "{text}");
        assert!(!text.contains("hunter2"), "{text}");
        assert!(text.contains("<redacted>"));
        assert!(text.contains("Authorization"));
    }

    #[test]
    fn categories_match_case_insensitively() {
        let mut c = chat();
        c.excluded_categories = vec![" Radarr ".to_string(), "".to_string()];
        c.normalize().unwrap();
        assert_eq!(c.excluded_categories, vec!["Radarr"]);
        assert!(c.excludes("radarr"));
        assert!(!c.excludes("sonarr"));
        assert!(!c.excludes(""));
    }

    #[test]
    fn secret_values_lists_key_and_headers() {
        let mut c = callback();
        c.chat.api_key = "k1".into();
        c.callback.headers.insert("X-A".into(), "v1".into());
        let v = c.secret_values();
        assert!(v.contains(&"k1".to_string()) && v.contains(&"v1".to_string()));
    }
}
