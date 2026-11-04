//! `plugin.toml`: what a plugin is, which events it wants, what it may do and which
//! settings it takes (contracts/manifest.md). The same file is meant for 029's
//! WebAssembly plugins, with `runtime = "wasm"`.

use std::collections::HashSet;

use putio_sync_core::events::EventType;
use putio_sync_core::hooks::ALL_EVENTS;
use putio_sync_core::plugins::{valid_host, valid_name, PluginSettings};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Most config fields in one manifest
pub const MAX_FIELDS: usize = 32;

fn default_runtime() -> String {
    "lua".to_string()
}

fn default_entry() -> String {
    "main.lua".to_string()
}

/// A parsed and checked `plugin.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// `[A-Za-z0-9_-]{1,40}`
    pub name: String,
    /// 1-32 printable characters
    pub version: String,
    /// At most 300 characters
    #[serde(default)]
    pub description: String,
    /// `lua`; `wasm` is kept for 029
    #[serde(default = "default_runtime")]
    pub runtime: String,
    /// The script, a plain file name in the plugin's folder
    #[serde(default = "default_entry")]
    pub entry: String,
    /// Event types, or `*` for all
    #[serde(default)]
    pub events: Vec<String>,
    /// What the plugin may do
    #[serde(default)]
    pub permissions: Permissions,
    /// Settings the plugin takes
    #[serde(default)]
    pub config: Vec<ConfigField>,
}

/// `[permissions]`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    /// `[permissions.http]`
    #[serde(default)]
    pub http: HttpPermission,
    /// A small key-value store kept across restarts
    #[serde(default)]
    pub store: bool,
}

/// `[permissions.http]`: the hosts a plugin may call
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpPermission {
    /// Fixed host names
    #[serde(default)]
    pub hosts: Vec<String>,
    /// Keys of `url` fields whose host the plugin may call
    #[serde(default)]
    pub host_from_config: Vec<String>,
}

impl HttpPermission {
    /// Whether the plugin may make HTTP requests at all
    pub fn any(&self) -> bool {
        !self.hosts.is_empty() || !self.host_from_config.is_empty()
    }
}

/// The type of a config field
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    /// Text
    String,
    /// A whole number
    Integer,
    /// Any number
    Number,
    /// true or false
    Boolean,
    /// An http or https URL
    Url,
    /// A list of text
    StringList,
}

/// One `[[config]]` field
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigField {
    /// `[a-z0-9_]{1,40}`
    pub key: String,
    /// The value's type
    #[serde(rename = "type")]
    pub kind: FieldType,
    /// Shown in Settings; the key when empty
    #[serde(default)]
    pub label: String,
    /// One line of help in Settings
    #[serde(default)]
    pub help: String,
    /// The value when none is set
    #[serde(default)]
    pub default: Option<Value>,
    /// The plugin cannot run without a value
    #[serde(default)]
    pub required: bool,
    /// Kept in `[plugins.secrets]`, masked and never logged
    #[serde(default)]
    pub secret: bool,
}

/// Whether `value` has the field's type; a `url` must be http(s) with a host
fn type_ok(kind: FieldType, value: &Value) -> bool {
    match kind {
        FieldType::String => value.is_string(),
        FieldType::Integer => value.is_i64() || value.is_u64(),
        FieldType::Number => value.is_number(),
        FieldType::Boolean => value.is_boolean(),
        FieldType::Url => value.as_str().is_some_and(|s| url_host(s).is_some()),
        FieldType::StringList => value
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_string)),
    }
}

fn type_name(kind: FieldType) -> &'static str {
    match kind {
        FieldType::String => "text",
        FieldType::Integer => "a whole number",
        FieldType::Number => "a number",
        FieldType::Boolean => "true or false",
        FieldType::Url => "an http or https URL",
        FieldType::StringList => "a list of text",
    }
}

/// The lowercase host of an http(s) URL, without the port
pub fn url_host(url: &str) -> Option<String> {
    let u = reqwest::Url::parse(url).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    u.host_str().filter(|h| !h.is_empty()).map(|h| {
        h.trim_start_matches('[')
            .trim_end_matches(']')
            .to_lowercase()
    })
}

impl Manifest {
    /// Parse and check `text`; `file` names the file in messages
    pub fn parse(text: &str, file: &str) -> Result<Manifest, String> {
        let manifest: Manifest = toml::from_str(text).map_err(|e| {
            let msg = e.message().to_string();
            format!("{}: {}", file, msg)
        })?;
        manifest
            .validate()
            .map_err(|e| format!("{}: {}", file, e))?;
        Ok(manifest)
    }

