//! The setup wizard's usage profile (feature 056).
//!
//! The wizard asks what is downloaded and how it arrives. [`plan`] turns those
//! [`Answers`], and the top-level Put.io folders [`detect`] found, into
//! [`Recommendation`]s: each a few typed settings ([`Key`]) with a reason code the UI
//! words. [`rerun`] applies a plan to a config and keeps the settings changed by hand
//! since the profile last set them, so Settings can run the profile again.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{ArrKind, Config, PrecheckOverride};
use crate::error::{CoreError, CoreResult};
use crate::rules::{normalize_path, FolderRule};

/// What kind of files a user downloads
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Films
    Movies,
    /// Series
    Tv,
    /// Music
    Music,
    /// Ebooks and audiobooks
    Books,
    /// Programs and games
    Software,
    /// Anything else: no rule, defaults
    Other,
}

/// A watch folder's release check size for a kind
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Size {
    /// The `[precheck]` default (50 MB)
    Default,
    /// This many MB
    Mb(u64),
    /// No check: small files are normal
    Off,
}

impl Kind {
    /// Every kind, in the order the wizard shows them
    pub const ALL: [Kind; 6] = [
        Kind::Movies,
        Kind::Tv,
        Kind::Music,
        Kind::Books,
        Kind::Software,
        Kind::Other,
    ];

    /// `movies`, `tv`, ...
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Movies => "movies",
            Kind::Tv => "tv",
            Kind::Music => "music",
            Kind::Books => "books",
            Kind::Software => "software",
            Kind::Other => "other",
        }
    }

    /// Video kinds get subtitles and the DLNA note
    pub fn is_video(self) -> bool {
        matches!(self, Kind::Movies | Kind::Tv)
    }

    /// The preset for a folder of this kind
    fn preset(self, folder: &str) -> Option<&'static str> {
        match self {
            Kind::Movies | Kind::Tv => Some("video"),
            Kind::Music => Some("audio"),
            Kind::Books if folder.to_lowercase().contains("audio") => Some("audio"),
            Kind::Books => Some("ebooks"),
            // Lets the default blocked extensions (exe, msi, dmg, ...) through
            Kind::Software => Some("everything"),
            Kind::Other => None,
        }
    }

    fn size(self) -> Size {
        match self {
            Kind::Music => Size::Mb(5),
            Kind::Books => Size::Mb(1),
            Kind::Software => Size::Off,
            _ => Size::Default,
        }
    }

    /// Top-level folder names (lower case) that hold this kind
    fn names(self) -> &'static [&'static str] {
        match self {
            Kind::Movies => &["movies", "movie", "films", "film", "radarr"],
            Kind::Tv => &["tv", "tv shows", "series", "shows", "sonarr"],
            Kind::Music => &["music"],
            Kind::Books => &["books", "ebooks", "e-books", "audiobooks", "audio books"],
            Kind::Software => &["software", "apps", "applications", "programs", "games"],
            Kind::Other => &[],
        }
    }

    /// The folder a rule goes on when none was found
    fn default_folder(self) -> Option<&'static str> {
        match self {
            Kind::Movies => Some("Movies"),
            Kind::Tv => Some("TV"),
            Kind::Music => Some("Music"),
            Kind::Books => Some("Books"),
            Kind::Software => Some("Software"),
            Kind::Other => None,
        }
    }

    /// The *arr app that imports this kind
    fn arr(self) -> Option<ArrKind> {
        match self {
            Kind::Movies => Some(ArrKind::Radarr),
            Kind::Tv => Some(ArrKind::Sonarr),
            _ => None,
        }
    }
}

/// What the wizard's usage screen asked
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Answers {
    /// What is downloaded; at least one
    pub kinds: Vec<Kind>,
    /// The *arr apps that send downloads
    pub arr: Vec<ArrKind>,
    /// Files are also added by hand on Put.io
    pub by_hand: bool,
    /// The watch folder, when torrent files are dropped in one
    pub watch: Option<PathBuf>,
    /// The UI language (051), for subtitles
    pub language: String,
}

