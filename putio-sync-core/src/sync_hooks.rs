//! Sync extension points (feature 028, contracts/sync-hooks.md): questions the sync asks
//! plugins while it runs, with a short time limit. A **filter** may change whether a file
//! is downloaded, a **rename** rule its local name and a **folder** rule its local folder.
//!
//! Nothing here knows about Lua: the download manager asks a [`SyncHooks`], and the Lua
//! plugins (and later 029's WebAssembly ones) answer it. Answers arrive as JSON and are
//! checked with [`parse_filter`], [`parse_name`] and [`parse_folder`]. An implementation
//! never fails: a plugin that errors, is slow or answers wrong gives `None`, and the
//! built-in rule stands.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;

use serde::Serialize;
use serde_json::Value;

use crate::rules::sanitize_name;

/// One extension point
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Hook {
    /// Whether a file is downloaded, streamed or skipped
    Filter,
    /// The local name of a download
    Rename,
    /// The local folder of a download
    Folder,
}

impl Hook {
    /// Every extension point
    pub const ALL: [Hook; 3] = [Hook::Filter, Hook::Rename, Hook::Folder];

    /// The manifest value, the plugin function and the metric label
    pub fn as_str(&self) -> &'static str {
        match self {
            Hook::Filter => "filter",
            Hook::Rename => "rename",
            Hook::Folder => "folder",
        }
    }
}

impl fmt::Display for Hook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Hook {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Hook::ALL
            .into_iter()
            .find(|h| h.as_str() == s)
            .ok_or_else(|| format!("unknown sync hook \"{}\" (filter, rename or folder)", s))
    }
}

/// What a plugin is told about a file
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookFile {
    /// Put.io file id
    pub id: i64,
    /// Put.io name
    pub name: String,
    /// Bytes
    pub size: u64,
    /// Lowercase, without the dot; empty when none
    pub extension: String,
    /// Put.io folder relative to the synced root, `""` for the root
    pub folder: String,
    /// The built-in decision: `download`, `stream`, `blocked` or `skipped`
    pub decision: String,
    /// The built-in reason in words, empty for a download
    pub reason: String,
}

/// What a filter can decide
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterAnswer {
    /// Download it, whatever the rules said
    Download,
    /// Keep it on Put.io for streaming (feature 022)
    Stream,
    /// Leave it alone
    Skip,
}

/// A boxed answer future, as in the download manager's other seams
pub type HookFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The plugins the sync asks. Each method asks the plugins that provide the hook in
/// name order and returns the first valid answer, or `None` for the built-in rule.
pub trait SyncHooks: Send + Sync {
    /// Whether any running plugin provides `hook`, so the sync can skip the call
    fn provides(&self, hook: Hook) -> bool;
    /// A filter answer and the plugin that gave it
    fn filter<'a>(&'a self, file: &'a HookFile) -> HookFuture<'a, Option<(FilterAnswer, String)>>;
    /// The local file name
    fn rename<'a>(&'a self, file: &'a HookFile) -> HookFuture<'a, Option<String>>;
    /// The local folder, relative to the download folder; `""` for the folder itself
    fn folder<'a>(&'a self, file: &'a HookFile) -> HookFuture<'a, Option<String>>;
}

/// Check a filter answer: `"download"`, `"stream"`, `"skip"`, `true` (download), `false`
/// (skip) or nothing
pub fn parse_filter(value: &Value) -> Result<Option<FilterAnswer>, String> {
    match value {
        Value::Null => Ok(None),
        Value::Bool(true) => Ok(Some(FilterAnswer::Download)),
        Value::Bool(false) => Ok(Some(FilterAnswer::Skip)),
        Value::String(s) => match s.as_str() {
            "download" => Ok(Some(FilterAnswer::Download)),
            "stream" => Ok(Some(FilterAnswer::Stream)),
            "skip" => Ok(Some(FilterAnswer::Skip)),
            other => Err(format!(
                "filter: unknown answer \"{}\" (download, stream, skip or nil)",
                other
            )),
        },
        _ => Err("filter: must return text, true, false or nil".to_string()),
    }
}

/// Longest name a rename may give, in bytes
pub const MAX_NAME_BYTES: usize = 255;

