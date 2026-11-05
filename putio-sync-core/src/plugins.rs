//! `[[plugins]]`: settings for sandboxed Lua plugins (feature 028)
//!
//! This module holds the settings, their structural checks and the masking of secret
//! values. Values are checked against each plugin's manifest by the `putio-sync-plugins`
//! crate on a Settings save, because a plugin folder can be missing when the config loads.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
pub use crate::hooks::MASK;

/// Most `[[plugins]]` entries, and most plugins loaded
pub const MAX_PLUGINS: usize = 32;
/// Longest call, in seconds
pub const MAX_TIMEOUT_SECS: u64 = 60;
/// Memory limits, in MiB
pub const MEMORY_MB: std::ops::RangeInclusive<u64> = 4..=256;

fn default_timeout() -> u64 {
    10
}

fn default_memory() -> u64 {
    32
}

/// One `[[plugins]]` entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginSettings {
    /// The plugin's manifest name
    pub name: String,
    /// Plugins are off until turned on
    #[serde(default)]
    pub enabled: bool,
    /// Hosts the plugin may call, written by Settings saves
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approved_hosts: Vec<String>,
    /// Longest call, in seconds (1-60)
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Memory for the whole Lua state, in MiB (4-256)
    #[serde(default = "default_memory")]
    pub memory_mb: u64,
    /// Values for the manifest's config fields that are not secret
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config: BTreeMap<String, serde_json::Value>,
    /// Values for secret fields; shown as [`MASK`]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, String>,
}

impl Default for PluginSettings {
    fn default() -> Self {
        PluginSettings {
            name: String::new(),
            enabled: false,
            approved_hosts: Vec::new(),
            timeout_secs: default_timeout(),
            memory_mb: default_memory(),
            config: BTreeMap::new(),
            secrets: BTreeMap::new(),
        }
    }
}

/// Whether `name` is a valid plugin name: `[A-Za-z0-9_-]{1,40}`
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Whether `host` is a plain lowercase host name: no scheme, port, path or spaces
pub fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('.')
        && !host.ends_with('.')
        && !host.contains("..")
        && host
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
}

fn scalar_or_strings(value: &serde_json::Value) -> bool {
    use serde_json::Value;
    match value {
        Value::String(_) | Value::Number(_) | Value::Bool(_) => true,
        // A list of text, or of tables of text such as `path_map` rules (031)
        Value::Array(items) => {
            items.iter().all(Value::is_string)
                || items.iter().all(|item| {
                    item.as_object()
                        .is_some_and(|o| o.values().all(Value::is_string))
                })
        }
        _ => false,
    }
}

impl PluginSettings {
    /// The call timeout
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.timeout_secs)
    }

    /// Check one entry; `index` is its place in the list, for a nameless entry's message
    pub fn validate(&self, index: usize) -> CoreResult<()> {
        let bad = |msg: String| Err(CoreError::ConfigError(msg));
        if self.name.is_empty() {
            return bad(format!("plugins[{}].name: required", index));
        }
        if !valid_name(&self.name) {
            return bad(format!(
                "plugins[{}].name: \"{}\" may only use letters, digits, - and _ (at most 40)",
                index, self.name
            ));
        }
        let plugin = format!("plugins \"{}\"", self.name);
        if !(1..=MAX_TIMEOUT_SECS).contains(&self.timeout_secs) {
            return bad(format!(
                "{}.timeout_secs: must be between 1 and {}",
                plugin, MAX_TIMEOUT_SECS
            ));
        }
        if !MEMORY_MB.contains(&self.memory_mb) {
            return bad(format!(
                "{}.memory_mb: must be between {} and {}",
                plugin,
                MEMORY_MB.start(),
                MEMORY_MB.end()
            ));
        }
        for host in &self.approved_hosts {
            if !valid_host(host) {
                return bad(format!(
                    "{}.approved_hosts: \"{}\" is not a lowercase host name",
                    plugin, host
                ));
            }
        }
        for (key, value) in &self.config {
            if !scalar_or_strings(value) {
                return bad(format!(
                    "{}.config.{}: must be a string, number, true/false, a list of strings or a list of tables of strings",
                    plugin, key
                ));
            }
            if self.secrets.contains_key(key) {
                return bad(format!(
                    "{}.config.{}: also set in secrets; keep it in one place",
                    plugin, key
                ));
            }
        }
        Ok(())
    }
}

