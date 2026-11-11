//! The plugin workbench's editor (feature 055): starters, reading a plugin's files,
//! checking them, and writing them back. Only Lua plugins in the plugins folder are
//! written; bundled ones are copied to a folder first, and WebAssembly ones show their
//! manifest only. A save keeps the old file as `<file>.bak` and reloads that plugin alone.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

use putio_sync_core::plugins::valid_name;
use serde::{Deserialize, Serialize};

use crate::discover::{bundled_source, Found, Source, MAX_SCRIPT_BYTES};
use crate::manifest::Manifest;
use crate::sandbox;
use crate::PluginsService;

/// A starter for **New plugin**: id, title, description, `plugin.toml`, `main.lua`
pub const STARTERS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "webhook-notifier",
        "Webhook notifier",
        "Posts events to a webhook as JSON",
        include_str!("../starters/webhook-notifier/plugin.toml"),
        include_str!("../starters/webhook-notifier/main.lua"),
    ),
    (
        "store-counter",
        "Store counter",
        "Counts events in the plugin's store and reports it as health",
        include_str!("../starters/store-counter/plugin.toml"),
        include_str!("../starters/store-counter/main.lua"),
    ),
    (
        "tick-poller",
        "Tick poller",
        "Checks a URL on a timer and logs status changes",
        include_str!("../starters/tick-poller/plugin.toml"),
        include_str!("../starters/tick-poller/main.lua"),
    ),
];

/// The manifest file in a plugin folder
pub const MANIFEST_FILE: &str = "plugin.toml";

/// A starter as the workbench lists it
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Starter {
    /// For **New plugin**
    pub id: &'static str,
    /// Shown in the list
    pub title: &'static str,
    /// One line
    pub description: &'static str,
}

/// The starters
pub fn starters() -> Vec<Starter> {
    STARTERS
        .iter()
        .map(|(id, title, description, _, _)| Starter {
            id,
            title,
            description,
        })
        .collect()
}

/// Something wrong in a plugin's files
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    /// `plugin.toml` or the entry script
    pub file: String,
    /// The line, for a Lua syntax error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// As a reload gives it
    pub message: String,
}

impl Problem {
    /// A manifest problem
    pub fn manifest(message: String) -> Problem {
        Problem {
            file: MANIFEST_FILE.to_string(),
            line: None,
            message,
        }
    }
}

/// One file of a plugin as the editor shows it
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceFile {
    /// Its name in the folder
    pub path: String,
    /// Its text
    pub text: String,
    /// `<length>-<modified ms>`; empty for a bundled file
    pub version: String,
}

/// A plugin's files
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginSource {
    /// The plugin
    pub name: String,
    /// `lua` or `wasm`
    pub runtime: String,
    /// `bundled` or `folder`
    pub source: &'static str,
    /// A Lua plugin in the plugins folder, with the editor on
    pub editable: bool,
    /// `plugin.toml` and, for Lua, the entry script
    pub files: Vec<SourceFile>,
    /// The folder's other file names
    pub other_files: Vec<String>,
}

/// Why the editor could not do it
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// No such plugin or starter
    NotFound,
    /// Not a name a plugin may have
    BadName(String),
    /// A folder or plugin of that name exists
    Taken(String),
    /// The file changed on disk since it was read
    Stale(String),
    /// Bundled, WebAssembly, or not in the plugins folder
    NotEditable(String),
    /// No plugins folder is set
    NoFolder,
    /// Reading or writing failed
    Io(String),
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceError::NotFound => write!(f, "not found"),
            SourceError::BadName(name) => write!(
                f,
                "\"{}\" may only use letters, digits, - and _ (1 to 40)",
                name
            ),
            SourceError::Taken(name) => write!(f, "a plugin or folder named {} exists", name),
            SourceError::Stale(file) => write!(
                f,
                "{} changed on disk since it was opened; open it again or save anyway",
                file
            ),
            SourceError::NotEditable(why) => write!(f, "{}", why),
            SourceError::NoFolder => write!(f, "set a plugins folder first"),
            SourceError::Io(e) => write!(f, "{}", e),
        }
    }
}

