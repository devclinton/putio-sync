//! Per-folder file rules (feature 009)
//!
//! A rule is attached to a folder path relative to the Put.io root, which is also the
//! folder under `target_directory` the files land in. It applies to that folder and
//! everything below it. For each setting the deepest rule on a file's path that sets it
//! wins; settings no rule sets come from `[download]` (research R2).
//!
//! The type filter is folded from the root down (R3): the global `blocked_extensions`
//! is the start, a rule's preset replaces the inherited filter, its `allow` extensions
//! are then allowed and its `block` extensions blocked.
//!
//! Feature 020 adds `include` (Only download) and `exclude` (Never download) patterns,
//! inherited like the other settings from `[download] whitelist_patterns` /
//! `blacklist_patterns`. A file is refused when its folder is skipped, then when it
//! matches `exclude`, then when it misses `include`, then by the type filter.

use crate::config::{DownloadConfig, ExistingFile};
use crate::error::{CoreError, CoreResult};
use crate::pattern::{self, Pattern};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::Path;

/// One folder rule (`[[download.rules]]`). Every setting is optional; an unset one is
/// inherited from the parent folders or `[download]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderRule {
    /// Folder path relative to the Put.io root, e.g. `Movies/Kids`
    pub path: String,
    /// Preset that replaces the inherited type filter
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// Extensions allowed on top of the preset or inherited filter
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    /// Extensions blocked on top of the preset or inherited filter; wins over `allow`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub block: Vec<String>,
    /// Only download files matching this pattern; `""` clears an inherited one
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<String>,
    /// Never download files matching this pattern; `""` clears an inherited one
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<String>,
    /// Download nothing below this folder
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip: Option<bool>,
    /// Delete files the type filter blocks from Put.io
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_blocked: Option<bool>,
    /// Per-folder `delete_after_download`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_after_download: Option<bool>,
    /// Per-folder `on_existing`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_existing: Option<ExistingFile>,
    /// Per-folder `skip_existing`, replaced by `on_existing` and still read
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_existing: Option<bool>,
}

impl FolderRule {
    /// True when the rule sets nothing, so it can be dropped
    pub fn is_empty(&self) -> bool {
        self.preset.is_none()
            && self.allow.is_empty()
            && self.block.is_empty()
            && self.include.is_none()
            && self.exclude.is_none()
            && self.skip.is_none()
            && self.delete_blocked.is_none()
            && self.delete_after_download.is_none()
            && self.on_existing.is_none()
            && self.skip_existing.is_none()
    }

    /// This rule's choice for existing files: `on_existing`, else the old `skip_existing`
    pub fn existing(&self) -> Option<ExistingFile> {
        self.on_existing.or(self.skip_existing.map(Into::into))
    }
}

/// A named type filter (`[download.presets.<name>]`): an allow list (`only`) or a block
/// list (`block`, used when `only` is absent)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    /// Only these extensions are downloaded
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only: Option<Vec<String>>,
    /// These extensions are not downloaded
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub block: Vec<String>,
}

/// A preset that ships with putio-sync; a `[download.presets]` entry with its name
/// replaces it
#[derive(Debug)]
pub struct BuiltinPreset {
    /// Config name
    pub name: &'static str,
    /// Name shown in the UI
    pub label: &'static str,
    /// Allow list, or `None` for a block list
    pub only: Option<&'static [&'static str]>,
    /// Block list
    pub block: &'static [&'static str],
}

impl BuiltinPreset {
    /// The preset as a config value
    pub fn preset(&self) -> Preset {
        Preset {
            only: self.only.map(strings),
            block: strings(self.block),
        }
    }
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// The built-in presets, in the order the UI lists them
pub const BUILTIN_PRESETS: &[BuiltinPreset] = &[
    BuiltinPreset {
        name: "video",
        label: "Video only",
        only: Some(&[
            "mkv", "mp4", "m4v", "avi", "mov", "wmv", "mpg", "mpeg", "ts", "m2ts", "webm", "flv",
            "srt", "sub", "idx", "ass", "ssa", "vtt",
        ]),
        block: &[],
    },
    BuiltinPreset {
        name: "audio",
        label: "Audio only",
        only: Some(&[
            "mp3", "flac", "m4a", "m4b", "aac", "ogg", "opus", "wav", "wma", "alac", "ape", "aiff",
            "cue",
        ]),
        block: &[],
    },
    BuiltinPreset {
        name: "ebooks",
        label: "Ebooks only",
        only: Some(&[
            "epub", "mobi", "azw", "azw3", "pdf", "cbz", "cbr", "djvu", "fb2",
        ]),
        block: &[],
    },
    BuiltinPreset {
        name: "no-binaries",
        label: "No binaries",
        only: None,
        // The default `blocked_extensions` plus libraries, packages and scripts
        block: &[
            "exe", "msi", "bat", "cmd", "com", "scr", "pif", "vbs", "js", "jar", "ps1", "lnk",
            "apk", "dmg", "pkg", "dll", "so", "bin", "deb", "rpm", "appimage", "run", "sh", "app",
        ],
    },
    BuiltinPreset {
        name: "everything",
        label: "Everything",
        only: None,
        block: &[],
    },
];

/// The built-in preset called `name`
pub fn builtin(name: &str) -> Option<&'static BuiltinPreset> {
    BUILTIN_PRESETS.iter().find(|p| p.name == name)
}

