//! The policy for one folder: `[precheck]`, the folder's 009 rule, then its override
//! (research R9)

use std::collections::BTreeSet;
use std::path::Path;

use putio_sync_core::config::PrecheckConfig;
use putio_sync_core::rules::{self, RuleSet};

use crate::rule::Policy;

const MB: u64 = 1024 * 1024;

/// Which `[precheck]` override applies
#[derive(Debug, Clone, Copy)]
pub enum Key<'a> {
    /// An *arr category
    Category(&'a str),
    /// A `[watch].paths` entry
    Watch(&'a Path),
}

impl Policy {
    /// The policy for releases that land in `putio_folder` (a path from the Put.io root,
    /// as 009 rules name folders)
    pub fn for_folder(
        cfg: &PrecheckConfig,
        rules: &RuleSet,
        putio_folder: &str,
        key: Key<'_>,
    ) -> Policy {
        let effective = rules.resolve(putio_folder);
        let filter = &effective.filter;
        let (wanted, wanted_from, kind) = match &filter.value.only {
            Some(only) => {
                let wanted: BTreeSet<String> =
                    only.difference(&filter.value.blocked).cloned().collect();
                let kind = effective
                    .preset
                    .value
                    .as_deref()
                    .and_then(preset_kind)
                    .map_or_else(|| types_kind(&wanted), str::to_string);
                (wanted, filter.from.clone(), kind)
            }
            None => (video(), None, "video".to_string()),
        };

        let override_ = match key {
            Key::Category(category) => cfg.categories.get(category),
            Key::Watch(path) => cfg
                .watch
                .iter()
                .find(|(k, _)| Path::new(k) == path)
                .map(|(_, v)| v),
        };
        let enabled = override_.and_then(|o| o.enabled).unwrap_or(cfg.enabled);
        let min_size_mb = override_
            .and_then(|o| o.min_size_mb)
            .unwrap_or(cfg.min_size_mb);
        let allow_archives = override_
            .and_then(|o| o.allow_archives)
            .unwrap_or(cfg.allow_archives);

        Policy {
            enabled,
            wanted,
            wanted_from,
            min_size: min_size_mb * MB,
            allow_archives,
            kind,
        }
    }
}

/// The built-in video allow list: the default when no rule has an allow list
fn video() -> BTreeSet<String> {
    rules::builtin("video")
        .and_then(|p| p.only)
        .unwrap_or_default()
        .iter()
        .map(|e| e.to_string())
        .collect()
}

/// The word for a built-in allow-list preset
fn preset_kind(preset: &str) -> Option<&'static str> {
    match preset {
        "video" => Some("video"),
        "audio" => Some("audio"),
        "ebooks" => Some("ebook"),
        _ => None,
    }
}

/// A custom allow list names its first types: "flac/mp3"
fn types_kind(wanted: &BTreeSet<String>) -> String {
    let first: Vec<&str> = wanted.iter().take(3).map(String::as_str).collect();
    if first.is_empty() {
        "wanted".to_string()
    } else {
        first.join("/")
    }
}