/// What **Save** sends
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SaveRequest {
    /// `plugin.toml` text
    pub manifest: String,
    /// Entry script text
    pub script: String,
    /// The versions read at open, by file name
    #[serde(default)]
    pub versions: BTreeMap<String, String>,
    /// Write even when a file changed on disk
    #[serde(default)]
    pub force: bool,
}

/// The line in a Lua error such as `main.lua:12: unexpected symbol`
pub fn error_line(text: &str, file: &str) -> Option<u32> {
    let rest = text.split(&format!("{}:", file)).nth(1)?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Check a manifest and script without running anything: the parsed manifest, when it
/// parsed, and every problem
pub fn check(manifest: &str, script: &str, source: &Source) -> (Option<Manifest>, Vec<Problem>) {
    let label = match source.path() {
        Some(dir) => dir.join(MANIFEST_FILE).display().to_string(),
        None => MANIFEST_FILE.to_string(),
    };
    let mut problems = Vec::new();
    let parsed = match Manifest::parse(manifest, &label) {
        Ok(m) => Some(m),
        Err(e) => {
            problems.push(Problem::manifest(e));
            None
        }
    };
    let entry = parsed
        .as_ref()
        .map(|m| m.entry.clone())
        .unwrap_or_else(|| "main.lua".to_string());
    if parsed.as_ref().is_some_and(|m| m.runtime == "wasm") {
        problems.push(Problem::manifest(format!(
            "{}: runtime: the editor writes Lua plugins only",
            label
        )));
        return (parsed, problems);
    }
    if script.len() as u64 > MAX_SCRIPT_BYTES {
        problems.push(Problem {
            file: entry,
            line: None,
            message: format!("larger than {} KiB", MAX_SCRIPT_BYTES / 1024),
        });
        return (parsed, problems);
    }
    if let Err(message) = syntax(script, &entry) {
        problems.push(Problem {
            line: error_line(&message, &entry),
            file: entry,
            message,
        });
    }
    (parsed, problems)
}

/// Compile `script` in a fresh sandbox without running it
fn syntax(script: &str, entry: &str) -> Result<(), String> {
    let lua = sandbox::new_state(64, Default::default()).map_err(|e| sandbox::error_text(&e))?;
    lua.load(script)
        .set_name(format!("@{}", entry))
        .into_function()
        .map(|_| ())
        .map_err(|e| sandbox::error_text(&e))
}

fn version(path: &Path) -> String {
    let Ok(meta) = std::fs::metadata(path) else {
        return String::new();
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis());
    format!("{}-{}", meta.len(), modified)
}

fn read_file(dir: &Path, name: &str) -> SourceFile {
    let path = dir.join(name);
    let text = match std::fs::metadata(&path) {
        Ok(m) if m.len() > MAX_SCRIPT_BYTES => String::new(),
        _ => std::fs::read_to_string(&path).unwrap_or_default(),
    };
    SourceFile {
        path: name.to_string(),
        text,
        version: version(&path),
    }
}

/// Write `text` to `dir/name` through a temporary file, keeping the old one as `.bak`
fn write_file(dir: &Path, name: &str, text: &str) -> Result<(), SourceError> {
    let io = |e: std::io::Error| SourceError::Io(format!("{}: {}", dir.join(name).display(), e));
    let path = dir.join(name);
    let tmp = dir.join(format!("{}.tmp", name));
    std::fs::write(&tmp, text).map_err(io)?;
    if path.exists() {
        std::fs::rename(&path, dir.join(format!("{}.bak", name))).map_err(io)?;
    }
    std::fs::rename(&tmp, &path).map_err(io)
}

/// A plain file name: no folder, not hidden
fn plain(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\'])
        && Path::new(name).file_name().is_some_and(|n| n == name)
}

/// `starter`'s manifest with its name set to `name`
fn renamed(manifest: &str, name: &str) -> String {
    let mut done = false;
    manifest
        .lines()
        .map(|line| {
            if !done && line.trim_start().starts_with("name =") {
                done = true;
                format!("name = \"{}\"", name)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// The files of `found`
pub fn read(found: &Found, editor: bool) -> PluginSource {
    let runtime = found
        .manifest
        .as_ref()
        .map_or("lua", |m| m.runtime.as_str())
        .to_string();
    let entry = found
        .manifest
        .as_ref()
        .map_or("main.lua", |m| m.entry.as_str())
        .to_string();
    let wasm = runtime == "wasm" || found.wasm.is_some();
    let (files, other_files) = match &found.source {
        Source::Bundled => {
            let (toml, script) = bundled_source(&found.name).unwrap_or_default();
            let file = |path: &str, text: &str| SourceFile {
                path: path.to_string(),
                text: text.to_string(),
                version: String::new(),
            };
            (
                vec![file(MANIFEST_FILE, toml), file(&entry, script)],
                Vec::new(),
            )
        }
        Source::Folder(dir) => {
            let mut files = vec![read_file(dir, MANIFEST_FILE)];
            if !wasm {
                files.push(read_file(dir, &entry));
            }
            let shown: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
            let mut other: Vec<String> = std::fs::read_dir(dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .filter(|n| !shown.contains(n) && !n.starts_with('.'))
                        .collect()
                })
                .unwrap_or_default();
            other.sort();
            (files, other)
        }
    };
    PluginSource {
        name: found.name.clone(),
        runtime: if wasm { "wasm".into() } else { "lua".into() },
        source: found.source.as_str(),
        editable: editor && !wasm && matches!(found.source, Source::Folder(_)),
        files,
        other_files,
    }
}

impl PluginsService {
    /// `name`'s files
    pub fn workbench_source(&self, name: &str, editor: bool) -> Option<PluginSource> {
        let (found, _) = self.found_with_settings(name)?;
        Some(read(&found, editor))
    }

    /// Make plugin `name` from `starter` in the plugins folder and load it (disabled)
    pub fn workbench_new(&self, starter: &str, name: &str) -> Result<(), SourceError> {
        let (_, _, _, manifest, script) = STARTERS
            .iter()
            .find(|s| s.0 == starter)
            .ok_or(SourceError::NotFound)?;
        self.create(name, &renamed(manifest, name), script)
    }

    /// Copy bundled plugin `name` to a folder of the same name, which replaces it
    pub fn workbench_copy(&self, name: &str) -> Result<(), SourceError> {
        let (found, _) = self
            .found_with_settings(name)
            .ok_or(SourceError::NotFound)?;
        if found.source != Source::Bundled {
            return Err(SourceError::NotEditable(format!(
                "{} is not a bundled plugin",
                name
            )));
        }
        let (manifest, script) = bundled_source(name).ok_or(SourceError::NotFound)?;
        self.create(name, manifest, script)
    }

    fn create(&self, name: &str, manifest: &str, script: &str) -> Result<(), SourceError> {
        if !valid_name(name) {
            return Err(SourceError::BadName(name.to_string()));
        }
        let dir = self.dir().ok_or(SourceError::NoFolder)?;
        let folder = dir.join(name);
        let bundled_copy = bundled_source(name).is_some_and(|(m, _)| m == manifest);
        let taken = self
            .found_with_settings(name)
            .is_some_and(|(f, _)| !(bundled_copy && f.source == Source::Bundled));
        if folder.exists() || taken {
            return Err(SourceError::Taken(name.to_string()));
        }
        let io = |e: std::io::Error| SourceError::Io(format!("{}: {}", folder.display(), e));
        std::fs::create_dir_all(&folder).map_err(io)?;
        std::fs::write(folder.join(MANIFEST_FILE), manifest).map_err(io)?;
        std::fs::write(folder.join("main.lua"), script).map_err(io)?;
        self.reload_only(&[name.to_string()]);
        Ok(())
    }

    /// Write `name`'s files, then reload that plugin alone. Problems do not stop the
    /// save; they come back, and the plugin shows as broken until fixed.
    pub fn workbench_save(
        &self,
        name: &str,
        request: &SaveRequest,
    ) -> Result<Vec<Problem>, SourceError> {
        let (found, _) = self
            .found_with_settings(name)
            .ok_or(SourceError::NotFound)?;
        let Source::Folder(dir) = &found.source else {
            return Err(SourceError::NotEditable(
                "a bundled plugin is read-only; copy it to edit".to_string(),
            ));
        };
        if found.wasm.is_some() || found.manifest.as_ref().is_some_and(|m| m.runtime == "wasm") {
            return Err(SourceError::NotEditable(
                "a WebAssembly plugin is edited as source elsewhere; install the new build"
                    .to_string(),
            ));
        }
        let (manifest, problems) = check(&request.manifest, &request.script, &found.source);
        let current = found
            .manifest
            .as_ref()
            .map_or("main.lua".to_string(), |m| m.entry.clone());
        let entry = manifest
            .as_ref()
            .map(|m| m.entry.clone())
            .filter(|e| plain(e))
            .unwrap_or(current);
        if !request.force {
            for file in [MANIFEST_FILE, entry.as_str()] {
                let was = request.versions.get(file).map(String::as_str).unwrap_or("");
                if version(&dir.join(file)) != was {
                    return Err(SourceError::Stale(file.to_string()));
                }
            }
        }
        write_file(dir, MANIFEST_FILE, &request.manifest)?;
        write_file(dir, &entry, &request.script)?;
        let mut names = vec![name.to_string()];
        if let Some(m) = manifest.filter(|m| m.name != name) {
            names.push(m.name);
        }
        self.reload_only(&names);
        Ok(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_errors_name_the_file_and_line() {
        let source = Source::Bundled;
        let manifest = renamed(STARTERS[0].3, "x");
        let (m, problems) = check(&manifest, "local a = 1\n\nlocal = 2\n", &source);
        assert!(m.is_some());
        assert_eq!(problems.len(), 1, "{:?}", problems);
        assert_eq!(problems[0].file, "main.lua");
        assert_eq!(problems[0].line, Some(3));
        assert!(
            problems[0].message.starts_with("main.lua:3:"),
            "{}",
            problems[0].message
        );
    }

    #[test]
    fn manifest_errors_read_like_a_reload() {
        let (m, problems) = check(
            "name = \"x\"\nversion = \"1\"\nevents = [\"download.done\"]\n",
            "return {}",
            &Source::Bundled,
        );
        assert!(m.is_none());
        assert_eq!(
            problems[0].message,
            "plugin.toml: events: unknown event type \"download.done\""
        );
    }

    #[test]
    fn every_starter_checks_clean() {
        for (id, _, _, manifest, script) in STARTERS {
            let (m, problems) = check(&renamed(manifest, id), script, &Source::Bundled);
            assert!(problems.is_empty(), "{}: {:?}", id, problems);
            assert_eq!(m.unwrap().name, *id);
        }
    }

    #[test]
    fn saves_keep_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.lua"), "old").unwrap();
        write_file(dir.path(), "main.lua", "new").unwrap();
        let read = |n: &str| std::fs::read_to_string(dir.path().join(n)).unwrap();
        assert_eq!(
            (read("main.lua"), read("main.lua.bak")),
            ("new".into(), "old".into())
        );
        assert!(!dir.path().join("main.lua.tmp").exists());
    }

    #[test]
    fn only_plain_names() {
        assert!(plain("main.lua"));
        assert!(!plain("../x.lua") && !plain(".hidden") && !plain("a/b.lua"));
    }
}