/// The UI name of a preset: the built-in label, or the config name
pub fn preset_label(name: &str) -> String {
    builtin(name).map_or_else(|| name.to_string(), |p| p.label.to_string())
}

/// A Put.io name as it appears in a download path: `/ \ : * ? " < > |` become `_`.
/// The download manager builds `relative_path` with this, so rule paths match it.
pub fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect()
}

fn invalid(message: String) -> CoreError {
    CoreError::ConfigError(message)
}

/// Normalise a rule path: split on `/`, drop empty segments, sanitise each segment.
/// The root (empty), `.` and `..` are refused.
pub fn normalize_path(path: &str) -> CoreResult<String> {
    let segments: Vec<&str> = path
        .split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if segments.is_empty() {
        return Err(invalid(
            "A folder rule needs a folder path; the root uses the [download] settings".into(),
        ));
    }
    if segments.iter().any(|s| *s == "." || *s == "..") {
        return Err(invalid(format!(
            "Folder rule path '{path}' must not contain '.' or '..'"
        )));
    }
    Ok(segments
        .iter()
        .map(|s| sanitize_name(s))
        .collect::<Vec<_>>()
        .join("/"))
}

/// Normalise an extension: leading `.` stripped, lowercased. Empty ones and ones with a
/// slash or whitespace are refused.
pub fn normalize_extension(ext: &str) -> CoreResult<String> {
    let e = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    if e.is_empty() || e.contains(['/', '\\']) || e.chars().any(char::is_whitespace) {
        return Err(invalid(format!("Invalid extension '{ext}'")));
    }
    Ok(e)
}

/// Preset names: `[a-z0-9][a-z0-9_-]{0,31}`
pub fn valid_preset_name(name: &str) -> bool {
    let mut chars = name.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    first_ok
        && name.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn check_extensions(list: &[String], what: &str) -> CoreResult<()> {
    for ext in list {
        normalize_extension(ext).map_err(|e| invalid(format!("{what}: {e}")))?;
    }
    Ok(())
}

/// Check a preset's name and lists
pub fn validate_preset(name: &str, preset: &Preset) -> CoreResult<()> {
    if !valid_preset_name(name) {
        return Err(invalid(format!(
            "Invalid preset name '{name}': use lowercase letters, digits, '-' and '_' (up to 32)"
        )));
    }
    let what = format!("Preset {name}");
    match &preset.only {
        Some(_) if !preset.block.is_empty() => Err(invalid(format!(
            "{what}: set either only or block, not both"
        ))),
        Some(only) if only.is_empty() => Err(invalid(format!(
            "{what}: only must list at least one extension"
        ))),
        Some(only) => check_extensions(only, &what),
        None => check_extensions(&preset.block, &what),
    }
}

/// Check `[[download.rules]]` and `[download.presets]`: preset names and lists, rule
/// paths (unique after normalising), extensions and preset references
pub fn validate(rules: &[FolderRule], presets: &BTreeMap<String, Preset>) -> CoreResult<()> {
    for (name, preset) in presets {
        validate_preset(name, preset)?;
    }
    let mut seen = HashSet::new();
    for rule in rules {
        let path = normalize_path(&rule.path)?;
        let what = format!("Folder rule {path}");
        if !seen.insert(path.clone()) {
            return Err(invalid(format!("Two folder rules for {path}")));
        }
        if let Some(name) = &rule.preset {
            if builtin(name).is_none() && !presets.contains_key(name) {
                return Err(invalid(format!("{what}: unknown preset '{name}'")));
            }
        }
        check_extensions(&rule.allow, &what)?;
        check_extensions(&rule.block, &what)?;
        check_pattern(rule.include.as_deref(), &format!("{what}: Only download"))?;
        check_pattern(rule.exclude.as_deref(), &format!("{what}: Never download"))?;
    }
    Ok(())
}

/// Check the account-wide `[download] whitelist_patterns` / `blacklist_patterns` strictly
pub fn validate_root_patterns(config: &DownloadConfig) -> CoreResult<()> {
    check_pattern(
        config.whitelist_patterns.as_deref(),
        "[download] whitelist_patterns (Only download)",
    )?;
    check_pattern(
        config.blacklist_patterns.as_deref(),
        "[download] blacklist_patterns (Never download)",
    )
}

/// Check one saved pattern strictly; `what` names it in the error
pub fn check_pattern(text: Option<&str>, what: &str) -> CoreResult<()> {
    match text.map(pattern::check) {
        Some(Err(e)) => Err(invalid(format!("{what}: {e}"))),
        _ => Ok(()),
    }
}

/// Check the account-wide `[download] preset` names a built-in or `[download.presets]` entry
pub fn validate_root_preset(
    preset: Option<&str>,
    presets: &BTreeMap<String, Preset>,
) -> CoreResult<()> {
    match preset {
        Some(name) if builtin(name).is_none() && !presets.contains_key(name) => Err(invalid(
            format!("[download] preset: unknown preset '{name}'"),
        )),
        _ => Ok(()),
    }
}

/// The rule names a user preset is used by
pub fn rules_using<'a>(rules: &'a [FolderRule], preset: &str) -> Vec<&'a str> {
    rules
        .iter()
        .filter(|r| r.preset.as_deref() == Some(preset))
        .map(|r| r.path.as_str())
        .collect()
}

