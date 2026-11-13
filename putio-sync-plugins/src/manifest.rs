//! `plugin.toml`: what a plugin is, which events it wants, what it may do and which
//! settings it takes (contracts/manifest.md). WebAssembly plugins (029) use the same file
//! with `runtime = "wasm"`, and may ask for folders under `[permissions.files]`.

use std::collections::HashSet;

use putio_sync_core::events::EventType;
use putio_sync_core::hooks::ALL_EVENTS;
use putio_sync_core::plugins::{valid_host, valid_link, valid_name, PluginSettings};
use putio_sync_core::sync_hooks::Hook;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Most config fields in one manifest
pub const MAX_FIELDS: usize = 32;

/// Most `[[actions]]` in one manifest
pub const MAX_ACTIONS: usize = 8;

/// Seconds between ticks when `tick_seconds` is not set
pub const DEFAULT_TICK_SECONDS: u64 = 60;

fn default_runtime() -> String {
    "lua".to_string()
}

/// The entry when the manifest names none, by runtime
pub fn default_entry(runtime: &str) -> &'static str {
    if runtime == "wasm" {
        "plugin.wasm"
    } else {
        "main.lua"
    }
}

/// `{download_dir}` in a `[permissions.files]` folder
pub const DOWNLOAD_DIR: &str = "{download_dir}";

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
    /// `lua` or `wasm` (029)
    #[serde(default = "default_runtime")]
    pub runtime: String,
    /// The script or module, a plain file name in the plugin's folder; `main.lua` or
    /// `plugin.wasm` when not set
    #[serde(default)]
    pub entry: String,
    /// Event types, or `*` for all; may be empty when `hooks` is not
    #[serde(default)]
    pub events: Vec<String>,
    /// Sync extension points it answers: `filter`, `rename`, `folder` (028)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<String>,
    /// What the plugin may do
    #[serde(default)]
    pub permissions: Permissions,
    /// Settings the plugin takes
    #[serde(default)]
    pub config: Vec<ConfigField>,
    /// Buttons in Settings that call `plugin.on_action` (031)
    #[serde(default)]
    pub actions: Vec<Action>,
    /// Seconds between `on_tick` calls, 5 to 3600 (031); 60 when not set
    #[serde(default)]
    pub tick_seconds: Option<u64>,
    /// Who made the plugin, up to 100 characters (059)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    /// An SPDX license identifier, such as `MIT`
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
    /// The plugin's website
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub homepage: String,
    /// Where to read how to set it up
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub docs: String,
    /// Its source code
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repository: String,
    /// Where to report a problem
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub issues: String,
}

/// One `[[actions]]` entry (031)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    /// `[a-z0-9_]{1,40}`, passed to `on_action`
    pub id: String,
    /// The button's text, 1 to 40 characters
    pub label: String,
    /// One line of help in Settings
    #[serde(default)]
    pub help: String,
    /// No button: a step reached through `next` or a field's `choices` only
    #[serde(default)]
    pub hidden: bool,
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
    /// `[permissions.files]` (029)
    #[serde(default, skip_serializing_if = "FilesPermission::is_empty")]
    pub files: FilesPermission,
}

/// `[permissions.files]`: folders a plugin may read or write (029). A folder is absolute,
/// or `{download_dir}` and below.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesPermission {
    /// Folders to read
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read: Vec<String>,
    /// Folders to read and write
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub write: Vec<String>,
    /// Keys of string fields holding a folder to read
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_from_config: Vec<String>,
    /// Keys of string fields holding a folder to write
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub write_from_config: Vec<String>,
}

impl FilesPermission {
    /// Whether the plugin asks for no folder
    pub fn is_empty(&self) -> bool {
        self.read.is_empty()
            && self.write.is_empty()
            && self.read_from_config.is_empty()
            && self.write_from_config.is_empty()
    }
}

/// Whether `folder` is one `[permissions.files]` takes: absolute, or `{download_dir}` and
/// below, with no `..` part
pub fn valid_folder(folder: &str) -> bool {
    let rest = match folder.strip_prefix(DOWNLOAD_DIR) {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => rest,
        Some(_) => return false,
        None if std::path::Path::new(folder).is_absolute() => folder,
        None => return false,
    };
    !folder.chars().any(char::is_control) && !rest.split(['/', '\\']).any(|part| part == "..")
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
    /// A list of `{from, to}` path rules (031)
    PathMap,
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
    /// An action whose `choices` Settings offers for this field (031)
    #[serde(default)]
    pub choices: Option<String>,
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
        FieldType::PathMap => crate::paths::rules(value).is_ok(),
    }
}

