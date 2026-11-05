//! Finding plugins: the bundled ones built into the binary, then one per folder under
//! the plugins folder. A folder plugin replaces a bundled one of the same name.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use putio_sync_core::plugins::{valid_name, MAX_PLUGINS};

use crate::manifest::Manifest;

/// Largest entry script read, in bytes
pub const MAX_SCRIPT_BYTES: u64 = 1024 * 1024;

/// Bundled plugins: folder name, `plugin.toml`, entry script. The notifiers share
/// `bundled/notifier.lua`, put in front of each script.
const BUNDLED: &[(&str, &str, &str)] = &[
    (
        "discord",
        include_str!("../bundled/discord/plugin.toml"),
        concat!(
            include_str!("../bundled/notifier.lua"),
            include_str!("../bundled/discord/main.lua")
        ),
    ),
    (
        "emby",
        include_str!("../bundled/emby/plugin.toml"),
        concat!(
            include_str!("../bundled/mediaserver.lua"),
            include_str!("../bundled/embyfin.lua"),
            include_str!("../bundled/emby/main.lua")
        ),
    ),
    (
        "jellyfin",
        include_str!("../bundled/jellyfin/plugin.toml"),
        concat!(
            include_str!("../bundled/mediaserver.lua"),
            include_str!("../bundled/embyfin.lua"),
            include_str!("../bundled/jellyfin/main.lua")
        ),
    ),
    (
        "ntfy",
        include_str!("../bundled/ntfy/plugin.toml"),
        concat!(
            include_str!("../bundled/notifier.lua"),
            include_str!("../bundled/ntfy/main.lua")
        ),
    ),
    (
        "plex",
        include_str!("../bundled/plex/plugin.toml"),
        concat!(
            include_str!("../bundled/mediaserver.lua"),
            include_str!("../bundled/plex/main.lua")
        ),
    ),
    (
        "pushover",
        include_str!("../bundled/pushover/plugin.toml"),
        concat!(
            include_str!("../bundled/notifier.lua"),
            include_str!("../bundled/pushover/main.lua")
        ),
    ),
    (
        "telegram",
        include_str!("../bundled/telegram/plugin.toml"),
        concat!(
            include_str!("../bundled/notifier.lua"),
            include_str!("../bundled/telegram/main.lua")
        ),
    ),
];

/// Where a plugin came from
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Built into putio-sync
    Bundled,
    /// A folder under the plugins folder
    Folder(PathBuf),
}

impl Source {
    /// `bundled` or `folder`
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Bundled => "bundled",
            Source::Folder(_) => "folder",
        }
    }

    /// The folder, for folder plugins
    pub fn path(&self) -> Option<&Path> {
        match self {
            Source::Bundled => None,
            Source::Folder(p) => Some(p),
        }
    }
}

/// One plugin found, loadable or not
#[derive(Debug, Clone)]
pub struct Found {
    /// The manifest's name, or the folder's name when the manifest is broken
    pub name: String,
    /// Bundled or a folder
    pub source: Source,
    /// The manifest, when it parsed
    pub manifest: Option<Manifest>,
    /// The entry script's text
    pub script: String,
    /// Why it cannot load
    pub error: Option<String>,
}

impl Found {
    fn broken(name: String, source: Source, error: String) -> Found {
        Found {
            name,
            source,
            manifest: None,
            script: String::new(),
            error: Some(error),
        }
    }

    /// The manifest and script, when the plugin can load
    pub fn loadable(&self) -> Option<&Manifest> {
        if self.error.is_some() {
            return None;
        }
        self.manifest.as_ref()
    }
}

fn bundled() -> Vec<Found> {
    BUNDLED
        .iter()
        .map(|(folder, toml, script)| {
            let file = format!("bundled/{}/plugin.toml", folder);
            match Manifest::parse(toml, &file) {
                Ok(m) => Found {
                    name: m.name.clone(),
                    source: Source::Bundled,
                    manifest: Some(m),
                    script: script.to_string(),
                    error: None,
                },
                Err(e) => Found::broken(folder.to_string(), Source::Bundled, e),
            }
        })
        .collect()
}

fn from_folder(path: &Path) -> Found {
    let folder = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let source = Source::Folder(path.to_path_buf());
    let file = path.join("plugin.toml");
    let label = file.display().to_string();
    let text = match std::fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Found::broken(folder, source, format!("{}: not found", label))
        }
        Err(e) => return Found::broken(folder, source, format!("{}: {}", label, e)),
    };
    let manifest = match Manifest::parse(&text, &label) {
        Ok(m) => m,
        Err(e) => return Found::broken(folder, source, e),
    };
    let entry = path.join(&manifest.entry);
    let entry_label = entry.display().to_string();
    let script = match std::fs::metadata(&entry) {
        Ok(meta) if meta.len() > MAX_SCRIPT_BYTES => Err(format!(
            "{}: larger than {} KiB",
            entry_label,
            MAX_SCRIPT_BYTES / 1024
        )),
        Ok(_) => std::fs::read_to_string(&entry).map_err(|e| format!("{}: {}", entry_label, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(format!("{}: not found", entry_label))
        }
        Err(e) => Err(format!("{}: {}", entry_label, e)),
    };
    match script {
        Ok(script) => Found {
            name: manifest.name.clone(),
            source,
            manifest: Some(manifest),
            script,
            error: None,
        },
        Err(e) => Found {
            name: manifest.name.clone(),
            source,
            manifest: Some(manifest),
            script: String::new(),
            error: Some(e),
        },
    }
}