impl Answers {
    /// Checks the answers can make a plan
    pub fn validate(&self) -> CoreResult<()> {
        if self.kinds.is_empty() {
            return Err(CoreError::ConfigError(
                "pick at least one kind of download".to_string(),
            ));
        }
        if let Some(watch) = &self.watch {
            if watch.as_os_str().is_empty() {
                return Err(CoreError::ConfigError(
                    "the watch folder is empty".to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// A change the user made to one recommendation on the review screen
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Override {
    /// Switched off: none of its settings are written
    pub off: bool,
    /// A changed value, in the recommendation's own shape
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

/// One setting a recommendation writes
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Setting {
    /// The [`Key`] as a string
    pub key: String,
    /// Its value; `null` is the default
    pub value: Value,
}

/// One line of the review screen
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Recommendation {
    /// `rule:<folder>`, `precheck_watch`, `delete`, `subtitles` or `watch`
    pub id: String,
    /// The kind a rule is for
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    /// Why, as a code the UI words: `found`, `arr_category`, `new_folder`,
    /// `smallest_size`, `software_off`, `arr_imports`, `library`, `video`, `detected`
    pub reason: String,
    /// Words the reason names, such as `folder`
    pub params: BTreeMap<String, Value>,
    /// The editable value: a preset name, MB (0 is off), a bool, languages or a path
    pub value: Value,
    /// Switched off on the review screen
    pub off: bool,
    /// What it writes (empty when off)
    pub settings: Vec<Setting>,
}

/// The kinds found in the top-level Put.io folders, and in which folders
pub type Found = BTreeMap<Kind, Vec<String>>;

/// Matches top-level Put.io folder names to kinds, case-insensitively
pub fn detect(folders: &[String]) -> Found {
    let mut found = Found::new();
    for folder in folders {
        let name = folder.trim().to_lowercase();
        if let Some(kind) = Kind::ALL
            .iter()
            .find(|k| k.names().contains(&name.as_str()))
        {
            found
                .entry(*kind)
                .or_default()
                .push(folder.trim().to_string());
        }
    }
    found
}

/// The folders each ticked kind gets a rule on, and why
fn folders(answers: &Answers, found: &Found) -> Vec<(Kind, String, &'static str)> {
    let mut out = Vec::new();
    for kind in &answers.kinds {
        let Some(default) = kind.default_folder() else {
            continue;
        };
        let arr = kind.arr().filter(|a| answers.arr.contains(a));
        let category = arr.map(ArrKind::as_str);
        let mut by_name = false;
        for folder in found.get(kind).into_iter().flatten() {
            let is_category = Some(folder.to_lowercase().as_str()) == category;
            if is_category {
                continue;
            }
            by_name = true;
            out.push((*kind, folder.clone(), "found"));
        }
        if let Some(category) = category {
            let name = found
                .get(kind)
                .into_iter()
                .flatten()
                .find(|f| f.to_lowercase() == category)
                .cloned()
                .unwrap_or_else(|| category.to_string());
            out.push((*kind, name, "arr_category"));
        }
        if !by_name && (category.is_none() || answers.by_hand) {
            out.push((*kind, default.to_string(), "new_folder"));
        }
    }
    out
}

fn setting(key: Key, value: Value) -> Setting {
    Setting {
        key: key.to_string(),
        value,
    }
}

/// The settings a recommendation writes for `value`
fn settings_for(id: &str, value: &Value, ctx: &Ctx) -> CoreResult<Vec<Setting>> {
    let bad = || CoreError::ConfigError(format!("{id}: unexpected value {value}"));
    if let Some(folder) = id.strip_prefix("rule:") {
        let preset = value.as_str().ok_or_else(bad)?;
        return Ok(vec![setting(
            Key::RulePreset(folder.to_string()),
            Value::from(preset),
        )]);
    }
    Ok(match id {
        "precheck_watch" => {
            let path = ctx.watch.clone().ok_or_else(bad)?;
            match value.as_u64().ok_or_else(bad)? {
                0 => vec![
                    setting(Key::PrecheckWatchEnabled(path.clone()), Value::Bool(false)),
                    setting(Key::PrecheckWatchMin(path), Value::Null),
                ],
                mb => vec![
                    setting(Key::PrecheckWatchEnabled(path.clone()), Value::Null),
                    setting(Key::PrecheckWatchMin(path), Value::from(mb)),
                ],
            }
        }
        "delete" => {
            let on = value.as_bool().ok_or_else(bad)?;
            vec![
                setting(Key::DeleteAfterDownload, Value::Bool(on)),
                setting(Key::CleanupEnabled, Value::Bool(on)),
            ]
        }
        "subtitles" => {
            let given: Vec<String> = serde_json::from_value(value.clone()).map_err(|_| bad())?;
            let languages: Vec<String> = given
                .into_iter()
                .map(|l| crate::lang::lookup(&l).unwrap_or(l))
                .collect();
            vec![
                setting(Key::SubtitlesLanguages, serde_json::json!(languages)),
                setting(Key::SubtitlesMode, Value::from("download_only")),
                setting(Key::SubtitlesExcluded, serde_json::json!(ctx.excluded)),
            ]
        }
        "watch" => {
            let path = value
                .as_str()
                .filter(|p| !p.trim().is_empty())
                .ok_or_else(bad)?;
            vec![
                setting(Key::WatchEnabled, Value::Bool(true)),
                setting(Key::WatchPaths, serde_json::json!([path.trim()])),
            ]
        }
        _ => {
            return Err(CoreError::ConfigError(format!(
                "unknown recommendation {id}"
            )))
        }
    })
}

/// A recommendation before overrides: id, kind, reason, params, value
type Draft<'a> = (
    String,
    Option<Kind>,
    &'a str,
    BTreeMap<String, Value>,
    Value,
);

/// What [`settings_for`] needs beyond the value
struct Ctx {
    watch: Option<String>,
    excluded: Vec<String>,
}

/// The recommendations for `answers`, with the review screen's `overrides` (by id)
pub fn plan(
    answers: &Answers,
    found: &Found,
    overrides: &BTreeMap<String, Override>,
) -> CoreResult<Vec<Recommendation>> {
    answers.validate()?;
    let rules = folders(answers, found);
    let watch_override = overrides.get("watch");
    let watch_value = watch_override
        .and_then(|o| o.value.as_ref())
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| answers.watch.as_ref().map(|p| p.display().to_string()));
    let watch_on = watch_value.is_some() && !watch_override.is_some_and(|o| o.off);
    let ctx = Ctx {
        watch: watch_value.clone(),
        excluded: rules
            .iter()
            .filter(|(k, _, _)| matches!(k, Kind::Music | Kind::Books | Kind::Software))
            .map(|(_, f, _)| f.clone())
            .collect(),
    };

    let mut out: Vec<Draft> = Vec::new();
    for (kind, folder, reason) in &rules {
        if let Some(preset) = kind.preset(folder) {
            let mut params = BTreeMap::new();
            params.insert("folder".to_string(), Value::from(folder.clone()));
            out.push((
                format!("rule:{folder}"),
                Some(*kind),
                reason,
                params,
                Value::from(preset),
            ));
        }
    }
    if let Some(path) = &watch_value {
        let mut params = BTreeMap::new();
        params.insert("folder".to_string(), Value::from(path.clone()));
        out.push((
            "watch".to_string(),
            None,
            "detected",
            params,
            Value::from(path.clone()),
        ));
        // The smallest size of the ticked kinds; software turns the check off
        let sizes: Vec<Size> = answers.kinds.iter().map(|k| k.size()).collect();
        let (mb, reason) = if sizes.contains(&Size::Off) {
            (Some(0), "software_off")
        } else {
            let min = sizes
                .iter()
                .filter_map(|s| match s {
                    Size::Mb(n) => Some(*n),
                    _ => None,
                })
                .min();
            (min, "smallest_size")
        };
        if let Some(mb) = mb {
            if watch_on {
                let mut params = BTreeMap::new();
                params.insert("folder".to_string(), Value::from(path.clone()));
                out.push((
                    "precheck_watch".to_string(),
                    None,
                    reason,
                    params,
                    Value::from(mb),
                ));
            }
        }
    }
    let imports = !answers.arr.is_empty();
    out.push((
        "delete".to_string(),
        None,
        if imports { "arr_imports" } else { "library" },
        BTreeMap::new(),
        Value::Bool(imports),
    ));
    if answers.kinds.iter().any(|k| k.is_video()) {
        let mut languages = Vec::new();
        if let Some(code) = crate::lang::lookup(&answers.language) {
            languages.push(code);
        }
        if !languages.iter().any(|l| l == "en") {
            languages.push("en".to_string());
        }
        out.push((
            "subtitles".to_string(),
            None,
            "video",
            BTreeMap::new(),
            serde_json::json!(languages),
        ));
    }

    out.into_iter()
        .map(|(id, kind, reason, params, value)| {
            let o = overrides.get(&id);
            let off = o.is_some_and(|o| o.off);
            let value = o.and_then(|o| o.value.clone()).unwrap_or(value);
            let settings = if off {
                Vec::new()
            } else {
                settings_for(&id, &value, &ctx)?
            };
            Ok(Recommendation {
                id,
                kind,
                reason: reason.to_string(),
                params,
                value,
                off,
                settings,
            })
        })
        .collect()
}

/// Notes for the review screen: `dlna` for video, `tailscale` when its socket was found
pub fn notes(answers: &Answers, tailscale: bool) -> Vec<&'static str> {
    let mut notes = Vec::new();
    if answers.kinds.iter().any(|k| k.is_video()) {
        notes.push("dlna");
    }
    if tailscale {
        notes.push("tailscale");
    }
    notes
}

/// One setting the profile writes, as a typed path into [`Config`]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// `download.rules[<folder>].preset`
    RulePreset(String),
    /// `precheck.watch[<path>].min_size_mb`
    PrecheckWatchMin(String),
    /// `precheck.watch[<path>].enabled`
    PrecheckWatchEnabled(String),
    /// `download.delete_after_download`
    DeleteAfterDownload,
    /// `cleanup.enabled`
    CleanupEnabled,
    /// `subtitles.languages`
    SubtitlesLanguages,
    /// `subtitles.mode`
    SubtitlesMode,
    /// `subtitles.excluded_categories`
    SubtitlesExcluded,
    /// `watch.enabled`
    WatchEnabled,
    /// `watch.paths`
    WatchPaths,
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::RulePreset(p) => write!(f, "download.rules[{p}].preset"),
            Key::PrecheckWatchMin(p) => write!(f, "precheck.watch[{p}].min_size_mb"),
            Key::PrecheckWatchEnabled(p) => write!(f, "precheck.watch[{p}].enabled"),
            Key::DeleteAfterDownload => f.write_str("download.delete_after_download"),
            Key::CleanupEnabled => f.write_str("cleanup.enabled"),
            Key::SubtitlesLanguages => f.write_str("subtitles.languages"),
            Key::SubtitlesMode => f.write_str("subtitles.mode"),
            Key::SubtitlesExcluded => f.write_str("subtitles.excluded_categories"),
            Key::WatchEnabled => f.write_str("watch.enabled"),
            Key::WatchPaths => f.write_str("watch.paths"),
        }
    }
}

impl FromStr for Key {
    type Err = CoreError;

