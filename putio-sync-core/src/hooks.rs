//! `[[hooks]]`: run a command or call a URL for events on the bus (feature 027)
//!
//! This module holds the config, its checks and the masking of header and env values,
//! which may be secrets. The runner lives in the `putio-sync-hooks` crate.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::events::EventType;

/// What the settings view and export show in place of a header or env value. A save that
/// sends it back keeps the stored value.
pub const MASK: &str = "********";

/// Most hooks in one config
pub const MAX_HOOKS: usize = 32;
/// Most headers on one hook
pub const MAX_HEADERS: usize = 32;
/// Longest run, in seconds
pub const MAX_TIMEOUT_SECS: u64 = 600;
/// Matches every event type except `hook.test` and `plugin.test`
pub const ALL_EVENTS: &str = "*";

/// Env names putio-sync sets for every command
pub const RESERVED_ENV: [&str; 3] = [
    "PUTIO_SYNC_EVENT_TYPE",
    "PUTIO_SYNC_EVENT_ID",
    "PUTIO_SYNC_HOOK_NAME",
];

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    30
}

/// One `[[hooks]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookConfig {
    /// Unique, `[A-Za-z0-9_-]{1,40}`; used in logs and metrics
    pub name: String,
    /// Off keeps the hook in the config without running it
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Event types to run for, or `"*"` for all
    #[serde(default)]
    pub events: Vec<String>,
    /// Absolute program path and its arguments
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    /// http or https URL to POST the event to
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Extra request headers, URL hooks only
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// Extra environment, command hooks only
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Longest a run (each webhook attempt) may take, 1..=600
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Set: a pipeline hook that runs as a step of this stage, not for events (feature 038)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<HookStage>,
    /// Pipeline hooks: a failure holds the item (`true`) or is retried and then given up
    #[serde(default = "default_true")]
    pub required: bool,
    /// Pipeline hooks: categories to run for; empty means all
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
}

/// The pipeline stages a hook can run in (feature 038)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookStage {
    /// Before the local download starts
    PreDownload,
    /// After the download, before subtitles
    PreProcessing,
    /// After subtitles and translation, before notifications
    PostProcessing,
}

impl HookStage {
    /// The config and API name
    pub fn as_str(&self) -> &'static str {
        match self {
            HookStage::PreDownload => "pre_download",
            HookStage::PreProcessing => "pre_processing",
            HookStage::PostProcessing => "post_processing",
        }
    }
}

impl Default for HookConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            events: Vec::new(),
            command: None,
            url: None,
            headers: BTreeMap::new(),
            env: BTreeMap::new(),
            timeout_secs: default_timeout(),
            stage: None,
            required: true,
            categories: Vec::new(),
        }
    }
}

/// How a hook delivers events
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookKind {
    /// Runs a program with the event on stdin
    Command,
    /// POSTs the event to a URL
    Url,
}

impl HookConfig {
    /// Command or URL; a hook with neither counts as a command (validation refuses it)
    pub fn kind(&self) -> HookKind {
        if self.url.is_some() {
            HookKind::Url
        } else {
            HookKind::Command
        }
    }

    /// What logs and the API show: the redacted URL, or the program path without its
    /// arguments, which may hold secrets
    pub fn target_label(&self) -> String {
        match self.kind() {
            HookKind::Url => crate::redact::url(self.url.as_deref().unwrap_or_default()),
            HookKind::Command => self
                .command
                .as_ref()
                .and_then(|c| c.first())
                .cloned()
                .unwrap_or_default(),
        }
    }

    /// Whether the hook runs for `event_type`. `*` matches all but `hook.test` and
    /// `plugin.test`, which only the test buttons send.
    pub fn matches(&self, event_type: EventType) -> bool {
        if event_type.is_test() || self.stage.is_some() {
            return false;
        }
        self.events
            .iter()
            .any(|e| e == ALL_EVENTS || e == event_type.as_str())
    }