    /// Check every field rule; the message names the field
    pub fn validate(&self) -> Result<(), String> {
        if !valid_name(&self.name) {
            return Err(format!(
                "name: \"{}\" may only use letters, digits, - and _ (1 to 40)",
                self.name
            ));
        }
        let printable = |s: &str| !s.chars().any(char::is_control);
        if self.version.is_empty() || self.version.chars().count() > 32 || !printable(&self.version)
        {
            return Err("version: 1 to 32 printable characters".to_string());
        }
        if self.description.chars().count() > 300 {
            return Err("description: at most 300 characters".to_string());
        }
        match self.runtime.as_str() {
            "lua" => {}
            "wasm" => {
                return Err(
                    "runtime: WebAssembly plugins need a later putio-sync (feature 029)"
                        .to_string(),
                )
            }
            other => return Err(format!("runtime: unknown runtime \"{}\"", other)),
        }
        let plain = !self.entry.is_empty()
            && !self.entry.starts_with('.')
            && !self.entry.contains(['/', '\\'])
            && printable(&self.entry);
        if !plain {
            return Err("entry: a file name in the plugin's folder".to_string());
        }
        if self.events.is_empty() {
            return Err("events: list at least one event, or \"*\" for all".to_string());
        }
        for event in &self.events {
            if event == ALL_EVENTS {
                continue;
            }
            match event.parse::<EventType>() {
                Err(_) => return Err(format!("events: unknown event type \"{}\"", event)),
                Ok(t) if t.is_test() => {
                    return Err(format!("events: {} is only sent by a test button", t))
                }
                Ok(_) => {}
            }
        }
        for host in &self.permissions.http.hosts {
            if !valid_host(host) {
                return Err(format!(
                    "permissions.http.hosts: \"{}\" is not a lowercase host name",
                    host
                ));
            }
        }
        if self.config.len() > MAX_FIELDS {
            return Err(format!("config: at most {} fields", MAX_FIELDS));
        }
        let mut keys = HashSet::new();
        for field in &self.config {
            let key = &field.key;
            let key_ok = !key.is_empty()
                && key.len() <= 40
                && key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if !key_ok {
                return Err(format!(
                    "config: key \"{}\" may only use a-z, 0-9 and _ (1 to 40)",
                    key
                ));
            }
            if !keys.insert(key.as_str()) {
                return Err(format!("config.{}: listed twice", key));
            }
            if field.secret && !matches!(field.kind, FieldType::String | FieldType::Url) {
                return Err(format!(
                    "config.{}: only text and URL fields can be secret",
                    key
                ));
            }
            if let Some(default) = &field.default {
                if !type_ok(field.kind, default) {
                    return Err(format!(
                        "config.{}.default: must be {}",
                        key,
                        type_name(field.kind)
                    ));
                }
            }
        }
        for key in &self.permissions.http.host_from_config {
            let field = self.config.iter().find(|f| &f.key == key);
            if field.is_none_or(|f| f.kind != FieldType::Url) {
                return Err(format!(
                    "permissions.http.host_from_config: \"{}\" is not a url config field",
                    key
                ));
            }
        }
        Ok(())
    }

    /// Whether the plugin wants `event_type`. `*` leaves out the test events.
    pub fn wants(&self, event_type: EventType) -> bool {
        !event_type.is_test()
            && self
                .events
                .iter()
                .any(|e| e == ALL_EVENTS || e == event_type.as_str())
    }

    fn field(&self, key: &str) -> Option<&ConfigField> {
        self.config.iter().find(|f| f.key == key)
    }

    /// The value for `key`: a secret, a set value or the default
    fn value(&self, settings: &PluginSettings, key: &str) -> Option<Value> {
        settings
            .secrets
            .get(key)
            .map(|s| Value::String(s.clone()))
            .or_else(|| settings.config.get(key).cloned())
            .or_else(|| self.field(key).and_then(|f| f.default.clone()))
    }

    /// The hosts the plugin would call with these settings: the fixed ones plus the
    /// host of each `host_from_config` field's value. Sorted, no repeats.
    pub fn hosts_needed(&self, settings: &PluginSettings) -> Vec<String> {
        let mut hosts: Vec<String> = self.permissions.http.hosts.clone();
        for key in &self.permissions.http.host_from_config {
            if let Some(host) = self
                .value(settings, key)
                .and_then(|v| v.as_str().and_then(url_host))
            {
                hosts.push(host);
            }
        }
        hosts.sort();
        hosts.dedup();
        hosts
    }