/// The lowercased last extension of a file name (`a.mkv.exe` → `exe`; `.exe` has none)
pub fn extension(name: &str) -> Option<String> {
    Some(Path::new(name).extension()?.to_str()?.to_ascii_lowercase())
}

/// Which extensions a folder downloads
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TypeFilter {
    /// Allow list; `None` allows everything not blocked
    pub only: Option<BTreeSet<String>>,
    /// Never downloaded; wins over `only`
    pub blocked: BTreeSet<String>,
}

fn ext_set(list: &[String]) -> BTreeSet<String> {
    list.iter()
        .filter_map(|e| normalize_extension(e).ok())
        .collect()
}

impl TypeFilter {
    fn from_preset(preset: &Preset) -> Self {
        Self {
            only: preset.only.as_deref().map(ext_set),
            blocked: ext_set(&preset.block),
        }
    }

    /// Why this filter refuses `name`, if it does
    fn refuses(&self, name: &str, preset: Option<&str>) -> Option<Reason> {
        let ext = extension(name);
        if let Some(e) = ext.as_ref().filter(|e| self.blocked.contains(*e)) {
            return Some(Reason::Blocked {
                extension: e.clone(),
            });
        }
        match &self.only {
            Some(only) if !ext.as_ref().is_some_and(|e| only.contains(e)) => {
                Some(Reason::NotAllowed {
                    extension: ext,
                    preset: preset.map(str::to_string),
                })
            }
            _ => None,
        }
    }
}

/// A value and the rule path it came from (`None`: the `[download]` settings)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Sourced<T> {
    /// The value
    pub value: T,
    /// Rule path, or `None` for the global settings
    pub from: Option<String>,
}

impl<T> Sourced<T> {
    fn global(value: T) -> Self {
        Self { value, from: None }
    }

    fn set(&mut self, value: Option<T>, path: &str) {
        if let Some(value) = value {
            self.value = value;
            self.from = Some(path.to_string());
        }
    }
}

/// The settings in force in one folder after inheritance
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Effective {
    /// Type filter and the deepest rule that changed it
    pub filter: Sourced<TypeFilter>,
    /// The preset in force, if any rule on the path set one
    pub preset: Sourced<Option<String>>,
    /// Only download files matching this (empty: every file)
    pub include: Sourced<Pattern>,
    /// Never download files matching this (empty: none)
    pub exclude: Sourced<Pattern>,
    /// Download nothing here
    pub skip: Sourced<bool>,
    /// Delete type-blocked files from Put.io
    pub delete_blocked: Sourced<bool>,
    /// Delete from Put.io after a verified download
    pub delete_after_download: Sourced<bool>,
    /// What to do when a file already exists locally
    pub on_existing: Sourced<ExistingFile>,
    /// The folder this was resolved for, for path patterns
    #[serde(skip)]
    pub folder: String,
}

impl Effective {
    /// What happens to a file called `name` in this folder
    pub fn decide(&self, name: &str) -> Decision {
        if self.skip.value {
            return Decision::Skipped {
                rule: self.skip.from.clone(),
            };
        }
        let path = if self.folder.is_empty() {
            name.to_string()
        } else {
            format!("{}/{name}", self.folder)
        };
        let exclude = &self.exclude.value;
        if !exclude.is_empty() && exclude.matches(name, &path) {
            return Decision::Blocked {
                reason: Reason::Excluded {
                    pattern: exclude.as_str().to_string(),
                },
                rule: self.exclude.from.clone(),
            };
        }
        let include = &self.include.value;
        if !include.is_empty() && !include.matches(name, &path) {
            return Decision::Blocked {
                reason: Reason::NotIncluded {
                    pattern: include.as_str().to_string(),
                },
                rule: self.include.from.clone(),
            };
        }
        match self
            .filter
            .value
            .refuses(name, self.preset.value.as_deref())
        {
            Some(reason) => Decision::Blocked {
                reason,
                rule: self.filter.from.clone(),
            },
            None => Decision::Download,
        }
    }
}