    /// The run timeout
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.timeout_secs)
    }

    /// Check one hook; `index` is its place in the list, for a nameless hook's message
    pub fn validate(&self, index: usize) -> CoreResult<()> {
        let bad = |msg: String| Err(CoreError::ConfigError(msg));
        if self.name.is_empty() {
            return bad(format!("hooks[{}].name: required", index));
        }
        if self.name.len() > 40
            || !self
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return bad(format!(
                "hooks[{}].name: \"{}\" may only use letters, digits, - and _ (at most 40)",
                index, self.name
            ));
        }
        let hook = format!("hooks \"{}\"", self.name);
        match (&self.command, &self.url) {
            (Some(_), Some(_)) => {
                return bad(format!("{}: set either command or url, not both", hook))
            }
            (None, None) => return bad(format!("{}: set command or url", hook)),
            (Some(command), None) => match command.first() {
                None => return bad(format!("{}.command: must name a program", hook)),
                Some(program) if !Path::new(program).is_absolute() => {
                    return bad(format!(
                        "{}.command: the program must be an absolute path",
                        hook
                    ))
                }
                Some(_) => {}
            },
            (None, Some(url)) => {
                let ok = reqwest::Url::parse(url).is_ok_and(|u| {
                    matches!(u.scheme(), "http" | "https") && u.host_str().is_some()
                });
                if !ok {
                    return bad(format!("{}.url: must be an http or https URL", hook));
                }
                if url.ends_with('…') {
                    return bad(format!(
                        "{}.url: still shortened; type the full URL again",
                        hook
                    ));
                }
            }
        }
        if self.stage.is_some() {
            if !self.events.is_empty() {
                return bad(format!(
                    "{}.events: a pipeline hook (stage set) runs as a step, not for events",
                    hook
                ));
            }
            if self.categories.iter().any(|c| c.trim().is_empty()) {
                return bad(format!("{}.categories: entries must not be empty", hook));
            }
        } else if self.events.is_empty() {
            return bad(format!(
                "{}.events: list at least one event, or \"*\" for all",
                hook
            ));
        } else if !self.categories.is_empty() {
            return bad(format!(
                "{}.categories: only for pipeline hooks (set stage)",
                hook
            ));
        }
        for event in &self.events {
            if event == ALL_EVENTS {
                continue;
            }
            match event.parse::<EventType>() {
                Err(e) => return bad(format!("{}.events: {}", hook, e)),
                Ok(t) if t.is_test() => {
                    return bad(format!(
                        "{}.events: {} is only sent by the test button",
                        hook, t
                    ))
                }
                Ok(_) => {}
            }
        }
        if !(1..=MAX_TIMEOUT_SECS).contains(&self.timeout_secs) {
            return bad(format!(
                "{}.timeout_secs: must be between 1 and {}",
                hook, MAX_TIMEOUT_SECS
            ));
        }
        if !self.headers.is_empty() && self.kind() != HookKind::Url {
            return bad(format!("{}.headers: only for url hooks", hook));
        }
        if !self.env.is_empty() && self.kind() != HookKind::Command {
            return bad(format!("{}.env: only for command hooks", hook));
        }
        if self.headers.len() > MAX_HEADERS {
            return bad(format!("{}.headers: at most {} headers", hook, MAX_HEADERS));
        }
        for (name, value) in &self.headers {
            if reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_err() {
                return bad(format!(
                    "{}.headers: \"{}\" is not a valid header name",
                    hook, name
                ));
            }
            if value == MASK {
                return bad(format!(
                    "{}.headers.{}: still masked; type the value again",
                    hook, name
                ));
            }
            if reqwest::header::HeaderValue::from_str(value).is_err() {
                return bad(format!(
                    "{}.headers.{}: the value has characters a header cannot carry",
                    hook, name
                ));
            }
        }
        for (name, value) in &self.env {
            let mut chars = name.chars();
            let valid = chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !valid {
                return bad(format!(
                    "{}.env: \"{}\" is not a valid variable name",
                    hook, name
                ));
            }
            if name.starts_with("PUTIO_SYNC_EVENT_")
                || name.starts_with("PUTIO_SYNC_ITEM_")
                || RESERVED_ENV.contains(&name.as_str())
            {
                return bad(format!("{}.env: {} is set by putio-sync", hook, name));
            }
            if value == MASK {
                return bad(format!(
                    "{}.env.{}: still masked; type the value again",
                    hook, name
                ));
            }
        }
        Ok(())
    }
}

/// Check the whole list: each hook, the count and unique names
pub fn validate(hooks: &[HookConfig]) -> CoreResult<()> {
    if hooks.len() > MAX_HOOKS {
        return Err(CoreError::ConfigError(format!(
            "hooks: at most {} hooks",
            MAX_HOOKS
        )));
    }
    let mut seen = HashSet::new();
    for (i, hook) in hooks.iter().enumerate() {
        hook.validate(i)?;
        if !seen.insert(hook.name.as_str()) {
            return Err(CoreError::ConfigError(format!(
                "hooks[{}].name: \"{}\" is used by another hook",
                i, hook.name
            )));
        }
    }
    Ok(())
}