    /// Required fields with no value, set or default
    pub fn missing(&self, settings: &PluginSettings) -> Vec<String> {
        self.config
            .iter()
            .filter(|f| f.required)
            .filter(|f| match self.value(settings, &f.key) {
                None => true,
                Some(Value::String(s)) => s.trim().is_empty(),
                Some(_) => false,
            })
            .map(|f| f.key.clone())
            .collect()
    }

    /// Check the values in `settings` against the fields: known keys, secrets in
    /// `secrets` and nothing else there, and types. Messages name plugin and field and
    /// never hold a secret value.
    pub fn check_values(&self, settings: &PluginSettings) -> Result<(), String> {
        let at = |part: &str, key: &str| format!("plugins \"{}\".{}.{}", self.name, part, key);
        for (key, value) in &settings.config {
            let Some(field) = self.field(key) else {
                return Err(format!(
                    "{}: {} has no such setting",
                    at("config", key),
                    self.name
                ));
            };
            if field.secret {
                return Err(format!(
                    "{}: is secret; set it under secrets",
                    at("config", key)
                ));
            }
            if !type_ok(field.kind, value) {
                return Err(format!(
                    "{}: must be {}",
                    at("config", key),
                    type_name(field.kind)
                ));
            }
        }
        for (key, value) in &settings.secrets {
            let Some(field) = self.field(key) else {
                return Err(format!(
                    "{}: {} has no such setting",
                    at("secrets", key),
                    self.name
                ));
            };
            if !field.secret {
                return Err(format!(
                    "{}: is not secret; set it under config",
                    at("secrets", key)
                ));
            }
            if !type_ok(field.kind, &Value::String(value.clone())) {
                return Err(format!(
                    "{}: must be {}",
                    at("secrets", key),
                    type_name(field.kind)
                ));
            }
        }
        Ok(())
    }

