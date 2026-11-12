//! The registry's `index.json` (059): reviewed plugins, each with its releases, as
//! `scripts/registry.py build` writes it in devclinton/putio-sync-plugin-registry. Entries
//! are checked again on read; a bad entry is dropped, a bad index is refused.

use std::collections::HashSet;

use putio_sync_core::plugins::{valid_link, valid_name};
use serde::{Deserialize, Serialize};

use crate::discover::bundled_source;

/// The index format this putio-sync reads
pub const SCHEMA_VERSION: u32 = 1;

/// The largest index read
pub const MAX_INDEX_BYTES: usize = 2 * 1024 * 1024;

/// The whole index
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub schema_version: u32,
    #[serde(default)]
    pub plugins: Vec<Entry>,
}

/// One reviewed plugin
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub description: String,
    pub author: String,
    pub repository: String,
    pub license: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issues: Option<String>,
    /// `lua` or `wasm`
    #[serde(default = "lua")]
    pub runtime: String,
    pub events: Vec<String>,
    /// Sync rules it answers (028)
    #[serde(default)]
    pub hooks: Vec<String>,
    #[serde(default)]
    pub hosts: Hosts,
    pub releases: Vec<Release>,
    /// Why it is no longer offered
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrawn: Option<String>,
}

fn lua() -> String {
    "lua".to_string()
}

/// The hosts the manifest's `[permissions.http]` names
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Hosts {
    #[serde(default)]
    pub fixed: Vec<String>,
    #[serde(default)]
    pub from_config: Vec<String>,
}

/// One pinned release
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    /// A `.tar.gz`/`.tgz` archive, or a `.wasm` module that carries its manifest
    pub url: String,
    /// Of the file at `url`, lowercase hex
    pub sha256: String,
    pub min_putio_sync: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// What an entry offers this putio-sync
#[derive(Debug, Clone, PartialEq)]
pub enum Offer<'a> {
    /// The newest release it can run
    Release(&'a Release),
    /// Every release needs at least this version
    NeedsNewer(String),
}

impl Index {
    /// Parse and check `bytes`. Returns the index with bad entries dropped, and how many were.
    pub fn parse(bytes: &[u8]) -> Result<(Index, usize), String> {
        if bytes.len() > MAX_INDEX_BYTES {
            return Err("the registry index is larger than 2 MB".to_string());
        }
        #[derive(Deserialize)]
        struct Head {
            schema_version: u32,
        }
        let head: Head = serde_json::from_slice(bytes)
            .map_err(|e| format!("the registry index is not valid: {}", e))?;
        if head.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "the registry needs a newer putio-sync (index format {})",
                head.schema_version
            ));
        }
        let raw: RawIndex = serde_json::from_slice(bytes)
            .map_err(|e| format!("the registry index is not valid: {}", e))?;
        let mut names = HashSet::new();
        for value in &raw.plugins {
            if let Some(name) = value.get("name").and_then(|n| n.as_str()) {
                if !names.insert(name.to_ascii_lowercase()) {
                    return Err(format!("the registry index lists {} twice", name));
                }
            }
        }
        let mut skipped = 0;
        let mut plugins = Vec::new();
        for value in raw.plugins {
            match serde_json::from_value::<Entry>(value) {
                Ok(entry) if entry.check().is_ok() => plugins.push(entry),
                _ => skipped += 1,
            }
        }
        Ok((
            Index {
                schema_version: SCHEMA_VERSION,
                plugins,
            },
            skipped,
        ))
    }

    /// The entry named `name`
    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.plugins.iter().find(|e| e.name == name)
    }
}

#[derive(Deserialize)]
struct RawIndex {
    #[serde(default)]
    plugins: Vec<serde_json::Value>,
}

impl Entry {
    /// The entry rules of the registry's `check`
    pub fn check(&self) -> Result<(), String> {
        if !valid_name(&self.name) || bundled_source(&self.name).is_some() {
            return Err("name".into());
        }
        let len = |s: &str| s.chars().count();
        if !(1..=300).contains(&len(&self.description)) || !(1..=100).contains(&len(&self.author)) {
            return Err("description or author".into());
        }
        if self.license.is_empty() || self.license.len() > 60 {
            return Err("license".into());
        }
        valid_link(&self.repository)?;
        for link in [&self.docs, &self.homepage, &self.issues]
            .into_iter()
            .flatten()
        {
            valid_link(link)?;
        }
        if !matches!(self.runtime.as_str(), "lua" | "wasm") {
            return Err("runtime".into());
        }
        if self.events.is_empty() && self.hooks.is_empty() {
            return Err("events".into());
        }
        if self.releases.is_empty() {
            return Err("releases".into());
        }
        for r in &self.releases {
            valid_link(&r.url)?;
            let kind =
                r.url.ends_with(".tar.gz") || r.url.ends_with(".tgz") || r.url.ends_with(".wasm");
            let hex = r.sha256.len() == 64
                && r.sha256
                    .chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
            if !kind
                || !hex
                || version(&r.version).is_none()
                || version(&r.min_putio_sync).is_none()
            {
                return Err("release".into());
            }
            if let Some(notes) = &r.notes {
                valid_link(notes)?;
            }
        }
        if self.withdrawn.as_ref().is_some_and(|w| len(w) > 300) {
            return Err("withdrawn".into());
        }
        Ok(())
    }