/// Why the type filter refused a file
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// The filter is an allow list without this extension
    NotAllowed {
        /// The file's extension, if it has one
        extension: Option<String>,
        /// The preset in force
        preset: Option<String>,
    },
    /// The extension is blocked
    Blocked {
        /// The file's extension
        extension: String,
    },
    /// The name matches the Never download pattern
    Excluded {
        /// The pattern text
        pattern: String,
    },
    /// The name misses the Only download pattern
    NotIncluded {
        /// The pattern text
        pattern: String,
    },
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::Blocked { extension } => write!(f, "extension {extension} is blocked"),
            Reason::Excluded { pattern } => write!(f, "matches never-download pattern `{pattern}`"),
            Reason::NotIncluded { pattern } => {
                write!(f, "doesn't match only-download pattern `{pattern}`")
            }
            Reason::NotAllowed { extension, preset } => {
                match extension {
                    Some(e) => write!(f, "extension {e} is not allowed")?,
                    None => write!(f, "no extension; not allowed")?,
                }
                match preset {
                    Some(p) => write!(f, " by {}", preset_label(p)),
                    None => Ok(()),
                }
            }
        }
    }
}

/// What a rule does with one file
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Download it
    Download,
    /// A pattern or the type filter refuses it
    Blocked {
        /// Why
        reason: Reason,
        /// The rule path that set the pattern or filter, `None` for `[download]`
        rule: Option<String>,
    },
    /// Its folder is skipped
    Skipped {
        /// The rule path that set `skip`
        rule: Option<String>,
    },
}

impl Decision {
    /// The deciding rule path, `None` for the global settings (or a download)
    pub fn rule(&self) -> Option<&str> {
        match self {
            Decision::Download => None,
            Decision::Blocked { rule, .. } | Decision::Skipped { rule } => rule.as_deref(),
        }
    }

    /// The reason in words, empty for a download
    pub fn reason(&self) -> String {
        match self {
            Decision::Download => String::new(),
            Decision::Blocked { reason, .. } => reason.to_string(),
            Decision::Skipped { .. } => "folder is skipped".to_string(),
        }
    }
}

/// `[download]` rules ready to resolve: the global settings, the rules by normalised
/// path and the presets (built-ins with config overrides)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleSet {
    blocked: Vec<String>,
    root_preset: Option<String>,
    include: Pattern,
    exclude: Pattern,
    delete_after_download: bool,
    on_existing: ExistingFile,
    rules: HashMap<String, FolderRule>,
    presets: BTreeMap<String, Preset>,
}

impl Default for RuleSet {
    fn default() -> Self {
        Self::from_config(&DownloadConfig::default())
    }
}

impl RuleSet {
    /// Build from `[download]`. Invalid entries (refused by `validate` at load and on
    /// save) are left out rather than failing.
    pub fn from_config(config: &DownloadConfig) -> Self {
        let mut presets: BTreeMap<String, Preset> = BUILTIN_PRESETS
            .iter()
            .map(|p| (p.name.to_string(), p.preset()))
            .collect();
        presets.extend(config.presets.clone());
        let rules = config
            .rules
            .iter()
            .filter_map(|rule| {
                let path = normalize_path(&rule.path).ok()?;
                Some((
                    path.clone(),
                    FolderRule {
                        path,
                        ..rule.clone()
                    },
                ))
            })
            .collect();
        let root_preset = config
            .preset
            .clone()
            .filter(|name| presets.contains_key(name));
        Self {
            blocked: config.blocked_extensions.clone(),
            root_preset,
            include: Pattern::parse(config.whitelist_patterns.as_deref().unwrap_or("")),
            exclude: Pattern::parse(config.blacklist_patterns.as_deref().unwrap_or("")),
            delete_after_download: config.delete_after_download,
            on_existing: config.on_existing(),
            rules,
            presets,
        }
    }