/// Every plugin, sorted by name. A missing folder is no error; an unreadable one is
/// logged. Two folders with one name are both broken.
pub fn discover(dir: Option<&Path>) -> Vec<Found> {
    let mut by_name: BTreeMap<String, Vec<Found>> = BTreeMap::new();
    for found in bundled() {
        by_name.entry(found.name.clone()).or_default().push(found);
    }
    let mut folders = Vec::new();
    if let Some(dir) = dir {
        match std::fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let hidden = entry.file_name().to_string_lossy().starts_with('.');
                    if !hidden && path.is_dir() {
                        folders.push(path);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(dir = %dir.display(), error = %e, "Cannot read the plugins folder")
            }
        }
    }
    folders.sort();
    for path in folders {
        let found = from_folder(&path);
        let list = by_name.entry(found.name.clone()).or_default();
        if list.len() == 1 && list[0].source == Source::Bundled {
            tracing::info!(
                plugin = %found.name,
                folder = %path.display(),
                "A plugin folder replaces the bundled plugin of the same name"
            );
            list.clear();
        }
        list.push(found);
    }
    let mut out = Vec::new();
    for (name, list) in by_name {
        if list.len() == 1 {
            out.extend(list);
            continue;
        }
        let paths: Vec<String> = list
            .iter()
            .filter_map(|f| f.source.path().map(|p| p.display().to_string()))
            .collect();
        for mut found in list {
            found.error = Some(format!(
                "name \"{}\" is used by more than one folder: {}",
                name,
                paths.join(", ")
            ));
            out.push(found);
        }
    }
    let mut n = 0;
    for found in out.iter_mut() {
        if !valid_name(&found.name) {
            continue;
        }
        n += 1;
        if n > MAX_PLUGINS && found.error.is_none() {
            found.error = Some(format!(
                "more than {} plugins; this one is not loaded",
                MAX_PLUGINS
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "name = \"NAME\"\nversion = \"1\"\nevents = [\"*\"]\n";

    fn plugin(dir: &Path, folder: &str, name: &str) -> PathBuf {
        let path = dir.join(folder);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("plugin.toml"), MANIFEST.replace("NAME", name)).unwrap();
        std::fs::write(path.join("main.lua"), "return {on_event = function() end}").unwrap();
        path
    }

    #[test]
    fn a_missing_folder_is_no_error() {
        let dir = tempfile::tempdir().unwrap();
        let found = discover(Some(&dir.path().join("nope")));
        assert!(found.iter().all(|f| f.source == Source::Bundled));
        assert_eq!(discover(None).len(), BUNDLED.len());
    }

    #[test]
    fn folders_load_sorted_and_broken_ones_say_why() {
        let dir = tempfile::tempdir().unwrap();
        plugin(dir.path(), "zeta", "zeta");
        plugin(dir.path(), "alpha", "alpha");
        let no_script = plugin(dir.path(), "noscript", "noscript");
        std::fs::remove_file(no_script.join("main.lua")).unwrap();
        std::fs::create_dir_all(dir.path().join("empty")).unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let bad = plugin(dir.path(), "bad", "bad");
        std::fs::write(bad.join("plugin.toml"), "name = \"bad\"\nversion = \"1\"\n").unwrap();
        let found: Vec<Found> = discover(Some(dir.path()))
            .into_iter()
            .filter(|f| f.source != Source::Bundled)
            .collect();
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "bad", "empty", "noscript", "zeta"]);
        assert!(found[0].loadable().is_some());
        assert!(found[0].script.contains("on_event"));
        assert!(found[1]
            .error
            .as_deref()
            .unwrap()
            .contains("plugin.toml: events: list at least one"));
        assert!(found[2]
            .error
            .as_deref()
            .unwrap()
            .ends_with("plugin.toml: not found"));
        assert!(found[3]
            .error
            .as_deref()
            .unwrap()
            .ends_with("main.lua: not found"));
        assert!(found[3].manifest.is_some() && found[3].loadable().is_none());
    }

    #[test]
    fn two_folders_with_one_name_are_both_broken() {
        let dir = tempfile::tempdir().unwrap();
        plugin(dir.path(), "one", "same");
        plugin(dir.path(), "two", "same");
        let found: Vec<Found> = discover(Some(dir.path()))
            .into_iter()
            .filter(|f| f.name == "same")
            .collect();
        assert_eq!(found.len(), 2);
        for f in found {
            assert!(f.error.unwrap().contains("used by more than one folder"));
        }
    }

    #[test]
    fn more_than_the_limit_are_not_loaded() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..MAX_PLUGINS + 1 {
            plugin(dir.path(), &format!("p{:02}", i), &format!("p{:02}", i));
        }
        let found = discover(Some(dir.path()));
        let refused = found
            .iter()
            .filter(|f| {
                f.error
                    .as_deref()
                    .is_some_and(|e| e.contains("more than 32"))
            })
            .count();
        assert_eq!(refused, 1 + BUNDLED.len());
    }
}