    /// What the plugin sees as `putio.config`: defaults, then set values, then secrets
    pub fn config_values(&self, settings: &PluginSettings) -> serde_json::Map<String, Value> {
        let mut out = serde_json::Map::new();
        for field in &self.config {
            if let Some(v) = self.value(settings, &field.key) {
                out.insert(field.key.clone(), v);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const NTFY: &str = r#"
name = "ntfy"
version = "1.0.0"
description = "Push notifications through ntfy"
events = ["download.finished", "storage.low"]

[permissions.http]
host_from_config = ["server"]

[[config]]
key = "server"
type = "url"
default = "https://ntfy.sh"

[[config]]
key = "topic"
type = "string"
required = true

[[config]]
key = "token"
type = "string"
secret = true

[[config]]
key = "group_minutes"
type = "integer"
default = 5
"#;

    fn parse(text: &str) -> Result<Manifest, String> {
        Manifest::parse(text, "plugins/x/plugin.toml")
    }

    fn with(extra: &str) -> String {
        NTFY.replacen(
            "events = [\"download.finished\", \"storage.low\"]",
            extra,
            1,
        )
    }

    fn err(text: &str) -> String {
        parse(text).unwrap_err()
    }

    #[test]
    fn parses_with_defaults() {
        let m = parse(NTFY).unwrap();
        assert_eq!(m.runtime, "lua");
        assert_eq!(m.entry, "main.lua");
        assert!(m.permissions.http.any() && !m.permissions.store);
        assert!(m.wants(EventType::StorageLow));
        assert!(!m.wants(EventType::SyncCompleted));
    }

    #[test]
    fn every_rule_names_file_and_field() {
        let cases = [
            (
                NTFY.replace("\"ntfy\"", "\"my plugin\""),
                "plugins/x/plugin.toml: name:",
            ),
            (NTFY.replace("\"1.0.0\"", "\"\""), "version: 1 to 32"),
            (
                NTFY.replace("Push notifications through ntfy", &"x".repeat(301)),
                "description: at most 300",
            ),
            (with("events = []"), "events: list at least one"),
            (
                with("events = [\"download.done\"]"),
                "events: unknown event type \"download.done\"",
            ),
            (
                with("events = [\"plugin.test\"]"),
                "only sent by a test button",
            ),
            (
                with("events = [\"hook.test\"]"),
                "only sent by a test button",
            ),
            (
                with("events = [\"*\"]\nruntime = \"wasm\""),
                "runtime: WebAssembly plugins need",
            ),
            (
                with("events = [\"*\"]\nruntime = \"js\""),
                "runtime: unknown",
            ),
            (
                with("events = [\"*\"]\nentry = \"../x.lua\""),
                "entry: a file name",
            ),
            (
                with("events = [\"*\"]\nentry = \"lib/x.lua\""),
                "entry: a file name",
            ),
            (
                with("events = [\"*\"]\ncolour = \"red\""),
                "unknown field `colour`",
            ),
            (
                NTFY.replace("host_from_config = [\"server\"]", "hosts = [\"https://x\"]"),
                "permissions.http.hosts: \"https://x\"",
            ),
            (
                NTFY.replace(
                    "host_from_config = [\"server\"]",
                    "host_from_config = [\"topic\"]",
                ),
                "host_from_config: \"topic\" is not a url",
            ),
            (
                NTFY.replace("key = \"topic\"", "key = \"Topic\""),
                "config: key \"Topic\"",
            ),
            (
                NTFY.replace("key = \"topic\"", "key = \"server\""),
                "config.server: listed twice",
            ),
            (
                NTFY.replace("default = 5", "default = \"five\""),
                "config.group_minutes.default: must be a whole number",
            ),
            (
                NTFY.replace(
                    "type = \"integer\"\ndefault = 5",
                    "type = \"integer\"\nsecret = true",
                ),
                "config.group_minutes: only text and URL fields can be secret",
            ),
            (
                NTFY.replace(
                    "default = \"https://ntfy.sh\"",
                    "default = \"ftp://ntfy.sh\"",
                ),
                "config.server.default: must be an http or https URL",
            ),
            (
                NTFY.replace("type = \"integer\"", "type = \"date\""),
                "unknown variant `date`",
            ),
        ];
        for (text, want) in cases {
            let e = err(&text);
            assert!(e.starts_with("plugins/x/plugin.toml: "), "{}", e);
            assert!(e.contains(want), "{} does not contain {}", e, want);
        }
    }

    fn settings() -> PluginSettings {
        PluginSettings {
            name: "ntfy".to_string(),
            config: BTreeMap::from([("topic".to_string(), serde_json::json!("nas"))]),
            secrets: BTreeMap::from([("token".to_string(), "tk_secret".to_string())]),
            ..Default::default()
        }
    }

    #[test]
    fn hosts_come_from_fixed_list_and_url_values() {
        let m = parse(NTFY).unwrap();
        assert_eq!(m.hosts_needed(&settings()), vec!["ntfy.sh"]);
        let mut s = settings();
        s.config.insert(
            "server".to_string(),
            serde_json::json!("https://Push.Example.com:8443/x"),
        );
        assert_eq!(m.hosts_needed(&s), vec!["push.example.com"]);
        let fixed = parse(&NTFY.replace(
            "host_from_config = [\"server\"]",
            "hosts = [\"b.example\", \"a.example\"]\nhost_from_config = [\"server\"]",
        ))
        .unwrap();
        assert_eq!(
            fixed.hosts_needed(&settings()),
            vec!["a.example", "b.example", "ntfy.sh"]
        );
    }

    #[test]
    fn values_checked_against_fields() {
        let m = parse(NTFY).unwrap();
        m.check_values(&settings()).unwrap();
        let check = |f: &dyn Fn(&mut PluginSettings)| {
            let mut s = settings();
            f(&mut s);
            m.check_values(&s).unwrap_err()
        };
        assert!(check(&|s| {
            s.config.insert("colour".into(), serde_json::json!("red"));
        })
        .contains("plugins \"ntfy\".config.colour: ntfy has no such setting"));
        assert!(check(&|s| {
            s.config
                .insert("group_minutes".into(), serde_json::json!("5"));
        })
        .contains("config.group_minutes: must be a whole number"));
        assert!(check(&|s| {
            s.config
                .insert("server".into(), serde_json::json!("file:///etc/passwd"));
        })
        .contains("config.server: must be an http or https URL"));
        let e = check(&|s| {
            s.secrets.remove("token");
            s.config
                .insert("token".into(), serde_json::json!("tk_secret"));
        });
        assert!(e.contains("config.token: is secret"), "{}", e);
        assert!(!e.contains("tk_secret"));
        assert!(check(&|s| {
            s.secrets.insert("topic".into(), "x".into());
        })
        .contains("secrets.topic: is not secret"));
    }

    #[test]
    fn missing_and_values() {
        let m = parse(NTFY).unwrap();
        assert!(m.missing(&settings()).is_empty());
        let mut s = settings();
        s.config.insert("topic".into(), serde_json::json!(" "));
        assert_eq!(m.missing(&s), vec!["topic"]);
        s.config.remove("topic");
        assert_eq!(m.missing(&s), vec!["topic"]);
        let values = m.config_values(&settings());
        assert_eq!(values["server"], "https://ntfy.sh");
        assert_eq!(values["group_minutes"], 5);
        assert_eq!(values["token"], "tk_secret");
        assert_eq!(values["topic"], "nas");
    }
}