/// Check the whole list: each entry, the count and unique names
pub fn validate(plugins: &[PluginSettings]) -> CoreResult<()> {
    if plugins.len() > MAX_PLUGINS {
        return Err(CoreError::ConfigError(format!(
            "plugins: at most {} plugins",
            MAX_PLUGINS
        )));
    }
    let mut seen = HashSet::new();
    for (i, plugin) in plugins.iter().enumerate() {
        plugin.validate(i)?;
        if !seen.insert(plugin.name.as_str()) {
            return Err(CoreError::ConfigError(format!(
                "plugins[{}].name: \"{}\" is listed twice",
                i, plugin.name
            )));
        }
    }
    Ok(())
}

/// The list with every secret value replaced by [`MASK`]
pub fn mask(plugins: &[PluginSettings]) -> Vec<PluginSettings> {
    plugins
        .iter()
        .map(|p| {
            let mut p = p.clone();
            p.secrets.values_mut().for_each(|v| *v = MASK.to_string());
            p
        })
        .collect()
}

/// Put the stored value back wherever `new` sends [`MASK`] for a plugin and key that
/// `current` has. A mask with nothing to resolve is refused.
pub fn unmask(
    new: &[PluginSettings],
    current: &[PluginSettings],
) -> CoreResult<Vec<PluginSettings>> {
    let mut out = new.to_vec();
    for plugin in &mut out {
        let stored = current
            .iter()
            .find(|c| c.name == plugin.name)
            .map(|c| &c.secrets);
        for (key, value) in plugin.secrets.iter_mut() {
            if value != MASK {
                continue;
            }
            match stored.and_then(|s| s.get(key)) {
                Some(v) => *value = v.clone(),
                None => {
                    return Err(CoreError::ConfigError(format!(
                        "plugins \"{}\".secrets.{}: still masked; type the value again",
                        plugin.name, key
                    )))
                }
            }
        }
    }
    Ok(out)
}