/// The list with every header and env value replaced by [`MASK`], and each URL that
/// could hold a secret (a query, user info or a deeper path) cut down by
/// [`crate::redact::url`]
pub fn mask(hooks: &[HookConfig]) -> Vec<HookConfig> {
    hooks
        .iter()
        .map(|h| {
            let mut h = h.clone();
            if let Some(url) = h.url.as_mut() {
                *url = crate::redact::url(url);
            }
            h.headers.values_mut().for_each(|v| *v = MASK.to_string());
            h.env.values_mut().for_each(|v| *v = MASK.to_string());
            h
        })
        .collect()
}

/// Put the stored value back wherever `new` sends [`MASK`] for a hook name and key that
/// `current` has, and the stored URL where `new` sends its shortened form. A mask with
/// nothing to resolve is refused.
pub fn unmask(new: &[HookConfig], current: &[HookConfig]) -> CoreResult<Vec<HookConfig>> {
    let mut out = new.to_vec();
    for hook in &mut out {
        let old = current.iter().find(|c| c.name == hook.name);
        let stored_url = old.and_then(|o| o.url.as_deref());
        if let (Some(url), Some(stored)) = (hook.url.as_mut(), stored_url) {
            if *url != stored && *url == crate::redact::url(stored) {
                *url = stored.to_string();
            }
        }
        if hook.url.as_deref().is_some_and(|u| u.ends_with('…')) {
            return Err(CoreError::ConfigError(format!(
                "hooks \"{}\".url: still shortened; type the full URL again",
                hook.name
            )));
        }
        for (field, values, stored) in [
            ("headers", &mut hook.headers, old.map(|o| &o.headers)),
            ("env", &mut hook.env, old.map(|o| &o.env)),
        ] {
            for (key, value) in values.iter_mut() {
                if value != MASK {
                    continue;
                }
                match stored.and_then(|s| s.get(key)) {
                    Some(v) => *value = v.clone(),
                    None => {
                        return Err(CoreError::ConfigError(format!(
                            "hooks \"{}\".{}.{}: still masked; type the value again",
                            hook.name, field, key
                        )))
                    }
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path_utils::test_abs;

    fn command_hook() -> HookConfig {
        HookConfig {
            name: "on-download".to_string(),
            events: vec!["download.finished".to_string()],
            command: Some(vec![test_abs("/config/hooks/on-download.py")]),
            env: BTreeMap::from([("LIBRARY".to_string(), "/data".to_string())]),
            ..Default::default()
        }
    }

    fn url_hook() -> HookConfig {
        HookConfig {
            name: "ha".to_string(),
            events: vec!["*".to_string()],
            url: Some("https://ha.local:8123/api/webhook/abc".to_string()),
            headers: BTreeMap::from([("Authorization".to_string(), "Bearer abc".to_string())]),
            timeout_secs: 10,
            ..Default::default()
        }
    }

    fn err(hook: HookConfig) -> String {
        hook.validate(2).unwrap_err().to_string()
    }

    #[test]
    fn good_hooks_pass_and_parse_from_toml() {
        validate(&[command_hook(), url_hook()]).unwrap();
        let hook: HookConfig =
            toml::from_str("name = \"x\"\nevents = [\"storage.low\"]\nurl = \"http://10.0.0.2/h\"")
                .unwrap();
        assert!(hook.enabled);
        assert_eq!(hook.timeout_secs, 30);
        hook.validate(0).unwrap();
    }

    fn stage_hook() -> HookConfig {
        HookConfig {
            name: "tidy".to_string(),
            stage: Some(HookStage::PostProcessing),
            command: Some(vec![test_abs("/config/tidy.sh")]),
            ..Default::default()
        }
    }

    /// Feature 038: a stage hook takes no events, and its extra fields are checked
    #[test]
    fn pipeline_hooks_are_checked() {
        stage_hook().validate(0).unwrap();
        let hook: HookConfig = toml::from_str(&format!(
            "name = \"t\"\nstage = \"pre_download\"\nrequired = false\ncategories = [\"sonarr\"]\ncommand = ['{}']",
            test_abs("/a")
        ))
        .unwrap();
        assert_eq!(hook.stage, Some(HookStage::PreDownload));
        assert!(!hook.required);
        hook.validate(0).unwrap();
        assert!(stage_hook().required, "required defaults to true");

        let mut h = stage_hook();
        h.events = vec!["*".to_string()];
        assert!(err(h).contains("runs as a step, not for events"));
        let mut h = stage_hook();
        h.categories = vec!["tv".to_string(), " ".to_string()];
        assert!(err(h).contains("categories: entries must not be empty"));
        let mut h = command_hook();
        h.categories = vec!["tv".to_string()];
        assert!(err(h).contains("categories: only for pipeline hooks"));
        let mut h = stage_hook();
        h.env = BTreeMap::from([("PUTIO_SYNC_ITEM_KEY".to_string(), "x".to_string())]);
        assert!(err(h).contains("is set by putio-sync"));
        let mut h = stage_hook();
        h.stage = Some(HookStage::PreProcessing);
        assert!(toml::from_str::<HookConfig>("name = \"t\"\nstage = \"subtitles\"").is_err());
        // An event hook never matches as a step, and a step never matches an event
        h.events.clear();
        assert!(!h.matches(EventType::DownloadFinished));
    }

    #[test]
    fn names_are_checked() {
        let mut h = command_hook();
        h.name = String::new();
        assert!(err(h.clone()).contains("hooks[2].name: required"));
        h.name = "my hook".to_string();
        assert!(err(h.clone()).contains("may only use letters"));
        h.name = "a".repeat(41);
        assert!(err(h).contains("may only use letters"));
        let e = validate(&[command_hook(), command_hook()]).unwrap_err();
        assert!(e.to_string().contains("used by another hook"));
        let many: Vec<HookConfig> = (0..33)
            .map(|i| HookConfig {
                name: format!("h{}", i),
                ..command_hook()
            })
            .collect();
        assert!(validate(&many)
            .unwrap_err()
            .to_string()
            .contains("at most 32"));
    }

    #[test]
    fn exactly_one_target() {
        let mut h = command_hook();
        h.url = Some("https://x".to_string());
        h.env.clear();
        assert!(err(h.clone()).contains("either command or url"));
        h.command = None;
        h.url = None;
        assert!(err(h).contains("set command or url"));
    }

    #[test]
    fn command_needs_an_absolute_program() {
        let mut h = command_hook();
        h.command = Some(vec!["on-download.py".to_string()]);
        assert!(err(h.clone()).contains("absolute path"));
        h.command = Some(vec![]);
        assert!(err(h).contains("must name a program"));
    }

    #[test]
    fn url_must_be_http() {
        let mut h = url_hook();
        for url in ["ftp://h/x", "not a url", "file:///etc/passwd"] {
            h.url = Some(url.to_string());
            assert!(err(h.clone()).contains("http or https URL"), "{}", url);
        }
    }

    #[test]
    fn events_are_checked() {
        let mut h = command_hook();
        h.events = vec![];
        assert!(err(h.clone()).contains("at least one event"));
        h.events = vec!["download.done".to_string()];
        assert!(err(h.clone()).contains("unknown event \"download.done\""));
        h.events = vec!["hook.test".to_string()];
        assert!(err(h.clone()).contains("only sent by the test button"));
        h.events = vec!["plugin.test".to_string()];
        assert!(err(h).contains("plugin.test is only sent by the test button"));
    }

    #[test]
    fn timeout_range() {
        let mut h = command_hook();
        h.timeout_secs = 0;
        assert!(err(h.clone()).contains("between 1 and 600"));
        h.timeout_secs = 601;
        assert!(err(h.clone()).contains("between 1 and 600"));
        h.timeout_secs = 600;
        h.validate(0).unwrap();
    }

    #[test]
    fn headers_and_env_belong_to_their_kind() {
        let mut h = command_hook();
        h.headers.insert("X-A".to_string(), "1".to_string());
        assert!(err(h).contains("headers: only for url hooks"));
        let mut h = url_hook();
        h.env.insert("A".to_string(), "1".to_string());
        assert!(err(h).contains("env: only for command hooks"));
    }

    #[test]
    fn header_and_env_names_and_values() {
        let mut h = url_hook();
        h.headers.insert("Bad Name".to_string(), "1".to_string());
        assert!(err(h).contains("\"Bad Name\" is not a valid header name"));
        let mut h = url_hook();
        h.headers
            .insert("X-A".to_string(), "line\nbreak".to_string());
        assert!(err(h).contains("cannot carry"));
        let mut h = url_hook();
        h.headers = (0..33)
            .map(|i| (format!("X-{}", i), "v".to_string()))
            .collect();
        assert!(err(h).contains("at most 32 headers"));
        let mut h = command_hook();
        h.env.insert("1BAD".to_string(), "1".to_string());
        assert!(err(h).contains("not a valid variable name"));
        let mut h = command_hook();
        h.env
            .insert("PUTIO_SYNC_EVENT_ID".to_string(), "1".to_string());
        assert!(err(h).contains("PUTIO_SYNC_EVENT_ID is set by putio-sync"));
        let mut h = command_hook();
        h.env
            .insert("PUTIO_SYNC_HOOK_NAME".to_string(), "1".to_string());
        assert!(err(h).contains("set by putio-sync"));
    }

    #[test]
    fn a_leftover_mask_is_refused() {
        let mut h = url_hook();
        h.headers
            .insert("Authorization".to_string(), MASK.to_string());
        assert!(err(h).contains("headers.Authorization: still masked"));
        let mut h = command_hook();
        h.env.insert("LIBRARY".to_string(), MASK.to_string());
        assert!(err(h).contains("env.LIBRARY: still masked"));
    }

    #[test]
    fn matching() {
        let h = command_hook();
        assert!(h.matches(EventType::DownloadFinished));
        assert!(!h.matches(EventType::DownloadFailed));
        let all = url_hook();
        assert!(all.matches(EventType::StorageLow));
        assert!(!all.matches(EventType::HookTest));
        assert!(!all.matches(EventType::PluginTest));
    }

    #[test]
    fn target_labels_hide_secrets() {
        let mut h = url_hook();
        h.url = Some("https://h/api/webhook/abc?token=x".to_string());
        assert_eq!(h.target_label(), "https://h/api/…");
        let mut c = command_hook();
        c.command = Some(vec!["/bin/hook".to_string(), "--key=secret".to_string()]);
        assert_eq!(c.target_label(), "/bin/hook");
        assert_eq!(c.kind(), HookKind::Command);
        assert_eq!(h.kind(), HookKind::Url);
    }

    #[test]
    fn mask_and_unmask_round_trip() {
        let current = vec![command_hook(), url_hook()];
        let masked = mask(&current);
        assert_eq!(masked[0].env["LIBRARY"], MASK);
        assert_eq!(masked[1].headers["Authorization"], MASK);
        assert_eq!(unmask(&masked, &current).unwrap(), current);

        // A changed value wins; a new key with the mask has nothing to resolve.
        let mut edited = masked.clone();
        edited[1]
            .headers
            .insert("Authorization".to_string(), "Bearer new".to_string());
        let out = unmask(&edited, &current).unwrap();
        assert_eq!(out[1].headers["Authorization"], "Bearer new");
        let mut edited = masked.clone();
        edited[1]
            .headers
            .insert("X-New".to_string(), MASK.to_string());
        let e = unmask(&edited, &current).unwrap_err().to_string();
        assert!(
            e.contains("hooks \"ha\".headers.X-New: still masked"),
            "{}",
            e
        );

        // A renamed hook cannot take another hook's secrets.
        let mut renamed = masked;
        renamed[1].name = "other".to_string();
        assert!(unmask(&renamed, &current).is_err());

        // A URL with a secret path or query is shortened and comes back whole.
        let mut secret = url_hook();
        secret.url = Some("https://ha.local:8123/api/webhook/abc?token=t".to_string());
        let current = vec![secret];
        let masked = mask(&current);
        assert_eq!(
            masked[0].url.as_deref(),
            Some("https://ha.local:8123/api/…")
        );
        assert_eq!(unmask(&masked, &current).unwrap(), current);
        let mut edited = masked.clone();
        edited[0].url = Some("https://ha.local:8123/new".to_string());
        assert_eq!(
            unmask(&edited, &current).unwrap()[0].url.as_deref(),
            edited[0].url.as_deref()
        );
        let mut renamed = masked;
        renamed[0].name = "other".to_string();
        let e = unmask(&renamed, &current).unwrap_err().to_string();
        assert!(e.contains("url: still shortened"), "{}", e);
    }
}