    /// True when no folder rule, account-wide preset or pattern exists
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
            && self.root_preset.is_none()
            && self.include.is_empty()
            && self.exclude.is_empty()
    }

    /// The root filter: the account-wide `[download] preset` when set, else
    /// `blocked_extensions`
    fn root_filter(&self) -> TypeFilter {
        match self.root_preset.as_ref().and_then(|n| self.presets.get(n)) {
            Some(preset) => TypeFilter::from_preset(preset),
            None => TypeFilter {
                only: None,
                blocked: ext_set(&self.blocked),
            },
        }
    }

    /// The settings in force in `folder_path` (relative to the Put.io root, `""` for it)
    pub fn resolve(&self, folder_path: &str) -> Effective {
        let mut effective = Effective {
            filter: Sourced::global(self.root_filter()),
            preset: Sourced::global(self.root_preset.clone()),
            include: Sourced::global(self.include.clone()),
            exclude: Sourced::global(self.exclude.clone()),
            skip: Sourced::global(false),
            delete_blocked: Sourced::global(false),
            delete_after_download: Sourced::global(self.delete_after_download),
            on_existing: Sourced::global(self.on_existing),
            folder: folder_path
                .split('/')
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("/"),
        };
        if self.rules.is_empty() {
            return effective;
        }
        let mut prefix = String::new();
        for segment in folder_path.split('/').filter(|s| !s.is_empty()) {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(segment);
            if let Some(rule) = self.rules.get(&prefix) {
                self.apply(&mut effective, rule, &prefix);
            }
        }
        effective
    }

    fn apply(&self, effective: &mut Effective, rule: &FolderRule, path: &str) {
        let filter = &mut effective.filter;
        let mut changed = false;
        if let Some(name) = &rule.preset {
            if let Some(preset) = self.presets.get(name) {
                filter.value = TypeFilter::from_preset(preset);
                effective.preset.set(Some(Some(name.clone())), path);
                changed = true;
            }
        }
        for ext in ext_set(&rule.allow) {
            filter.value.blocked.remove(&ext);
            if let Some(only) = filter.value.only.as_mut() {
                only.insert(ext);
            }
            changed = true;
        }
        for ext in ext_set(&rule.block) {
            filter.value.blocked.insert(ext);
            changed = true;
        }
        if changed {
            filter.from = Some(path.to_string());
        }
        let parse = |p: &Option<String>| p.as_deref().map(Pattern::parse);
        effective.include.set(parse(&rule.include), path);
        effective.exclude.set(parse(&rule.exclude), path);
        effective.skip.set(rule.skip, path);
        effective.delete_blocked.set(rule.delete_blocked, path);
        effective
            .delete_after_download
            .set(rule.delete_after_download, path);
        effective.on_existing.set(rule.existing(), path);
    }

    /// What happens to `name` in `folder_path`
    pub fn decide(&self, folder_path: &str, name: &str) -> Decision {
        self.resolve(folder_path).decide(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(rules: Vec<FolderRule>) -> DownloadConfig {
        DownloadConfig {
            rules,
            ..DownloadConfig::default()
        }
    }

    fn rule(path: &str) -> FolderRule {
        FolderRule {
            path: path.to_string(),
            ..FolderRule::default()
        }
    }

    fn preset_rule(path: &str, preset: &str) -> FolderRule {
        FolderRule {
            preset: Some(preset.to_string()),
            ..rule(path)
        }
    }

    fn set(rules: Vec<FolderRule>) -> RuleSet {
        RuleSet::from_config(&config(rules))
    }

    fn downloads(rules: &RuleSet, folder: &str, name: &str) -> bool {
        rules.decide(folder, name) == Decision::Download
    }

    #[test]
    fn no_rules_is_the_global_blocklist() {
        let rules = set(vec![]);
        assert!(!downloads(&rules, "Movies", "setup.exe"));
        assert!(downloads(&rules, "Movies", "info.nfo"));
        assert!(downloads(&rules, "", "README"));
        let d = rules.decide("Movies", "setup.exe");
        assert_eq!(d.rule(), None);
        assert_eq!(d.reason(), "extension exe is blocked");
    }

    #[test]
    fn preset_applies_to_the_folder_and_below() {
        let rules = set(vec![preset_rule("Movies", "video")]);
        assert!(downloads(&rules, "Movies/Film (2020)", "film.MKV"));
        assert!(downloads(&rules, "Movies", "film.srt"));
        assert!(!downloads(&rules, "Movies/Film (2020)", "info.nfo"));
        assert!(!downloads(&rules, "Movies", "README"));
        assert!(downloads(&rules, "Music", "info.nfo"));
        // Not a prefix match on names
        assert!(downloads(&rules, "Movies2", "info.nfo"));
        let d = rules.decide("Movies/Film", "info.nfo");
        assert_eq!(d.rule(), Some("Movies"));
        assert_eq!(d.reason(), "extension nfo is not allowed by Video only");
        assert_eq!(
            rules.decide("Movies", "README").reason(),
            "no extension; not allowed by Video only"
        );
    }

    #[test]
    fn deeper_rule_wins() {
        let rules = set(vec![
            preset_rule("Movies", "video"),
            preset_rule("Movies/Extras", "everything"),
        ]);
        assert!(downloads(&rules, "Movies/Extras", "notes.txt"));
        assert!(downloads(&rules, "Movies/Extras/a", "tool.exe"));
        assert!(!downloads(&rules, "Movies", "notes.txt"));
    }

    #[test]
    fn allow_alone_unblocks_a_global_extension() {
        let rules = set(vec![FolderRule {
            allow: vec![".EXE".into()],
            ..rule("Software")
        }]);
        assert!(downloads(&rules, "Software", "tool.exe"));
        assert!(!downloads(&rules, "Software", "setup.msi"));
        assert!(!downloads(&rules, "Other", "tool.exe"));
    }

    #[test]
    fn block_alone_keeps_the_inherited_filter() {
        let rules = set(vec![FolderRule {
            block: vec!["nfo".into()],
            ..rule("Movies")
        }]);
        assert!(!downloads(&rules, "Movies", "info.nfo"));
        assert!(!downloads(&rules, "Movies", "setup.exe"));
        assert!(downloads(&rules, "Movies", "film.mkv"));
    }

    #[test]
    fn preset_plus_allow_and_block() {
        let rules = set(vec![FolderRule {
            allow: vec!["jpg".into()],
            block: vec!["wav".into()],
            ..preset_rule("Music", "audio")
        }]);
        assert!(downloads(&rules, "Music/Album", "cover.jpg"));
        assert!(downloads(&rules, "Music/Album", "01.flac"));
        assert!(!downloads(&rules, "Music/Album", "01.wav"));
        assert!(!downloads(&rules, "Music/Album", "info.txt"));
    }

    #[test]
    fn last_extension_counts() {
        let rules = set(vec![preset_rule("Movies", "video")]);
        assert!(!downloads(&rules, "Movies", "a.mkv.exe"));
        assert!(!downloads(&rules, "Movies", ".mkv"));
    }

    #[test]
    fn skip_and_unskip() {
        let rules = set(vec![
            FolderRule {
                skip: Some(true),
                ..rule("Seeding")
            },
            FolderRule {
                skip: Some(false),
                ..rule("Seeding/Keep")
            },
        ]);
        assert_eq!(
            rules.decide("Seeding/x", "film.mkv"),
            Decision::Skipped {
                rule: Some("Seeding".into())
            }
        );
        assert!(downloads(&rules, "Seeding/Keep", "film.mkv"));
        assert!(!downloads(&rules, "Seeding/Keep", "setup.exe"));
    }

    #[test]
    fn settings_inherit_one_by_one_with_sources() {
        let rules = set(vec![
            FolderRule {
                delete_after_download: Some(true),
                delete_blocked: Some(true),
                ..preset_rule("Movies", "video")
            },
            FolderRule {
                delete_after_download: Some(false),
                ..rule("Movies/Kids")
            },
        ]);
        let e = rules.resolve("Movies/Kids/Film");
        assert_eq!(e.preset.value.as_deref(), Some("video"));
        assert_eq!(e.preset.from.as_deref(), Some("Movies"));
        assert_eq!(e.filter.from.as_deref(), Some("Movies"));
        assert!(e.delete_blocked.value);
        assert_eq!(e.delete_blocked.from.as_deref(), Some("Movies"));
        assert!(!e.delete_after_download.value);
        assert_eq!(e.delete_after_download.from.as_deref(), Some("Movies/Kids"));
        assert_eq!(e.on_existing.value, ExistingFile::Resume);
        assert_eq!(e.on_existing.from, None);
        let root = rules.resolve("");
        assert!(!root.delete_after_download.value);
        assert_eq!(root.filter.from, None);
    }

    #[test]
    fn on_existing_nearest_rule_wins_and_old_key_maps() {
        let rules = set(vec![
            FolderRule {
                on_existing: Some(ExistingFile::KeepBoth),
                ..rule("Backups")
            },
            FolderRule {
                skip_existing: Some(false),
                ..rule("Backups/Daily")
            },
        ]);
        let e = rules.resolve("Backups/Weekly");
        assert_eq!(e.on_existing.value, ExistingFile::KeepBoth);
        assert_eq!(e.on_existing.from.as_deref(), Some("Backups"));
        let e = rules.resolve("Backups/Daily/Mon");
        assert_eq!(e.on_existing.value, ExistingFile::Overwrite);
        assert_eq!(e.on_existing.from.as_deref(), Some("Backups/Daily"));
        let e = rules.resolve("Movies");
        assert_eq!(e.on_existing.value, ExistingFile::Resume);
        assert_eq!(e.on_existing.from, None);
    }

    #[test]
    fn user_presets_and_overrides() {
        let mut cfg = config(vec![
            preset_rule("Comics", "comics"),
            preset_rule("Movies", "video"),
        ]);
        cfg.presets.insert(
            "comics".into(),
            Preset {
                only: Some(vec!["cbz".into()]),
                block: vec![],
            },
        );
        cfg.presets.insert(
            "video".into(),
            Preset {
                only: Some(vec!["m2ts".into()]),
                block: vec![],
            },
        );
        let rules = RuleSet::from_config(&cfg);
        assert!(downloads(&rules, "Comics", "a.cbz"));
        assert!(!downloads(&rules, "Comics", "a.cbr"));
        assert!(downloads(&rules, "Movies", "a.m2ts"));
        assert!(!downloads(&rules, "Movies", "a.mkv"));
        assert_eq!(
            rules.decide("Comics", "a.cbr").reason(),
            "extension cbr is not allowed by comics"
        );
    }

    #[test]
    fn rule_paths_are_normalised() {
        let rules = set(vec![preset_rule("/Movies//Kids: 2/", "video")]);
        assert!(!downloads(&rules, "Movies/Kids_ 2", "info.nfo"));
        assert_eq!(normalize_path(" a / b ").unwrap(), "a/b");
        assert!(normalize_path("/").is_err());
        assert!(normalize_path("a/../b").is_err());
        assert!(normalize_path("./a").is_err());
    }

    #[test]
    fn validation() {
        let presets = BTreeMap::new();
        assert!(validate(&[preset_rule("Movies", "video")], &presets).is_ok());
        let err = |rules: &[FolderRule], presets: &BTreeMap<String, Preset>| {
            validate(rules, presets).unwrap_err().to_string()
        };
        assert!(err(&[preset_rule("Movies", "nope")], &presets).contains("unknown preset 'nope'"));
        assert!(err(&[rule("Movies"), rule("/Movies/")], &presets).contains("Two folder rules"));
        assert!(err(&[rule("")], &presets).contains("needs a folder path"));
        let bad_ext = FolderRule {
            allow: vec!["a b".into()],
            ..rule("Movies")
        };
        assert!(err(&[bad_ext], &presets).contains("Folder rule Movies"));

        let one = |name: &str, preset: Preset| BTreeMap::from([(name.to_string(), preset)]);
        let both = Preset {
            only: Some(vec!["a".into()]),
            block: vec!["b".into()],
        };
        assert!(err(&[], &one("x", both)).contains("not both"));
        let empty = Preset {
            only: Some(vec![]),
            block: vec![],
        };
        assert!(err(&[], &one("x", empty)).contains("at least one"));
        assert!(err(&[], &one("Bad Name", Preset::default())).contains("Invalid preset name"));
        assert!(validate(
            &[preset_rule("C", "comics")],
            &one("comics", Preset::default())
        )
        .is_ok());
    }

    #[test]
    fn builtins_are_valid() {
        for p in BUILTIN_PRESETS {
            validate_preset(p.name, &p.preset()).unwrap();
        }
        assert_eq!(preset_label("no-binaries"), "No binaries");
        assert_eq!(preset_label("comics"), "comics");
    }

    fn with_root(preset: Option<&str>, rules: Vec<FolderRule>) -> RuleSet {
        RuleSet::from_config(&DownloadConfig {
            preset: preset.map(str::to_string),
            ..config(rules)
        })
    }

    #[test]
    fn root_preset_filters_the_whole_account() {
        let rules = with_root(Some("video"), vec![]);
        assert!(!rules.is_empty());
        assert!(downloads(&rules, "", "a.mkv"));
        assert!(!downloads(&rules, "Music", "a.mp3"));
        let effective = rules.resolve("Music");
        assert_eq!(effective.preset.value.as_deref(), Some("video"));
        assert_eq!(effective.preset.from, None);
        assert_eq!(effective.filter.from, None);
    }

    #[test]
    fn folder_rule_overrides_the_root_preset() {
        let rules = with_root(Some("video"), vec![preset_rule("Music", "audio")]);
        assert!(downloads(&rules, "Music/Album", "a.mp3"));
        assert!(!downloads(&rules, "Movies", "a.mp3"));
        let skip = FolderRule {
            skip: Some(true),
            ..rule("Junk")
        };
        let rules = with_root(Some("everything"), vec![skip]);
        assert!(!downloads(&rules, "Junk", "a.mkv"));
        assert!(downloads(&rules, "Other", "a.exe"));
    }

    #[test]
    fn no_root_preset_keeps_blocked_extensions() {
        let rules = with_root(None, vec![]);
        assert!(rules.is_empty());
        assert_eq!(rules, RuleSet::default());
        assert!(!downloads(&rules, "", "setup.exe"));
        assert!(downloads(&rules, "", "a.mp3"));
        // An unknown name (refused by validation) falls back to the same
        assert_eq!(with_root(Some("nope"), vec![]), RuleSet::default());
    }

    #[test]
    fn root_preset_must_exist() {
        let none = BTreeMap::new();
        assert!(validate_root_preset(None, &none).is_ok());
        assert!(validate_root_preset(Some("no-binaries"), &none).is_ok());
        let err = validate_root_preset(Some("comics"), &none).unwrap_err();
        assert!(err.to_string().contains("unknown preset 'comics'"), "{err}");
        let user = BTreeMap::from([("comics".to_string(), Preset::default())]);
        assert!(validate_root_preset(Some("comics"), &user).is_ok());
    }

    fn patterns(path: &str, include: Option<&str>, exclude: Option<&str>) -> FolderRule {
        FolderRule {
            include: include.map(str::to_string),
            exclude: exclude.map(str::to_string),
            ..rule(path)
        }
    }

    fn with_globals(
        include: Option<&str>,
        exclude: Option<&str>,
        rules: Vec<FolderRule>,
    ) -> RuleSet {
        RuleSet::from_config(&DownloadConfig {
            whitelist_patterns: include.map(str::to_string),
            blacklist_patterns: exclude.map(str::to_string),
            ..config(rules)
        })
    }

    #[test]
    fn exclude_refuses_matching_files() {
        let rules = set(vec![patterns("Movies", None, Some("*sample*"))]);
        assert!(!downloads(&rules, "Movies", "Film.sample.mkv"));
        assert!(downloads(&rules, "Movies", "Film.mkv"));
        assert!(downloads(&rules, "TV", "Show.sample.mkv"));
        let d = rules.decide("Movies/2024", "Film.sample.mkv");
        assert_eq!(d.rule(), Some("Movies"));
        assert_eq!(d.reason(), "matches never-download pattern `*sample*`");
    }

    #[test]
    fn include_refuses_the_rest() {
        let rules = set(vec![patterns("Movies", Some("*.mkv"), None)]);
        assert!(downloads(&rules, "Movies", "a.mkv"));
        assert!(!downloads(&rules, "Movies", "a.avi"));
        assert_eq!(
            rules.decide("Movies", "a.avi").reason(),
            "doesn't match only-download pattern `*.mkv`"
        );
        let e = rules.resolve("Movies/Sub");
        assert_eq!(e.include.value.as_str(), "*.mkv");
        assert_eq!(e.include.from.as_deref(), Some("Movies"));
        assert!(e.exclude.value.is_empty());
    }

    #[test]
    fn deeper_pattern_wins_and_empty_clears() {
        let rules = set(vec![
            patterns("Movies", Some("*.mkv"), Some("*sample*")),
            patterns("Movies/Kids", Some("*.mp4"), None),
            patterns("Movies/Raw", Some(""), Some("")),
        ]);
        assert!(downloads(&rules, "Movies/Kids", "a.mp4"));
        assert!(!downloads(&rules, "Movies/Kids", "a.mkv"));
        assert!(!downloads(&rules, "Movies/Kids", "a.sample.mp4"));
        assert!(downloads(&rules, "Movies/Raw", "a.sample.avi"));
        let e = rules.resolve("Movies/Raw");
        assert!(e.include.value.is_empty());
        assert_eq!(e.include.from.as_deref(), Some("Movies/Raw"));
    }

    #[test]
    fn path_patterns_see_the_folder() {
        let rules = set(vec![patterns("Movies", None, Some("Sample/*"))]);
        assert!(!downloads(&rules, "Movies/Film/Sample", "a.mkv"));
        assert!(downloads(&rules, "Movies/Film", "a.mkv"));
        assert!(downloads(&rules, "Movies/Film/Samples", "a.mkv"));
        let root = with_globals(None, Some("Movies/Extras/*"), vec![]);
        assert!(!downloads(&root, "Movies/Extras", "b.mkv"));
        assert!(downloads(&root, "Movies", "b.mkv"));
    }

    #[test]
    fn patterns_never_unblock() {
        let rules = set(vec![patterns("Apps", Some("*.exe"), None)]);
        let d = rules.decide("Apps", "setup.exe");
        assert_eq!(d.reason(), "extension exe is blocked");
    }

    #[test]
    fn no_patterns_change_nothing() {
        let rules = set(vec![preset_rule("Movies", "video")]);
        let e = rules.resolve("Movies");
        assert!(e.include.value.is_empty() && e.exclude.value.is_empty());
        assert!(set(vec![]).is_empty());
        assert!(!with_globals(Some("*.mkv"), None, vec![]).is_empty());
        assert!(!patterns("A", None, Some("")).is_empty());
    }

    #[test]
    fn global_patterns_come_from_the_old_settings() {
        let rules = with_globals(Some("*.mkv,*.mp4"), Some("*sample*"), vec![]);
        assert!(downloads(&rules, "", "a.mkv"));
        assert!(downloads(&rules, "Movies", "b.MP4"));
        assert!(!downloads(&rules, "Movies", "c.avi"));
        assert!(!downloads(&rules, "Movies", "a.sample.mkv"));
        let d = rules.decide("Movies", "c.avi");
        assert_eq!(d.rule(), None);
        let rules = with_globals(Some("*.{mkv,mp4}"), None, vec![]);
        assert!(downloads(&rules, "", "a.mkv") && !downloads(&rules, "", "b.avi"));
    }

    #[test]
    fn combined_patterns() {
        let rules = set(vec![
            patterns("Movies", Some("*.mkv OR *.mp4 -sample"), None),
            patterns("TV", None, Some("Sample/* OR *.nfo")),
        ]);
        assert!(downloads(&rules, "Movies", "a.mkv"));
        assert!(downloads(&rules, "Movies", "a.MP4"));
        assert!(!downloads(&rules, "Movies", "a.sample.mkv"));
        assert!(!downloads(&rules, "Movies", "a.avi"));
        assert!(!downloads(&rules, "TV/Show/Sample", "e1.mkv"));
        assert!(!downloads(&rules, "TV/Show", "e1.nfo"));
        assert!(downloads(&rules, "TV/Show", "e1.mkv"));
    }

    #[test]
    fn bad_patterns_are_refused() {
        let presets = BTreeMap::new();
        let err = validate(&[patterns("Movies", None, Some("\"abc"))], &presets)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("Folder rule Movies: Never download: unclosed quote"),
            "{err}"
        );
        let err = validate(&[patterns("Movies", Some("*.{mkv"), None)], &presets)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("Folder rule Movies: Only download: unbalanced { }"),
            "{err}"
        );
        assert!(validate(
            &[patterns("Movies", Some("*.mkv,*.mp4"), Some(""))],
            &presets
        )
        .is_ok());
    }
}