    /// The newest release whose `min_putio_sync` is at most `running`
    pub fn offered(&self, running: &str) -> Offer<'_> {
        let running = version(running).unwrap_or((0, 0, 0));
        let mut usable: Vec<&Release> = self
            .releases
            .iter()
            .filter(|r| version(&r.min_putio_sync).is_some_and(|min| min <= running))
            .collect();
        usable.sort_by_key(|r| std::cmp::Reverse(version(&r.version)));
        match usable.first() {
            Some(r) => Offer::Release(r),
            None => Offer::NeedsNewer(
                self.releases
                    .iter()
                    .min_by_key(|r| version(&r.min_putio_sync))
                    .map(|r| r.min_putio_sync.clone())
                    .unwrap_or_default(),
            ),
        }
    }
}

/// `1.2.3` (with an optional `-pre` or `+build` suffix, ignored) as numbers
pub fn version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn entry_json(name: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "description": "POST a message to a webhook",
            "author": "ada",
            "repository": "https://github.com/ada/hook",
            "license": "MIT",
            "events": ["download.finished"],
            "hosts": {"from_config": ["url"]},
            "releases": [
                {"version": "1.1.0", "url": "https://x.org/hook-1.1.0.tar.gz",
                 "sha256": "a".repeat(64), "min_putio_sync": "9.0.0"},
                {"version": "1.0.0", "url": "https://x.org/hook-1.0.0.tar.gz",
                 "sha256": "b".repeat(64), "min_putio_sync": "0.2.0"}
            ],
            "future_field": true
        })
    }

    fn index(plugins: Vec<serde_json::Value>) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"schema_version": 1, "plugins": plugins})).unwrap()
    }

    #[test]
    fn parses_and_ignores_unknown_fields() {
        let (index, skipped) = Index::parse(&index(vec![entry_json("hook")])).unwrap();
        assert_eq!(skipped, 0);
        let e = index.get("hook").unwrap();
        assert_eq!(e.runtime, "lua");
        assert_eq!(e.hosts.from_config, vec!["url"]);
    }

    #[test]
    fn refuses_a_newer_format_and_duplicates() {
        let v2 = br#"{"schema_version": 2, "plugins": []}"#;
        assert!(Index::parse(v2)
            .unwrap_err()
            .contains("needs a newer putio-sync"));
        let dup = index(vec![entry_json("hook"), entry_json("HOOK")]);
        assert!(Index::parse(&dup).unwrap_err().contains("twice"));
        assert!(Index::parse(&vec![b' '; MAX_INDEX_BYTES + 1]).is_err());
    }

    #[test]
    fn drops_bad_entries_only() {
        let mut bundled = entry_json("ntfy");
        bundled["name"] = "ntfy".into();
        let mut link = entry_json("badlink");
        link["docs"] = "javascript:alert(1)".into();
        let mut none = entry_json("norelease");
        none["releases"] = serde_json::json!([]);
        let mut zip = entry_json("zipped");
        zip["releases"][0]["url"] = "https://x.org/a.zip".into();
        let (index, skipped) =
            Index::parse(&index(vec![entry_json("hook"), bundled, link, none, zip])).unwrap();
        assert_eq!(skipped, 4);
        assert_eq!(index.plugins.len(), 1);
    }

    #[test]
    fn offers_the_newest_supported_release() {
        let e: Entry = serde_json::from_value(entry_json("hook")).unwrap();
        match e.offered("0.2.2") {
            Offer::Release(r) => assert_eq!(r.version, "1.0.0"),
            other => panic!("{:?}", other),
        }
        match e.offered("9.1.0") {
            Offer::Release(r) => assert_eq!(r.version, "1.1.0"),
            other => panic!("{:?}", other),
        }
        assert_eq!(e.offered("0.1.0"), Offer::NeedsNewer("0.2.0".into()));
        assert_eq!(version("1.2.3-rc.1"), Some((1, 2, 3)));
        assert_eq!(version("1.2"), None);
    }
}