    fn from_str(s: &str) -> CoreResult<Self> {
        let indexed = |prefix: &str, suffix: &str| {
            s.strip_prefix(prefix)
                .and_then(|r| r.strip_suffix(suffix))
                .filter(|inner| !inner.is_empty())
                .map(str::to_string)
        };
        if let Some(p) = indexed("download.rules[", "].preset") {
            return Ok(Key::RulePreset(p));
        }
        if let Some(p) = indexed("precheck.watch[", "].min_size_mb") {
            return Ok(Key::PrecheckWatchMin(p));
        }
        if let Some(p) = indexed("precheck.watch[", "].enabled") {
            return Ok(Key::PrecheckWatchEnabled(p));
        }
        Ok(match s {
            "download.delete_after_download" => Key::DeleteAfterDownload,
            "cleanup.enabled" => Key::CleanupEnabled,
            "subtitles.languages" => Key::SubtitlesLanguages,
            "subtitles.mode" => Key::SubtitlesMode,
            "subtitles.excluded_categories" => Key::SubtitlesExcluded,
            "watch.enabled" => Key::WatchEnabled,
            "watch.paths" => Key::WatchPaths,
            _ => return Err(CoreError::ConfigError(format!("unknown profile key {s}"))),
        })
    }
}

fn to_json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn from_json<T: serde::de::DeserializeOwned>(key: &Key, value: Value) -> CoreResult<T> {
    serde_json::from_value(value).map_err(|e| CoreError::ConfigError(format!("{key}: {e}")))
}

fn same_rule(rule: &FolderRule, path: &str) -> bool {
    normalize_path(&rule.path).ok().as_deref() == Some(path)
}

impl Key {
    /// The config section and key [`crate::restart::needs_restart`] takes
    fn section(&self) -> (&'static str, &'static str) {
        match self {
            Key::RulePreset(_) => ("download", "rules"),
            Key::PrecheckWatchMin(_) | Key::PrecheckWatchEnabled(_) => ("precheck", "watch"),
            Key::DeleteAfterDownload => ("download", "delete_after_download"),
            Key::CleanupEnabled => ("cleanup", "enabled"),
            Key::SubtitlesLanguages => ("subtitles", "languages"),
            Key::SubtitlesMode => ("subtitles", "mode"),
            Key::SubtitlesExcluded => ("subtitles", "excluded_categories"),
            Key::WatchEnabled => ("watch", "enabled"),
            Key::WatchPaths => ("watch", "paths"),
        }
    }