/// Why `value` is not of the field's type
fn type_error(kind: FieldType, value: &Value) -> Option<String> {
    if type_ok(kind, value) {
        return None;
    }
    Some(match (kind, crate::paths::rules(value)) {
        (FieldType::PathMap, Err(e)) => e,
        _ => format!("must be {}", type_name(kind)),
    })
}

fn type_name(kind: FieldType) -> &'static str {
    match kind {
        FieldType::String => "text",
        FieldType::Integer => "a whole number",
        FieldType::Number => "a number",
        FieldType::Boolean => "true or false",
        FieldType::Url => "an http or https URL",
        FieldType::StringList => "a list of text",
        FieldType::PathMap => "a list of {from, to} path rules",
    }
}

/// `[a-z0-9_]{1,40}`: config keys and action ids
fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 40
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
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
        let mut manifest: Manifest = toml::from_str(text).map_err(|e| {
            let msg = e.message().to_string();
            format!("{}: {}", file, msg)
        })?;
        if manifest.entry.is_empty() {
            manifest.entry = default_entry(&manifest.runtime).to_string();
        }
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
        if self.author.chars().count() > 100 || !printable(&self.author) {
            return Err("author: at most 100 printable characters".to_string());
        }
        let spdx = |c: char| c.is_ascii_alphanumeric() || " .+()-".contains(c);
        if !self.license.is_empty() && (self.license.len() > 60 || !self.license.chars().all(spdx))
        {
            return Err("license: an SPDX identifier, such as MIT or Apache-2.0".to_string());
        }
        for (field, url) in [
            ("homepage", &self.homepage),
            ("docs", &self.docs),
            ("repository", &self.repository),
            ("issues", &self.issues),
        ] {
            if !url.is_empty() {
                valid_link(url).map_err(|e| format!("{}: {}", field, e))?;
            }
        }
        match self.runtime.as_str() {
            "lua" | "wasm" => {}
            other => return Err(format!("runtime: unknown runtime \"{}\"", other)),
        }
        let plain = !self.entry.is_empty()
            && !self.entry.starts_with('.')
            && !self.entry.contains(['/', '\\'])
            && printable(&self.entry);
        if !plain {
            return Err("entry: a file name in the plugin's folder".to_string());
        }
        let mut hooks = HashSet::new();
        for hook in &self.hooks {
            let parsed: Hook = hook.parse().map_err(|e| format!("hooks: {}", e))?;
            if !hooks.insert(parsed) {
                return Err(format!("hooks: \"{}\" listed twice", hook));
            }
        }
        if self.events.is_empty() && self.hooks.is_empty() {
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
        if let Some(t) = self.tick_seconds {
            if !(5..=3600).contains(&t) {
                return Err("tick_seconds: 5 to 3600".to_string());
            }
        }
        if self.actions.len() > MAX_ACTIONS {
            return Err(format!("actions: at most {}", MAX_ACTIONS));
        }
        let mut ids = HashSet::new();
        for action in &self.actions {
            if !valid_key(&action.id) {
                return Err(format!(
                    "actions: id \"{}\" may only use a-z, 0-9 and _ (1 to 40)",
                    action.id
                ));
            }
            if !ids.insert(action.id.as_str()) {
                return Err(format!("actions.{}: listed twice", action.id));
            }
            let n = action.label.trim().chars().count();
            if n == 0 || n > 40 {
                return Err(format!("actions.{}.label: 1 to 40 characters", action.id));
            }
        }
        let mut keys = HashSet::new();
        for field in &self.config {
            let key = &field.key;
            if !valid_key(key) {
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
                if let Some(e) = type_error(field.kind, default) {
                    return Err(format!("config.{}.default: {}", key, e));
                }
            }
            if let Some(action) = &field.choices {
                if !matches!(
                    field.kind,
                    FieldType::String | FieldType::Url | FieldType::StringList
                ) {
                    return Err(format!(
                        "config.{}.choices: only for text, URL and list fields",
                        key
                    ));
                }
                if !ids.contains(action.as_str()) {
                    return Err(format!(
                        "config.{}.choices: \"{}\" is not one of the actions",
                        key, action
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
        let files = &self.permissions.files;
        for (part, folders) in [("read", &files.read), ("write", &files.write)] {
            if let Some(bad) = folders.iter().find(|f| !valid_folder(f)) {
                return Err(format!(
                    "permissions.files.{}: \"{}\" must be an absolute folder or start with {} (no ..)",
                    part, bad, DOWNLOAD_DIR
                ));
            }
        }
        for (part, keys) in [
            ("read_from_config", &files.read_from_config),
            ("write_from_config", &files.write_from_config),
        ] {
            for key in keys {
                let field = self.config.iter().find(|f| &f.key == key);
                if field.is_none_or(|f| f.kind != FieldType::String || f.secret) {
                    return Err(format!(
                        "permissions.files.{}: \"{}\" is not a string config field",
                        part, key
                    ));
                }
            }
        }
        Ok(())
    }

    /// Seconds between `on_tick` calls
    pub fn tick_every(&self) -> u64 {
        self.tick_seconds.unwrap_or(DEFAULT_TICK_SECONDS)
    }

    /// Whether the plugin answers the sync extension point `hook`
    pub fn has_hook(&self, hook: Hook) -> bool {
        self.hooks.iter().any(|h| h == hook.as_str())
    }

    /// Whether `id` is one of the actions
    pub fn has_action(&self, id: &str) -> bool {
        self.actions.iter().any(|a| a.id == id)
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

    /// The folders the plugin would use with these settings, as `r:<folder>` and
    /// `w:<folder>`: the fixed ones plus each `*_from_config` value that is a valid folder.
    /// `{download_dir}` stays as written. Sorted, no repeats.
    pub fn paths_needed(&self, settings: &PluginSettings) -> Vec<String> {
        let files = &self.permissions.files;
        let mut paths = Vec::new();
        for (mode, fixed, keys) in [
            ("r", &files.read, &files.read_from_config),
            ("w", &files.write, &files.write_from_config),
        ] {
            let from_config = keys.iter().filter_map(|key| {
                self.value(settings, key)
                    .and_then(|v| v.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| valid_folder(s))
            });
            for folder in fixed.iter().cloned().chain(from_config) {
                paths.push(format!("{}:{}", mode, folder));
            }
        }
        paths.sort();
        paths.dedup();
        paths
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
            if let Some(e) = type_error(field.kind, value) {
                return Err(format!("{}: {}", at("config", key), e));
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
                with("events = [\"*\"]\n[permissions.files]\nread = [\"media\"]"),
                "permissions.files.read: \"media\" must be an absolute folder",
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
            (with("events = [\"*\"]\ntick_seconds = 2"), "tick_seconds: 5 to 3600"),
            (
                format!("{}\n[[actions]]\nid = \"Go\"\nlabel = \"Go\"\n", NTFY),
                "actions: id \"Go\"",
            ),
            (
                format!("{}\n[[actions]]\nid = \"go\"\nlabel = \"\"\n", NTFY),
                "actions.go.label: 1 to 40",
            ),
            (
                format!(
                    "{}\n[[actions]]\nid = \"go\"\nlabel = \"Go\"\n[[actions]]\nid = \"go\"\nlabel = \"Go\"\n",
                    NTFY
                ),
                "actions.go: listed twice",
            ),
            (
                NTFY.replace("type = \"string\"\nrequired = true", "type = \"string\"\nchoices = \"load\""),
                "config.topic.choices: \"load\" is not one of the actions",
            ),
            (
                format!(
                    "{}\n[[config]]\nkey = \"paths\"\ntype = \"path_map\"\ndefault = [{{ from = \"x\", to = \"/y\" }}]\n",
                    NTFY
                ),
                "config.paths.default: rule 1: from must start with /",
            ),
            (
                format!(
                    "{}\n[[config]]\nkey = \"paths\"\ntype = \"path_map\"\nsecret = true\n",
                    NTFY
                ),
                "config.paths: only text and URL fields can be secret",
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
    fn actions_choices_and_path_maps() {
        let text = format!(
            "{}tick_seconds = 5\n{}",
            "",
            NTFY.replace(
                "type = \"string\"\nrequired = true",
                "type = \"string\"\nchoices = \"load\"",
            ) + "\n[[config]]\nkey = \"paths\"\ntype = \"path_map\"\ndefault = []\n\n[[actions]]\nid = \"load\"\nlabel = \"Load\"\nhidden = true\n"
        );
        let m = parse(&text).unwrap();
        assert_eq!(m.tick_every(), 5);
        assert!(m.has_action("load") && !m.has_action("x"));
        assert!(m.actions[0].hidden);
        assert_eq!(parse(NTFY).unwrap().tick_every(), 60);
        let mut s = settings();
        s.config.insert(
            "paths".into(),
            serde_json::json!([{"from": "/downloads", "to": "D:\\Media"}]),
        );
        m.check_values(&s).unwrap();
        s.config.insert(
            "paths".into(),
            serde_json::json!([{"from": "/d", "to": "rel"}]),
        );
        assert!(m
            .check_values(&s)
            .unwrap_err()
            .contains("config.paths: rule 1: to must start with"));
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

    #[test]
    fn sync_hooks() {
        let m = parse(&with("hooks = [\"filter\", \"folder\"]")).unwrap();
        assert!(m.events.is_empty());
        assert!(m.has_hook(Hook::Filter) && m.has_hook(Hook::Folder));
        assert!(!m.has_hook(Hook::Rename));
        let m = parse(&with("events = [\"*\"]\nhooks = [\"rename\"]")).unwrap();
        assert!(m.has_hook(Hook::Rename) && m.wants(EventType::StorageLow));
        assert!(err(&with("hooks = [\"sort\"]"))
            .contains("hooks: unknown sync hook \"sort\" (filter, rename or folder)"));
        assert!(err(&with("hooks = [\"filter\", \"filter\"]"))
            .contains("hooks: \"filter\" listed twice"));
        assert!(err(&with("hooks = []")).contains("events: list at least one event"));
    }

    #[test]
    fn links_and_author() {
        let m = parse(NTFY).unwrap();
        assert!(m.author.is_empty() && m.docs.is_empty());
        let m = parse(&with(
            "events = [\"*\"]\nauthor = \"Ada\"\nlicense = \"MIT OR Apache-2.0\"\nhomepage = \"https://example.com\"\ndocs = \"http://nas.local/docs\"\nrepository = \"https://github.com/ada/p\"\nissues = \"https://github.com/ada/p/issues\"",
        ))
        .unwrap();
        assert_eq!(m.author, "Ada");
        assert_eq!(m.docs, "http://nas.local/docs");
        for field in ["homepage", "docs", "repository", "issues"] {
            let text = with(&format!(
                "events = [\"*\"]\n{} = \"javascript:alert(1)\"",
                field
            ));
            assert_eq!(
                err(&text),
                format!("plugins/x/plugin.toml: {}: must be an https address", field)
            );
        }
        let long = format!("events = [\"*\"]\nauthor = \"{}\"", "a".repeat(101));
        assert!(err(&with(&long)).contains("author: at most 100"));
        assert!(
            err(&with("events = [\"*\"]\nauthor = \"a\\u0007\"")).contains("author: at most 100")
        );
        assert!(err(&with("events = [\"*\"]\nlicense = \"MIT;rm\""))
            .contains("license: an SPDX identifier"));
    }

    #[test]
    #[cfg_attr(windows, ignore = "its folders are Unix absolute paths")]
    fn wasm_runtime_and_folders() {
        let m = parse(&with("events = [\"*\"]\nruntime = \"wasm\"")).unwrap();
        assert_eq!(m.entry, "plugin.wasm");
        let m = parse(&with(
            "events = [\"*\"]\nruntime = \"wasm\"\nentry = \"x.wasm\"",
        ))
        .unwrap();
        assert_eq!(m.entry, "x.wasm");
        let text = format!(
            "{}\n[[config]]\nkey = \"library\"\ntype = \"string\"\n",
            with("events = [\"*\"]\n[permissions.files]\nread = [\"{download_dir}\", \"/media\"]\nwrite = [\"{download_dir}/.cache\"]\nread_from_config = [\"library\"]")
        );
        let m = parse(&text).unwrap();
        let mut s = settings();
        assert_eq!(
            m.paths_needed(&s),
            vec!["r:/media", "r:{download_dir}", "w:{download_dir}/.cache"]
        );
        s.config
            .insert("library".into(), serde_json::json!("/srv/library"));
        assert!(m.paths_needed(&s).contains(&"r:/srv/library".to_string()));
        s.config
            .insert("library".into(), serde_json::json!("relative/dir"));
        assert_eq!(m.paths_needed(&s).len(), 3);
        for bad in [
            "media",
            "/a/../b",
            "{download_dir}x",
            "{download_dir}/../etc",
        ] {
            assert!(!valid_folder(bad), "{bad}");
        }
        assert!(err(&with(
            "events = [\"*\"]\n[permissions.files]\nwrite_from_config = [\"token\"]"
        ))
        .contains("write_from_config: \"token\" is not a string config field"));
    }
}