/// The plugins folder: `plugins_dir` when set, else `plugins` next to the config file.
/// `None` without either (tests and setups with no config file).
pub fn dir_for(plugins_dir: Option<&Path>, config_path: Option<&Path>) -> Option<PathBuf> {
    match plugins_dir {
        Some(dir) => Some(dir.to_path_buf()),
        None => config_path
            .and_then(Path::parent)
            .map(|parent| parent.join("plugins")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ntfy() -> PluginSettings {
        PluginSettings {
            name: "ntfy".to_string(),
            enabled: true,
            approved_hosts: vec!["ntfy.sh".to_string()],
            config: BTreeMap::from([("topic".to_string(), serde_json::json!("my-nas"))]),
            secrets: BTreeMap::from([("token".to_string(), "tk_abcdef".to_string())]),
            ..Default::default()
        }
    }

    fn err(p: PluginSettings) -> String {
        validate(&[p]).unwrap_err().to_string()
    }

    #[test]
    fn defaults_from_toml() {
        let p: PluginSettings = toml::from_str("name = \"ntfy\"").unwrap();
        assert!(!p.enabled);
        assert_eq!(p.timeout_secs, 10);
        assert_eq!(p.memory_mb, 32);
        assert!(p.config.is_empty() && p.secrets.is_empty() && p.approved_hosts.is_empty());
        validate(&[p]).unwrap();
    }

    #[test]
    fn name_rules() {
        let mut p = ntfy();
        p.name = String::new();
        assert!(err(p.clone()).contains("plugins[0].name: required"));
        p.name = "my plugin".to_string();
        assert!(err(p.clone()).contains("letters, digits"));
        p.name = "a".repeat(41);
        assert!(err(p).contains("at most 40"));
        let e = validate(&[ntfy(), ntfy()]).unwrap_err().to_string();
        assert!(
            e.contains("plugins[1].name: \"ntfy\" is listed twice"),
            "{}",
            e
        );
        let many: Vec<_> = (0..33)
            .map(|i| PluginSettings {
                name: format!("p{}", i),
                ..Default::default()
            })
            .collect();
        assert!(validate(&many)
            .unwrap_err()
            .to_string()
            .contains("at most 32"));
    }

    #[test]
    fn ranges() {
        let mut p = ntfy();
        p.timeout_secs = 0;
        assert!(err(p.clone()).contains("timeout_secs: must be between 1 and 60"));
        p.timeout_secs = 61;
        assert!(err(p.clone()).contains("between 1 and 60"));
        p.timeout_secs = 60;
        p.memory_mb = 3;
        assert!(err(p.clone()).contains("memory_mb: must be between 4 and 256"));
        p.memory_mb = 257;
        assert!(err(p.clone()).contains("between 4 and 256"));
        p.memory_mb = 4;
        validate(&[p]).unwrap();
    }

    #[test]
    fn host_syntax() {
        for bad in [
            "https://ntfy.sh",
            "ntfy.sh:443",
            "NTFY.sh",
            "ntfy.sh/x",
            "",
            ".sh",
            "a..b",
        ] {
            let mut p = ntfy();
            p.approved_hosts = vec![bad.to_string()];
            assert!(err(p).contains("is not a lowercase host name"), "{}", bad);
        }
        assert!(valid_host("discord.com") && valid_host("10.0.0.2") && valid_host("nas-1"));
    }

    #[test]
    fn values_are_scalars_or_string_lists() {
        let mut p = ntfy();
        p.config.insert(
            "notify".to_string(),
            serde_json::json!(["download.finished"]),
        );
        p.config
            .insert("group_minutes".to_string(), serde_json::json!(5));
        p.config.insert("on".to_string(), serde_json::json!(true));
        p.config.insert(
            "path_map".to_string(),
            serde_json::json!([{"from": "/downloads", "to": "/data"}]),
        );
        validate(&[p.clone()]).unwrap();
        p.config
            .insert("x".to_string(), serde_json::json!([{"from": 1}]));
        assert!(err(p.clone()).contains("config.x"));
        p.config
            .insert("x".to_string(), serde_json::json!({"a": 1}));
        assert!(err(p.clone()).contains("config.x: must be a string"));
        p.config.insert("x".to_string(), serde_json::json!([1, 2]));
        assert!(err(p.clone()).contains("config.x"));
        p.config.remove("x");
        p.config.insert("token".to_string(), serde_json::json!("t"));
        assert!(err(p).contains("config.token: also set in secrets"));
    }

    #[test]
    fn mask_and_unmask() {
        let stored = vec![ntfy()];
        let masked = mask(&stored);
        assert_eq!(masked[0].secrets["token"], MASK);
        assert_eq!(masked[0].config, stored[0].config);
        // Sending the mask back keeps the stored value.
        assert_eq!(unmask(&masked, &stored).unwrap(), stored);
        // A new value replaces it.
        let mut changed = masked.clone();
        changed[0]
            .secrets
            .insert("token".to_string(), "tk_new".to_string());
        assert_eq!(
            unmask(&changed, &stored).unwrap()[0].secrets["token"],
            "tk_new"
        );
        // A mask with nothing behind it is refused, naming plugin and key.
        let e = unmask(&masked, &[]).unwrap_err().to_string();
        assert!(
            e.contains("plugins \"ntfy\".secrets.token: still masked"),
            "{}",
            e
        );
        assert!(!e.contains("tk_abcdef"));
    }

    #[test]
    fn config_round_trip() {
        let mut plex = ntfy();
        plex.name = "plex".to_string();
        plex.config.insert(
            "path_map".to_string(),
            serde_json::json!([{"from": "/downloads", "to": "D:\\Media"}]),
        );
        let config = crate::config::Config {
            plugins_dir: Some(PathBuf::from("/config/plugins")),
            plugins: vec![ntfy(), plex],
            ..Default::default()
        };
        let text = toml::to_string(&config).unwrap();
        let back: crate::config::Config = toml::from_str(&text).unwrap();
        assert_eq!(back.plugins_dir, config.plugins_dir);
        assert_eq!(back.plugins, config.plugins);
        assert!(!toml::to_string(&crate::config::Config::default())
            .unwrap()
            .contains("plugins"));
    }

    #[test]
    fn plugins_folder() {
        assert_eq!(
            dir_for(None, Some(Path::new("/config/config.toml"))),
            Some(PathBuf::from("/config/plugins"))
        );
        assert_eq!(
            dir_for(
                Some(Path::new("/data/p")),
                Some(Path::new("/config/config.toml"))
            ),
            Some(PathBuf::from("/data/p"))
        );
        assert_eq!(dir_for(None, None), None);
    }
}