    /// True when a change to this key waits for a restart
    pub fn needs_restart(&self) -> bool {
        let (section, key) = self.section();
        crate::restart::needs_restart(section, key)
    }

    /// The current value; `null` when unset
    pub fn get(&self, config: &Config) -> Value {
        match self {
            Key::RulePreset(folder) => {
                let path = normalize_path(folder).unwrap_or_default();
                config
                    .download
                    .rules
                    .iter()
                    .find(|r| same_rule(r, &path))
                    .and_then(|r| r.preset.clone())
                    .map(Value::from)
                    .unwrap_or(Value::Null)
            }
            Key::PrecheckWatchMin(p) => {
                to_json(&config.precheck.watch.get(p).and_then(|o| o.min_size_mb))
            }
            Key::PrecheckWatchEnabled(p) => {
                to_json(&config.precheck.watch.get(p).and_then(|o| o.enabled))
            }
            Key::DeleteAfterDownload => Value::Bool(config.download.delete_after_download),
            Key::CleanupEnabled => Value::Bool(config.cleanup.enabled),
            Key::SubtitlesLanguages => to_json(&config.subtitles.languages),
            Key::SubtitlesMode => to_json(&config.subtitles.mode),
            Key::SubtitlesExcluded => to_json(&config.subtitles.excluded_categories),
            Key::WatchEnabled => Value::Bool(config.watch.enabled),
            Key::WatchPaths => to_json(&config.watch.paths),
        }
    }