/// Check a rename answer: one plain file name, made safe like Put.io names
pub fn parse_name(value: &Value) -> Result<Option<String>, String> {
    let name = match value {
        Value::Null => return Ok(None),
        Value::String(s) => s.trim(),
        _ => return Err("rename: must return text or nil".to_string()),
    };
    if name.is_empty() || name == "." || name == ".." {
        return Err("rename: not a file name".to_string());
    }
    if name.contains(['/', '\\']) {
        return Err("rename: a name, without / or \\".to_string());
    }
    if name.len() > MAX_NAME_BYTES || name.chars().any(char::is_control) {
        return Err(format!(
            "rename: at most {} bytes, no control characters",
            MAX_NAME_BYTES
        ));
    }
    Ok(Some(sanitize_name(name)))
}

/// Check a folder answer: a relative folder without `..`, each part made safe like
/// Put.io names; `""` is the download folder itself
pub fn parse_folder(value: &Value) -> Result<Option<String>, String> {
    let folder = match value {
        Value::Null => return Ok(None),
        Value::String(s) => s.trim(),
        _ => return Err("folder: must return text or nil".to_string()),
    };
    if folder.starts_with(['/', '\\']) || folder.get(1..2) == Some(":") {
        return Err("folder: must be relative to the download folder".to_string());
    }
    let mut parts = Vec::new();
    for part in folder.split(['/', '\\']).map(str::trim) {
        match part {
            "" | "." => continue,
            ".." => return Err("folder: .. is not allowed".to_string()),
            p if p.len() > MAX_NAME_BYTES || p.chars().any(char::is_control) => {
                return Err(format!(
                    "folder: each part at most {} bytes, no control characters",
                    MAX_NAME_BYTES
                ))
            }
            p => parts.push(sanitize_name(p)),
        }
    }
    Ok(Some(parts.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hook_names_round_trip() {
        for hook in Hook::ALL {
            assert_eq!(hook.as_str().parse::<Hook>(), Ok(hook));
        }
        assert!("filters".parse::<Hook>().unwrap_err().contains("filters"));
    }

    #[test]
    fn filter_answers() {
        assert_eq!(parse_filter(&Value::Null), Ok(None));
        assert_eq!(parse_filter(&json!(true)), Ok(Some(FilterAnswer::Download)));
        assert_eq!(parse_filter(&json!(false)), Ok(Some(FilterAnswer::Skip)));
        assert_eq!(
            parse_filter(&json!("stream")),
            Ok(Some(FilterAnswer::Stream))
        );
        assert_eq!(parse_filter(&json!("skip")), Ok(Some(FilterAnswer::Skip)));
        assert!(parse_filter(&json!("maybe")).is_err());
        assert!(parse_filter(&json!(3)).is_err());
    }

    #[test]
    fn names_are_plain_and_safe() {
        assert_eq!(parse_name(&Value::Null), Ok(None));
        assert_eq!(
            parse_name(&json!(" Show S01E02.mkv ")),
            Ok(Some("Show S01E02.mkv".to_string()))
        );
        assert_eq!(
            parse_name(&json!("a:b?.mkv")),
            Ok(Some("a_b_.mkv".to_string()))
        );
        for bad in ["", " ", ".", "..", "a/b.mkv", "a\\b", "a\nb"] {
            assert!(parse_name(&json!(bad)).is_err(), "{bad:?}");
        }
        assert!(parse_name(&json!("x".repeat(256))).is_err());
        assert!(parse_name(&json!(1)).is_err());
    }

    #[test]
    fn folders_are_relative() {
        assert_eq!(parse_folder(&Value::Null), Ok(None));
        assert_eq!(parse_folder(&json!("")), Ok(Some(String::new())));
        assert_eq!(
            parse_folder(&json!("TV//Show/./Season 01/")),
            Ok(Some("TV/Show/Season 01".to_string()))
        );
        assert_eq!(
            parse_folder(&json!("TV\\Show: A")),
            Ok(Some("TV/Show_ A".to_string()))
        );
        for bad in ["/etc", "\\x", "C:/x", "TV/../..", ".."] {
            assert!(parse_folder(&json!(bad)).is_err(), "{bad:?}");
        }
        assert!(parse_folder(&json!(["a"])).is_err());
    }
}