    /// Writes `value`; `null` puts the default back
    pub fn set(&self, config: &mut Config, value: Value) -> CoreResult<()> {
        let defaults = Config::default();
        let null = value.is_null();
        match self {
            Key::RulePreset(folder) => {
                let path = normalize_path(folder)?;
                let rules = &mut config.download.rules;
                if null {
                    rules.retain_mut(|r| {
                        if !same_rule(r, &path) {
                            return true;
                        }
                        r.preset = None;
                        !r.is_empty()
                    });
                } else {
                    let preset: String = from_json(self, value)?;
                    match rules.iter_mut().find(|r| same_rule(r, &path)) {
                        Some(rule) => {
                            rule.preset = Some(preset);
                            rule.skip = None;
                        }
                        None => rules.push(FolderRule {
                            path,
                            preset: Some(preset),
                            ..FolderRule::default()
                        }),
                    }
                }
            }
            Key::PrecheckWatchMin(p) | Key::PrecheckWatchEnabled(p) => {
                let entry = config.precheck.watch.entry(p.clone()).or_default();
                if let Key::PrecheckWatchMin(_) = self {
                    entry.min_size_mb = if null { None } else { from_json(self, value)? };
                } else {
                    entry.enabled = if null { None } else { from_json(self, value)? };
                }
                if *entry == PrecheckOverride::default() {
                    config.precheck.watch.remove(p);
                }
            }
            Key::DeleteAfterDownload => {
                config.download.delete_after_download = if null {
                    defaults.download.delete_after_download
                } else {
                    from_json(self, value)?
                }
            }
            Key::CleanupEnabled => {
                config.cleanup.enabled = if null {
                    defaults.cleanup.enabled
                } else {
                    from_json(self, value)?
                }
            }
            Key::SubtitlesLanguages => {
                config.subtitles.languages = if null {
                    defaults.subtitles.languages
                } else {
                    from_json(self, value)?
                }
            }
            Key::SubtitlesMode => {
                config.subtitles.mode = if null { None } else { from_json(self, value)? }
            }
            Key::SubtitlesExcluded => {
                config.subtitles.excluded_categories = if null {
                    defaults.subtitles.excluded_categories
                } else {
                    from_json(self, value)?
                }
            }
            Key::WatchEnabled => {
                config.watch.enabled = if null {
                    defaults.watch.enabled
                } else {
                    from_json(self, value)?
                }
            }
            Key::WatchPaths => {
                config.watch.paths = if null {
                    defaults.watch.paths
                } else {
                    from_json(self, value)?
                }
            }
        }
        Ok(())
    }
}

/// `[profile]`: the saved answers and the values the profile set
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(default)]
pub struct ProfileConfig {
    /// What is downloaded
    pub kinds: Vec<Kind>,
    /// The *arr apps that send downloads
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub arr: Vec<ArrKind>,
    /// Files are also added by hand on Put.io
    pub by_hand: bool,
    /// The watch folder
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watch: Option<PathBuf>,
    /// The UI language when the profile ran
    #[serde(skip_serializing_if = "String::is_empty")]
    pub language: String,
    /// Kind (`movies`, ...) to the Put.io folders found for it, so Settings can run the
    /// profile again without asking Put.io
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub folders: BTreeMap<String, Vec<String>>,
    /// Setting key to the value the profile last wrote
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub applied: BTreeMap<String, Value>,
}

impl ProfileConfig {
    /// The saved answers
    pub fn answers(&self) -> Answers {
        Answers {
            kinds: self.kinds.clone(),
            arr: self.arr.clone(),
            by_hand: self.by_hand,
            watch: self.watch.clone(),
            language: self.language.clone(),
        }
    }

    /// The saved folders as [`Found`]
    pub fn found(&self) -> Found {
        let mut found = Found::new();
        for kind in Kind::ALL {
            if let Some(folders) = self.folders.get(kind.as_str()) {
                found.insert(kind, folders.clone());
            }
        }
        found
    }
}

/// One setting a run of the profile changes
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Change {
    /// The key
    pub key: String,
    /// Its value now
    pub before: Value,
    /// Its value after
    pub after: Value,
    /// Waits for a restart
    pub restart: bool,
}

/// A setting changed by hand that the profile leaves alone
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Kept {
    /// The key
    pub key: String,
    /// Its value now (kept)
    pub current: Value,
    /// What the profile would have set
    pub profile: Value,
}

/// What a run of the profile does
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Diff {
    /// Settings it changes
    pub changes: Vec<Change>,
    /// Settings changed by hand that it keeps
    pub kept: Vec<Kept>,
}

/// Applies `recs` to a copy of `config`. A setting whose value differs from what the
/// profile last wrote (or, never written, from the default) was changed by hand and is
/// kept. Settings the profile wrote before and no longer recommends go back to the
/// default when still unchanged. Returns the new config, with `[profile]` set from
/// `answers` and `found`, and what changed.
pub fn rerun(
    config: &Config,
    answers: &Answers,
    found: &Found,
    recs: &[Recommendation],
) -> CoreResult<(Config, Diff)> {
    let defaults = Config::default();
    let previous = config.profile.clone().unwrap_or_default();
    let mut next = config.clone();
    let mut diff = Diff::default();
    let mut applied = BTreeMap::new();
    let mut wanted: Vec<String> = Vec::new();

    for setting in recs.iter().flat_map(|r| &r.settings) {
        let key: Key = setting.key.parse()?;
        wanted.push(setting.key.clone());
        let current = key.get(config);
        let baseline = previous
            .applied
            .get(&setting.key)
            .cloned()
            .unwrap_or_else(|| key.get(&defaults));
        let target = if setting.value.is_null() {
            key.get(&defaults)
        } else {
            setting.value.clone()
        };
        if current != baseline && current != target {
            diff.kept.push(Kept {
                key: setting.key.clone(),
                current,
                profile: target,
            });
            if let Some(v) = previous.applied.get(&setting.key) {
                applied.insert(setting.key.clone(), v.clone());
            }
            continue;
        }
        key.set(&mut next, setting.value.clone())?;
        let after = key.get(&next);
        if after != current {
            diff.changes.push(Change {
                key: setting.key.clone(),
                before: current,
                after: after.clone(),
                restart: key.needs_restart(),
            });
        }
        if !after.is_null() {
            applied.insert(setting.key.clone(), after);
        }
    }
    for (name, value) in &previous.applied {
        if wanted.contains(name) {
            continue;
        }
        let Ok(key) = name.parse::<Key>() else {
            continue;
        };
        let current = key.get(config);
        if &current != value {
            continue;
        }
        key.set(&mut next, Value::Null)?;
        let after = key.get(&next);
        if after != current {
            diff.changes.push(Change {
                key: name.clone(),
                before: current,
                after,
                restart: key.needs_restart(),
            });
        }
    }

    next.profile = Some(ProfileConfig {
        kinds: answers.kinds.clone(),
        arr: answers.arr.clone(),
        by_hand: answers.by_hand,
        watch: answers.watch.clone(),
        language: answers.language.clone(),
        folders: found
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), v.clone()))
            .collect(),
        applied,
    });
    Ok((next, diff))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn answers(kinds: &[Kind]) -> Answers {
        Answers {
            kinds: kinds.to_vec(),
            language: "en".to_string(),
            by_hand: true,
            ..Answers::default()
        }
    }

    fn rec<'a>(recs: &'a [Recommendation], id: &str) -> Option<&'a Recommendation> {
        recs.iter().find(|r| r.id == id)
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn detect_matches_names_case_insensitively() {
        let found = detect(&names(&[
            "Movies",
            "TV Shows",
            "music",
            "Audiobooks",
            "Stuff",
        ]));
        assert_eq!(found[&Kind::Movies], vec!["Movies"]);
        assert_eq!(found[&Kind::Tv], vec!["TV Shows"]);
        assert_eq!(found[&Kind::Music], vec!["music"]);
        assert_eq!(found[&Kind::Books], vec!["Audiobooks"]);
        assert!(!found.contains_key(&Kind::Other));
    }

    #[test]
    fn rules_go_on_found_folders_or_new_ones() {
        let found = detect(&names(&["Films", "Audio Books"]));
        let recs = plan(
            &answers(&[Kind::Movies, Kind::Books, Kind::Music, Kind::Software]),
            &found,
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(rec(&recs, "rule:Films").unwrap().value, json!("video"));
        assert_eq!(rec(&recs, "rule:Films").unwrap().reason, "found");
        // An audiobook folder gets the audio preset
        assert_eq!(
            rec(&recs, "rule:Audio Books").unwrap().value,
            json!("audio")
        );
        assert_eq!(rec(&recs, "rule:Music").unwrap().reason, "new_folder");
        // Software lifts the blocked extensions
        assert_eq!(
            rec(&recs, "rule:Software").unwrap().value,
            json!("everything")
        );
    }

    #[test]
    fn arr_apps_add_their_category_folder() {
        let mut a = answers(&[Kind::Movies, Kind::Tv]);
        a.arr = vec![ArrKind::Radarr, ArrKind::Sonarr];
        a.by_hand = false;
        let recs = plan(&a, &Found::new(), &BTreeMap::new()).unwrap();
        assert_eq!(rec(&recs, "rule:radarr").unwrap().reason, "arr_category");
        assert!(rec(&recs, "rule:sonarr").is_some());
        // Not added by hand: no Movies folder made up
        assert!(rec(&recs, "rule:Movies").is_none());
        let delete = rec(&recs, "delete").unwrap();
        assert_eq!(delete.value, json!(true));
        assert_eq!(delete.reason, "arr_imports");
        assert!(delete
            .settings
            .iter()
            .any(|s| s.key == "cleanup.enabled" && s.value == json!(true)));
    }

    #[test]
    fn delete_is_off_without_arr() {
        let recs = plan(&answers(&[Kind::Music]), &Found::new(), &BTreeMap::new()).unwrap();
        let delete = rec(&recs, "delete").unwrap();
        assert_eq!(delete.value, json!(false));
        assert_eq!(delete.reason, "library");
        // No video: no subtitles
        assert!(rec(&recs, "subtitles").is_none());
    }

    #[test]
    fn watch_folder_uses_the_smallest_size() {
        let mut a = answers(&[Kind::Movies, Kind::Music, Kind::Books]);
        a.watch = Some(PathBuf::from("/watches"));
        let recs = plan(&a, &Found::new(), &BTreeMap::new()).unwrap();
        let pc = rec(&recs, "precheck_watch").unwrap();
        assert_eq!(pc.value, json!(1));
        assert_eq!(pc.reason, "smallest_size");
        assert!(rec(&recs, "watch").is_some());

        a.kinds.push(Kind::Software);
        let recs = plan(&a, &Found::new(), &BTreeMap::new()).unwrap();
        let pc = rec(&recs, "precheck_watch").unwrap();
        assert_eq!(pc.value, json!(0));
        assert_eq!(pc.reason, "software_off");
        assert!(pc
            .settings
            .iter()
            .any(|s| s.key == "precheck.watch[/watches].enabled" && s.value == json!(false)));

        // Video only keeps the 50 MB default
        a.kinds = vec![Kind::Movies];
        let recs = plan(&a, &Found::new(), &BTreeMap::new()).unwrap();
        assert!(rec(&recs, "precheck_watch").is_none());
    }

    #[test]
    fn subtitles_want_the_ui_language_and_english() {
        let mut a = answers(&[Kind::Tv, Kind::Music]);
        a.language = "de".to_string();
        let recs = plan(&a, &Found::new(), &BTreeMap::new()).unwrap();
        let subs = rec(&recs, "subtitles").unwrap();
        assert_eq!(subs.value, json!(["de", "en"]));
        let excluded = subs
            .settings
            .iter()
            .find(|s| s.key == "subtitles.excluded_categories")
            .unwrap();
        assert_eq!(excluded.value, json!(["Music"]));
    }

    #[test]
    fn overrides_switch_off_and_change_values() {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            "delete".to_string(),
            Override {
                off: true,
                value: None,
            },
        );
        overrides.insert(
            "rule:Movies".to_string(),
            Override {
                off: false,
                value: Some(json!("everything")),
            },
        );
        let recs = plan(&answers(&[Kind::Movies]), &Found::new(), &overrides).unwrap();
        let delete = rec(&recs, "delete").unwrap();
        assert!(delete.off && delete.settings.is_empty());
        assert_eq!(
            rec(&recs, "rule:Movies").unwrap().settings[0].value,
            json!("everything")
        );
    }

    #[test]
    fn no_kinds_is_an_error() {
        assert!(plan(&answers(&[]), &Found::new(), &BTreeMap::new()).is_err());
    }

    #[test]
    fn keys_round_trip() {
        let keys = [
            Key::RulePreset("TV Shows".to_string()),
            Key::PrecheckWatchMin("/watches".to_string()),
            Key::PrecheckWatchEnabled("/watches".to_string()),
            Key::DeleteAfterDownload,
            Key::CleanupEnabled,
            Key::SubtitlesLanguages,
            Key::SubtitlesMode,
            Key::SubtitlesExcluded,
            Key::WatchEnabled,
            Key::WatchPaths,
        ];
        let values = [
            json!("video"),
            json!(5),
            json!(false),
            json!(true),
            json!(true),
            json!(["de", "en"]),
            json!("download_only"),
            json!(["Music"]),
            json!(true),
            json!(["/watches"]),
        ];
        for (key, value) in keys.iter().zip(values) {
            assert_eq!(&key.to_string().parse::<Key>().unwrap(), key);
            let mut config = Config::default();
            key.set(&mut config, value.clone()).unwrap();
            assert_eq!(key.get(&config), value, "{key}");
            key.set(&mut config, Value::Null).unwrap();
            assert_eq!(key.get(&config), key.get(&Config::default()), "{key}");
        }
        assert!("download.rules[].preset".parse::<Key>().is_err());
        assert!(Key::PrecheckWatchMin("/w".to_string()).needs_restart());
        assert!(!Key::WatchPaths.needs_restart());
    }

    #[test]
    fn rerun_writes_profile_and_keeps_hand_changes() {
        let mut a = answers(&[Kind::Movies, Kind::Music]);
        a.watch = Some(PathBuf::from("/watches"));
        let found = detect(&names(&["Movies"]));
        let recs = plan(&a, &found, &BTreeMap::new()).unwrap();
        let (config, diff) = rerun(&Config::default(), &a, &found, &recs).unwrap();
        assert!(diff.kept.is_empty());
        assert!(diff
            .changes
            .iter()
            .any(|c| c.key == "download.rules[Movies].preset"));
        assert!(diff
            .changes
            .iter()
            .any(|c| c.key == "precheck.watch[/watches].min_size_mb" && c.restart));
        assert_eq!(config.precheck.watch["/watches"].min_size_mb, Some(5));
        let profile = config.profile.clone().unwrap();
        assert_eq!(profile.kinds, a.kinds);
        assert_eq!(profile.found(), found);
        assert_eq!(
            profile.applied["download.rules[Movies].preset"],
            json!("video")
        );

        // The user changes the Movies preset by hand, then drops music from the profile
        let mut edited = config.clone();
        Key::RulePreset("Movies".to_string())
            .set(&mut edited, json!("everything"))
            .unwrap();
        let mut b = a.clone();
        b.kinds = vec![Kind::Movies];
        let recs = plan(&b, &found, &BTreeMap::new()).unwrap();
        let (again, diff) = rerun(&edited, &b, &found, &recs).unwrap();
        assert_eq!(diff.kept.len(), 1);
        assert_eq!(diff.kept[0].key, "download.rules[Movies].preset");
        assert_eq!(
            Key::RulePreset("Movies".to_string()).get(&again),
            json!("everything")
        );
        // Music's rule and the watch size go back to the defaults
        assert_eq!(
            Key::RulePreset("Music".to_string()).get(&again),
            Value::Null
        );
        assert!(!again.precheck.watch.contains_key("/watches"));
    }

    #[test]
    fn profile_round_trips_through_toml() {
        let a = answers(&[Kind::Tv]);
        let recs = plan(&a, &Found::new(), &BTreeMap::new()).unwrap();
        let (config, _) = rerun(&Config::default(), &a, &Found::new(), &recs).unwrap();
        let text = toml::to_string(&config).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.profile, config.profile);
        // Without a profile nothing is written
        let text = toml::to_string(&Config::default()).unwrap();
        assert!(!text.contains("[profile]"));
    }
}
