//! Configuration management for putio-sync
//!
//! This module handles loading, saving, and managing configuration files
//! in TOML format.

use crate::error::{CoreError, CoreResult};
use crate::path_utils::expand_path;
use crate::rules::{FolderRule, Preset};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::listen::{BindList, ListenAddr};
use crate::logging::LevelSource;
use putio_sync_telemetry::server::MetricsSettings;
use putio_sync_telemetry::Detail;

/// Default value for delete_empty_folders
fn default_slow_limit() -> u64 {
    crate::bandwidth::DEFAULT_SLOW_LIMIT
}

fn default_delete_empty_folders() -> bool {
    true
}

fn default_true() -> bool {
    true
}

fn default_verify_max_redownloads() -> u32 {
    DownloadConfig::DEFAULT_VERIFY_MAX_REDOWNLOADS
}

fn default_verify_read_limit() -> u64 {
    DownloadConfig::DEFAULT_VERIFY_READ_LIMIT_MB
}

/// What to do when a file already exists at its local path (`[download] on_existing`,
/// feature 040)
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExistingFile {
    /// Skip complete files, continue partial ones
    #[default]
    Resume,
    /// Download again, replacing the old file once the new one is complete
    Overwrite,
    /// Keep the old file and save the new one as `name (1).ext`
    #[serde(alias = "rename")]
    KeepBoth,
}

impl ExistingFile {
    /// The config value
    pub fn as_str(self) -> &'static str {
        match self {
            ExistingFile::Resume => "resume",
            ExistingFile::Overwrite => "overwrite",
            ExistingFile::KeepBoth => "keep_both",
        }
    }
}

/// Whether accepted files are downloaded or kept on Put.io for streaming (`[download]
/// mode` and `[[download.rules]] mode`, feature 022)
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    /// Download to `target_directory`
    #[default]
    Download,
    /// Keep on Put.io; the media server's Put.io root shows it
    Stream,
}

impl SyncMode {
    /// The config value
    pub fn as_str(self) -> &'static str {
        match self {
            SyncMode::Download => "download",
            SyncMode::Stream => "stream",
        }
    }
}

impl From<bool> for ExistingFile {
    /// The old `skip_existing`: `true` is resume, `false` overwrite
    fn from(skip_existing: bool) -> Self {
        if skip_existing {
            ExistingFile::Resume
        } else {
            ExistingFile::Overwrite
        }
    }
}

impl std::fmt::Display for ExistingFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What happened to a file that already existed locally (feature 040)
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExistingOutcome {
    /// Complete already; nothing downloaded
    Skipped,
    /// The partial file was continued
    Resumed,
    /// A resumed file failed its checksum and was downloaded again
    Restarted,
    /// Replaced by a new download
    Overwritten,
    /// Kept, with the new download under a numbered name
    SavedAs,
    /// A folder is in the way; nothing downloaded
    Refused,
}

impl ExistingOutcome {
    /// Every outcome
    pub const ALL: [ExistingOutcome; 6] = [
        ExistingOutcome::Skipped,
        ExistingOutcome::Resumed,
        ExistingOutcome::Restarted,
        ExistingOutcome::Overwritten,
        ExistingOutcome::SavedAs,
        ExistingOutcome::Refused,
    ];

    /// The stored and logged name
    pub fn as_str(self) -> &'static str {
        match self {
            ExistingOutcome::Skipped => "skipped",
            ExistingOutcome::Resumed => "resumed",
            ExistingOutcome::Restarted => "restarted",
            ExistingOutcome::Overwritten => "overwritten",
            ExistingOutcome::SavedAs => "saved_as",
            ExistingOutcome::Refused => "refused",
        }
    }
}

impl std::str::FromStr for ExistingOutcome {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ExistingOutcome::ALL
            .into_iter()
            .find(|o| o.as_str() == s)
            .ok_or(())
    }
}

/// Extensions that are never downloaded unless `blocked_extensions` overrides them.
/// Fake releases on the NAS were mostly `.exe` files (133 of 244 downloads).
pub const DEFAULT_BLOCKED_EXTENSIONS: &[&str] = &[
    "exe", "msi", "bat", "cmd", "com", "scr", "pif", "vbs", "js", "jar", "ps1", "lnk", "apk",
    "dmg", "pkg",
];

/// Default value for blocked_extensions
pub fn default_blocked_extensions() -> Vec<String> {
    DEFAULT_BLOCKED_EXTENSIONS
        .iter()
        .map(|e| e.to_string())
        .collect()
}

/// Main configuration structure
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Config {
    /// Authentication configuration
    pub auth: Option<AuthConfig>,
    /// Put.io app settings (`[putio]`, feature 013)
    #[serde(default, skip_serializing_if = "PutioConfig::is_empty")]
    pub putio: PutioConfig,
    /// Sync configuration
    #[serde(default)]
    pub sync: SyncConfig,
    /// Download configuration
    #[serde(default)]
    pub download: DownloadConfig,
    /// Watch configuration
    #[serde(default)]
    pub watch: WatchConfig,
    /// Notification configuration
    #[serde(default)]
    pub notifications: NotificationConfig,
    /// API/Daemon mode configuration
    #[serde(default)]
    pub api: ApiConfig,
    /// Logging configuration
    #[serde(default)]
    pub logging: crate::logging::LoggingConfig,
    /// Prometheus metrics configuration
    #[serde(default)]
    pub metrics: MetricsConfig,
    /// Put.io empty-folder cleanup
    #[serde(default)]
    pub cleanup: CleanupConfig,
    /// Put.io transfer monitoring (stall detection)
    #[serde(default)]
    pub transfers: TransfersConfig,
    /// Processing pipeline: what runs after a download (feature 038)
    #[serde(default)]
    pub pipeline: PipelineConfig,
    /// Subtitles: wanted languages and optional transcription (feature 025)
    #[serde(default)]
    pub subtitles: crate::subtitles::SubtitlesConfig,
    /// `[translation]` (feature 026)
    #[serde(default)]
    pub translation: crate::translation::TranslationConfig,
    /// Radarr/Sonarr download client (Transmission RPC)
    #[serde(default)]
    pub arr: ArrConfig,
    /// Scheduled doctor runs (feature 010)
    #[serde(default)]
    pub doctor: DoctorConfig,
    /// Release pre-check (feature 042)
    #[serde(default)]
    pub precheck: PrecheckConfig,
    /// Put.io storage levels and the automatic free-up run (feature 033)
    #[serde(default)]
    pub storage: StorageConfig,
    /// Tailscale: tailnet-only listening and identity sign-in (feature 036)
    #[serde(default)]
    pub tailscale: TailscaleConfig,
    /// Password and OAuth sign-in (feature 019)
    #[serde(
        default,
        skip_serializing_if = "crate::sign_in::SignInConfig::is_default"
    )]
    pub sign_in: crate::sign_in::SignInConfig,
    /// DLNA media server: show downloads and a Put.io folder to players (feature 022)
    #[serde(default)]
    pub dlna: DlnaConfig,
    /// Update checks against GitHub Releases (feature 055)
    #[serde(default, skip_serializing_if = "UpdatesConfig::is_default")]
    pub updates: UpdatesConfig,
    /// Organising files stream mode keeps on Put.io (feature 023)
    #[serde(
        default,
        skip_serializing_if = "crate::organise::OrganiseConfig::is_default"
    )]
    pub organise: crate::organise::OrganiseConfig,
    /// Event hooks: commands and webhooks run for events (feature 027)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<crate::hooks::HookConfig>,
    /// Folder of Lua plugins; default `plugins` next to the config file (feature 028)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugins_dir: Option<PathBuf>,
    /// Lua plugin settings (feature 028)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<crate::plugins::PluginSettings>,
    /// The plugin workbench's editor may write plugin files (feature 055)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub plugin_editor: bool,
}

/// `[doctor]`: how often the daemon runs the doctor's checks (feature 010). The CLI and
/// the web UI can run them at any time, whatever this says.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct DoctorConfig {
    /// Run the checks on a schedule in the daemon
    pub enabled: bool,
    /// Minutes between scheduled runs; values below 5 count as 5
    pub interval_minutes: u64,
}

impl Default for DoctorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_minutes: 60,
        }
    }
}

impl DoctorConfig {
    /// Time between scheduled runs
    pub fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.interval_minutes.max(5) * 60)
    }
}

/// `[updates]`: look for a newer release on GitHub (feature 055). Left out of a saved
/// config while both keys are at their defaults, so older files round-trip unchanged.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct UpdatesConfig {
    /// Check GitHub Releases about a minute after start and then once a day
    pub check: bool,
    /// Install an update by itself once no transfer is running (GitHub archive installs only)
    pub auto_install: bool,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self {
            check: true,
            auto_install: false,
        }
    }
}

impl UpdatesConfig {
    /// Both keys at their defaults
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// The default magnet lookup cache (research R3)
pub const DEFAULT_PRECHECK_CACHE: &str = "https://itorrents.net/torrent/{HASH}.torrent";

/// `[precheck]`: read a release's file list before it reaches Put.io and reject it when
/// no file is a wanted type of at least the minimum size (feature 042)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct PrecheckConfig {
    /// Default for every *arr category and the watch folder
    pub enabled: bool,
    /// Check, log and count, but never reject or delete (Constitution IV)
    pub dry_run: bool,
    /// The largest wanted file must be at least this big
    pub min_size_mb: u64,
    /// Count archives (`.rar`, `.7z`, split parts) as wanted
    pub allow_archives: bool,
    /// Look magnets up in `caches` before adding them
    pub magnet_lookup: bool,
    /// Total time for all caches, 1..=30
    pub lookup_timeout_secs: u64,
    /// .torrent cache URLs with `{HASH}` (upper case) or `{hash}` (lower case)
    pub caches: Vec<String>,
    /// How long a magnet missed on the first ask waits for a cache to fetch it, 0..=3600;
    /// 0 sends it to Put.io at once (feature 043)
    pub hold_magnet_secs: u64,
    /// Time between cache asks for a held magnet, 10..=600
    pub hold_retry_secs: u64,
    /// The first ask's limit while holding is on, 1..=`lookup_timeout_secs`
    pub first_lookup_secs: u64,
    /// Per *arr category (the folder below the download dir)
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub categories: BTreeMap<String, PrecheckOverride>,
    /// Per `[watch].paths` entry
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub watch: BTreeMap<String, PrecheckOverride>,
}

/// Settings one category or watch folder changes; unset ones come from `[precheck]`
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct PrecheckOverride {
    /// Check this folder at all
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Its minimum size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_size_mb: Option<u64>,
    /// Its archive setting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_archives: Option<bool>,
}

impl Default for PrecheckConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dry_run: false,
            min_size_mb: 50,
            allow_archives: false,
            magnet_lookup: true,
            lookup_timeout_secs: 10,
            caches: vec![DEFAULT_PRECHECK_CACHE.to_string()],
            hold_magnet_secs: 300,
            hold_retry_secs: 30,
            first_lookup_secs: 3,
            categories: BTreeMap::new(),
            watch: BTreeMap::new(),
        }
    }
}

impl PrecheckConfig {
    /// A magnet missed on the first ask is held and asked again (feature 043)
    pub fn holding(&self) -> bool {
        self.enabled && self.magnet_lookup && self.hold_magnet_secs > 0
    }

    /// Check the ranges and cache URLs. A `watch` key that is not in `watch_paths` is
    /// only a warning, so removing a watch path doesn't break startup.
    pub fn validate(&self, watch_paths: &[PathBuf]) -> CoreResult<()> {
        let sizes = std::iter::once(("precheck.min_size_mb", self.min_size_mb)).chain(
            self.categories
                .values()
                .chain(self.watch.values())
                .filter_map(|o| o.min_size_mb)
                .map(|v| ("precheck override min_size_mb", v)),
        );
        for (setting, value) in sizes {
            if value > 100_000 {
                return Err(invalid(setting, &value.to_string(), "0..=100000"));
            }
        }
        if !(1..=30).contains(&self.lookup_timeout_secs) {
            return Err(invalid(
                "precheck.lookup_timeout_secs",
                &self.lookup_timeout_secs.to_string(),
                "1..=30",
            ));
        }
        if self.hold_magnet_secs > 3600 {
            return Err(invalid(
                "precheck.hold_magnet_secs",
                &self.hold_magnet_secs.to_string(),
                "0..=3600",
            ));
        }
        if !(10..=600).contains(&self.hold_retry_secs) {
            return Err(invalid(
                "precheck.hold_retry_secs",
                &self.hold_retry_secs.to_string(),
                "10..=600",
            ));
        }
        if !(1..=self.lookup_timeout_secs).contains(&self.first_lookup_secs) {
            return Err(invalid(
                "precheck.first_lookup_secs",
                &self.first_lookup_secs.to_string(),
                &format!("1..={} (lookup_timeout_secs)", self.lookup_timeout_secs),
            ));
        }
        for cache in &self.caches {
            let http = cache.starts_with("http://") || cache.starts_with("https://");
            if !http || !(cache.contains("{HASH}") || cache.contains("{hash}")) {
                return Err(invalid(
                    "precheck cache",
                    cache,
                    "an http(s):// URL containing {HASH} or {hash}",
                ));
            }
        }
        for key in self.watch.keys() {
            if !watch_paths.iter().any(|p| p == Path::new(key)) {
                tracing::warn!(path = %key, "[precheck.watch] names a path not in [watch].paths");
            }
        }
        Ok(())
    }
}

/// Default `[arr] username`
pub const DEFAULT_ARR_USERNAME: &str = "putio-sync";

/// `[arr]`: putio-sync answers Radarr's and Sonarr's Transmission client (feature 011).
/// The password and API keys may come from the environment instead, see
/// [`Config::resolve_arr`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct ArrConfig {
    /// Answer the Transmission RPC at `/transmission/rpc`
    pub enabled: bool,
    /// Username the *arr apps send
    pub username: String,
    /// Password the *arr apps send. `PUTIO_SYNC_ARR_PASSWORD` overrides it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// *arr apps whose API putio-sync calls to blocklist failed releases
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<ArrServer>,
}

impl Default for ArrConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            username: DEFAULT_ARR_USERNAME.to_string(),
            password: None,
            servers: Vec::new(),
        }
    }
}

impl std::fmt::Debug for ArrConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArrConfig")
            .field("enabled", &self.enabled)
            .field("username", &self.username)
            .field("password", &redacted(&self.password))
            .field("servers", &self.servers)
            .finish()
    }
}

/// Which *arr app a `[[arr.servers]]` entry is
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArrKind {
    /// Radarr (movies)
    Radarr,
    /// Sonarr (series)
    Sonarr,
}

impl ArrKind {
    /// `radarr` or `sonarr`
    pub fn as_str(self) -> &'static str {
        match self {
            ArrKind::Radarr => "radarr",
            ArrKind::Sonarr => "sonarr",
        }
    }

    /// The `appName` its `/api/v3/system/status` reports
    pub fn app_name(self) -> &'static str {
        match self {
            ArrKind::Radarr => "Radarr",
            ArrKind::Sonarr => "Sonarr",
        }
    }
}

/// `[[arr.servers]]`: one *arr app (an "application" in the UI)
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct ArrServer {
    /// `radarr` or `sonarr`
    pub kind: ArrKind,
    /// Name shown in the UI; [`Config::resolve_arr`] fills in "Radarr"/"Sonarr" (feature 018)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Base URL, e.g. `http://192.168.1.50:8989`
    pub url: String,
    /// API key. `PUTIO_SYNC_RADARR_API_KEY` / `PUTIO_SYNC_SONARR_API_KEY` override it
    /// for the first server of that kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Legacy (feature 011): the category typed for this server. The category is now read
    /// from the *arr; this one is only a fallback for blocklisting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

impl std::fmt::Debug for ArrServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArrServer")
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("url", &self.url)
            .field("api_key", &redacted(&self.api_key))
            .field("category", &self.category)
            .finish()
    }
}

/// `Debug` of a secret: `<set>` or `<unset>`
struct Redacted(bool);

impl std::fmt::Debug for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0 { "<set>" } else { "<unset>" })
    }
}

fn redacted(secret: &Option<String>) -> Redacted {
    Redacted(secret.is_some())
}

/// Normalise a category: no leading or trailing `/`. `None` when it has an empty or
/// `.`/`..` segment or a backslash.
pub fn normalize_arr_category(category: &str) -> Option<String> {
    let trimmed = category.trim_matches('/');
    if trimmed.is_empty() {
        return Some(String::new());
    }
    let ok = !trimmed.contains('\\')
        && trimmed
            .split('/')
            .all(|seg| !seg.is_empty() && seg != "." && seg != "..");
    ok.then(|| trimmed.to_string())
}

impl ArrConfig {
    /// Check the username and each server's URL, and that no URL is used twice. A legacy
    /// `category` isn't checked: it's ignored when it isn't a valid folder.
    pub fn validate(&self) -> CoreResult<()> {
        if self.username.is_empty() || self.username.contains(':') {
            return Err(CoreError::ConfigError(
                "invalid arr username: expected a non-empty value without ':'".to_string(),
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for server in &self.servers {
            let url = server.url.trim_end_matches('/');
            let rest = url
                .strip_prefix("http://")
                .or_else(|| url.strip_prefix("https://"));
            if rest.is_none_or(|r| r.is_empty() || r.chars().any(char::is_whitespace)) {
                return Err(invalid("arr server url", &server.url, "an http(s):// URL"));
            }
            if !seen.insert(url.to_ascii_lowercase()) {
                return Err(CoreError::ConfigError(format!(
                    "arr server url '{}' is used twice",
                    url
                )));
            }
        }
        Ok(())
    }
}

/// `[arr]` after env overrides and validation
#[derive(Clone, PartialEq, Eq)]
pub struct ArrSettings {
    /// Answer the Transmission RPC
    pub enabled: bool,
    /// Username the *arr apps send
    pub username: String,
    /// Password the *arr apps send; the RPC answers 503 without one
    pub password: Option<String>,
    /// Servers with normalised categories and URLs without a trailing `/`
    pub servers: Vec<ArrServer>,
}

impl std::fmt::Debug for ArrSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArrSettings")
            .field("enabled", &self.enabled)
            .field("username", &self.username)
            .field("password", &redacted(&self.password))
            .field("servers", &self.servers)
            .finish()
    }
}

impl ArrSettings {
    /// The server whose legacy typed category is `category` (normalised)
    pub fn server_for(&self, category: &str) -> Option<&ArrServer> {
        let category = normalize_arr_category(category)?;
        self.servers
            .iter()
            .find(|s| s.category.as_deref() == Some(category.as_str()))
    }

    /// `PUTIO_SYNC_ARR_PASSWORD` sets the password, so the UI can't change it
    pub fn password_from_env() -> bool {
        env_value(ARR_PASSWORD_VAR).is_some()
    }

    /// The API key of server `index` comes from `PUTIO_SYNC_RADARR_API_KEY` /
    /// `PUTIO_SYNC_SONARR_API_KEY` (the first server of its kind)
    pub fn api_key_from_env(servers: &[ArrServer], index: usize) -> bool {
        let Some(server) = servers.get(index) else {
            return false;
        };
        env_value(arr_key_var(server.kind)).is_some()
            && servers.iter().position(|s| s.kind == server.kind) == Some(index)
    }
}

const ARR_PASSWORD_VAR: &str = "PUTIO_SYNC_ARR_PASSWORD";

fn arr_key_var(kind: ArrKind) -> &'static str {
    match kind {
        ArrKind::Radarr => "PUTIO_SYNC_RADARR_API_KEY",
        ArrKind::Sonarr => "PUTIO_SYNC_SONARR_API_KEY",
    }
}

/// An environment variable; empty counts as unset
fn env_value(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// `[transfers]`: when a Put.io transfer counts as stalled (feature 007), and how long the
/// Active page keeps finished transfers (feature 039)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct TransfersConfig {
    /// Hours without new bytes before a downloading transfer is stalled
    pub stall_threshold_hours: u64,
    /// Seconds between checks of the Put.io transfer list
    pub stall_check_interval_secs: u64,
    /// Hours a finished transfer stays in the Active page's Recently finished list (1–720)
    pub finished_retention_hours: u64,
}

impl Default for TransfersConfig {
    fn default() -> Self {
        Self {
            stall_threshold_hours: 6,
            stall_check_interval_secs: 600,
            finished_retention_hours: 24,
        }
    }
}

/// `[pipeline]`: how the steps after a download run (feature 038)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct PipelineConfig {
    /// Items whose steps run at once (1–16)
    pub concurrency: usize,
    /// Seconds to wait before each retry of a failing optional step (at most 10 entries)
    pub optional_retries: Vec<u64>,
    /// Watch-folder staging: `auto` (on while a file-changing step is configured), `on`, `off`
    pub staging: StagingMode,
}

/// `[pipeline] staging`
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StagingMode {
    /// On while a pipeline hook exists
    #[default]
    Auto,
    /// Always stage watch-folder downloads
    On,
    /// Never stage
    Off,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            concurrency: 2,
            optional_retries: vec![60, 300, 600],
            staging: StagingMode::Auto,
        }
    }
}

/// Most entries in `[pipeline] optional_retries`
pub const MAX_OPTIONAL_RETRIES: usize = 10;
/// Most items whose steps run at once
pub const MAX_PIPELINE_CONCURRENCY: usize = 16;

impl PipelineConfig {
    /// Check `concurrency` and `optional_retries`
    pub fn validate(&self) -> CoreResult<()> {
        if !(1..=MAX_PIPELINE_CONCURRENCY).contains(&self.concurrency) {
            return Err(CoreError::ConfigError(format!(
                "pipeline concurrency must be from 1 to {}, got {}",
                MAX_PIPELINE_CONCURRENCY, self.concurrency
            )));
        }
        if self.optional_retries.len() > MAX_OPTIONAL_RETRIES {
            return Err(CoreError::ConfigError(format!(
                "pipeline optional_retries has at most {} entries, got {}",
                MAX_OPTIONAL_RETRIES,
                self.optional_retries.len()
            )));
        }
        Ok(())
    }
}

/// Largest `[transfers] finished_retention_hours` (30 days)
pub const MAX_FINISHED_RETENTION_HOURS: u64 = 720;

impl TransfersConfig {
    /// Check `finished_retention_hours`
    pub fn validate(&self) -> CoreResult<()> {
        if !(1..=MAX_FINISHED_RETENTION_HOURS).contains(&self.finished_retention_hours) {
            return Err(CoreError::ConfigError(format!(
                "transfers finished_retention_hours must be from 1 to {}, got {}",
                MAX_FINISHED_RETENTION_HOURS, self.finished_retention_hours
            )));
        }
        Ok(())
    }
}

/// Authentication method
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "PascalCase")]
pub enum AuthMethod {
    /// OAuth token authentication (Bearer token)
    #[default]
    Token,
    /// App-specific password authentication (Basic Auth)
    AppPassword,
}

/// `[putio]`: the Put.io app putio-sync identifies as
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq, Eq)]
pub struct PutioConfig {
    /// Public app ID for the device link (put.io/link). `PUTIO_SYNC_APP_ID`
    /// overrides it; it overrides the built-in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
}

impl PutioConfig {
    /// Nothing set (the section is left out when saving)
    pub fn is_empty(&self) -> bool {
        self.app_id.is_none()
    }

    /// Check that `app_id`, when set, is a Put.io app ID: digits only. Empty counts as
    /// unset, as it always has.
    pub fn validate(&self) -> CoreResult<()> {
        match self.app_id.as_deref() {
            Some(id) if !id.trim().is_empty() && !valid_app_id(id) => Err(CoreError::ConfigError(
                "putio app_id must be the number Put.io shows for the app, e.g. 1234".to_string(),
            )),
            _ => Ok(()),
        }
    }
}

/// A Put.io app ID: 1 to 12 digits, spaces around it allowed
pub fn valid_app_id(id: &str) -> bool {
    let id = id.trim();
    (1..=12).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

/// Authentication configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthConfig {
    /// Authentication method to use
    #[serde(default)]
    pub auth_method: AuthMethod,

    /// OAuth token (used when auth_method is Token)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// Put.io username/email (used when auth_method is AppPassword)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    /// App-specific password (used when auth_method is AppPassword)
    /// Create at: https://app.put.io/account/app-specific-passwords
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl AuthConfig {
    /// Create a new AuthConfig with OAuth token
    pub fn with_token(token: String) -> Self {
        Self {
            auth_method: AuthMethod::Token,
            token: Some(token),
            username: None,
            password: None,
        }
    }

    /// Create a new AuthConfig with app-specific password
    pub fn with_app_password(username: String, password: String) -> Self {
        Self {
            auth_method: AuthMethod::AppPassword,
            token: None,
            username: Some(username),
            password: Some(password),
        }
    }

    /// Validate the authentication configuration
    pub fn validate(&self) -> CoreResult<()> {
        match self.auth_method {
            AuthMethod::Token => {
                if self.token.is_none() || self.token.as_ref().unwrap().is_empty() {
                    return Err(CoreError::ConfigError(
                        "Token is required for Token authentication".to_string(),
                    ));
                }
            }
            AuthMethod::AppPassword => {
                if self.username.is_none() || self.username.as_ref().unwrap().is_empty() {
                    return Err(CoreError::ConfigError(
                        "Username is required for AppPassword authentication".to_string(),
                    ));
                }
                if self.password.is_none() || self.password.as_ref().unwrap().is_empty() {
                    return Err(CoreError::ConfigError(
                        "Password is required for AppPassword authentication".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Sync configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncConfig {
    /// Local path where files will be synced
    pub local_path: PathBuf,
    /// Remote parent folder ID (None or Some(0) for root)
    pub remote_parent_id: Option<i64>,
    /// Delete files from Put.io after successful download
    pub delete_after_download: bool,
}

/// Progress bar display style
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProgressStyle {
    /// One bar per file + overall progress bar
    #[default]
    PerFile,
    /// Overall progress only
    Overall,
    /// Individual file progress only
    Individual,
    /// Rotate between files and overall
    Rotating,
    /// No progress bars
    None,
}

/// Sort order for downloads
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SortOrder {
    /// Sort by date (default)
    #[default]
    ByDate,
    /// Sort by filename
    ByName,
    /// Sort by file extension
    ByExtension,
    /// Sort by file size
    BySize,
}

/// Download configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DownloadConfig {
    /// Number of concurrent download workers (default: 2)
    pub workers: usize,
    /// Target directory for downloads
    pub target_directory: PathBuf,
    /// Maximum number of concurrent downloads (alias for workers, for backwards compatibility)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrent: Option<usize>,
    /// Buffer size in MB for streaming downloads (default: 16)
    pub buffer_size_mb: usize,
    /// Chunk size in MB for chunked downloads (None to disable - Put.io doesn't support range requests)
    pub chunk_size_mb: Option<usize>,
    /// Continue interrupted and partial downloads instead of starting over
    pub resume_enabled: bool,
    /// What to do when a file already exists locally; absent, `skip_existing` decides
    /// (see [`DownloadConfig::on_existing`])
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_existing: Option<ExistingFile>,
    /// Replaced by `on_existing` and still read: `true` is resume, `false` overwrite.
    /// Saves from the web UI drop it once `on_existing` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_existing: Option<bool>,
    /// Delete files from Put.io after successful download
    pub delete_after_download: bool,
    /// Download accepted files, or keep them on Put.io for streaming (feature 022); folder
    /// rules can override it
    #[serde(default)]
    pub mode: SyncMode,
    /// Ask Put.io for an MP4 version of videos kept on Put.io that aren't MP4
    #[serde(default = "default_true")]
    pub stream_mp4: bool,
    /// Progress bar display style
    #[serde(default)]
    pub progress_style: ProgressStyle,
    /// Sort order for downloads
    #[serde(default)]
    pub sort_order: SortOrder,
    /// Reverse the sort order
    pub reverse_sort: bool,
    /// Glob patterns to whitelist files (e.g., "*.mkv")
    pub whitelist_patterns: Option<String>,
    /// Glob patterns to blacklist files (e.g., "*.exe")
    pub blacklist_patterns: Option<String>,
    /// Bandwidth limit in bytes per second (None for unlimited)
    /// Example: Some(1048576) for 1 MB/s limit
    pub bandwidth_limit_bytes_per_sec: Option<u64>,
    /// Limits by time of day and day of week (`[[download.bandwidth_schedule]]`, feature
    /// 032); the first matching entry wins, `bandwidth_limit_bytes_per_sec` applies outside
    /// them. Times are the daemon's local time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bandwidth_schedule: Vec<crate::bandwidth::ScheduleEntry>,
    /// The limit while "Slow" is on, or the schedule's if lower (default 1 MiB/s)
    #[serde(default = "default_slow_limit")]
    pub slow_limit_bytes_per_sec: u64,
    /// Check each download against Put.io's CRC32 (or its size) before it is saved under
    /// its real name (feature 034)
    #[serde(default = "default_true")]
    pub verify: bool,
    /// Downloads again after a mismatch before the file fails, 0..=5
    #[serde(default = "default_verify_max_redownloads")]
    pub verify_max_redownloads: u32,
    /// How fast "verify everything" reads files from disk, in MB/s, 1..=10000
    #[serde(default = "default_verify_read_limit")]
    pub verify_read_limit_mb_per_sec: u64,
    /// "Verify everything" waits while the bandwidth schedule or an override pauses
    #[serde(default = "default_true")]
    pub verify_follow_bandwidth_pause: bool,
    /// Maximum number of retry attempts for failed downloads (default: 3)
    pub max_retries: u32,
    /// Initial retry delay in seconds (doubles with each retry)
    pub retry_initial_delay_secs: u64,
    /// Delete empty folders after downloading all files from them (default: true)
    #[serde(default = "default_delete_empty_folders")]
    pub delete_empty_folders: bool,
    /// File extensions (without the dot, case-insensitive) that are never downloaded.
    /// A missing key uses the default list; `[]` disables the guard.
    #[serde(default = "default_blocked_extensions")]
    pub blocked_extensions: Vec<String>,
    /// Account-wide preset (feature 013): the root filter instead of `blocked_extensions`.
    /// Folder rules still override it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// Per-folder rules (`[[download.rules]]`); folders below a rule inherit it
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<FolderRule>,
    /// User presets and built-in overrides (`[download.presets.<name>]`)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub presets: BTreeMap<String, Preset>,
}

/// Watch configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WatchConfig {
    /// Enable watch mode
    #[serde(default)]
    pub enabled: bool,
    /// Paths to watch for changes
    pub paths: Vec<PathBuf>,
    /// How often to refresh in seconds (used when file system events aren't available)
    pub refresh_cycle: i8,
    /// Parent folder ID for uploaded torrents (None or 0 for root)
    #[serde(default)]
    pub parent_folder_id: Option<i64>,
    /// Archive directory for processed torrents (moves files here after upload)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_directory: Option<PathBuf>,
    /// Delete torrent files after successful upload
    #[serde(default)]
    pub delete_after_upload: bool,
}

/// Notification backend type
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum NotificationBackend {
    /// No notifications
    #[default]
    None,
    /// Webhook notifications (HTTP POST)
    Webhook,
    /// Email notifications (future)
    Email,
}

/// Notification configuration. Every field is optional, so `[notifications.smtp]` alone is
/// enough.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct NotificationConfig {
    /// Enable notifications
    pub enabled: bool,
    /// Notification backend to use
    pub backend: NotificationBackend,
    /// Webhook URL (used when backend is Webhook)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
    /// Email address (used when backend is Email, future)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// The email channel (feature 030), separate from the legacy fields above
    #[serde(default, skip_serializing_if = "crate::email::SmtpConfig::is_default")]
    pub smtp: crate::email::SmtpConfig,
}

/// API/Daemon mode configuration
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct ApiConfig {
    /// Enable daemon to communicate with API server for downloads
    /// When true, the daemon will send download requests to an API server
    /// instead of downloading files directly
    pub enabled: bool,
    /// API server URL (e.g., "http://localhost:15432")
    /// Required when enabled is true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// API authentication token for securing REST API endpoints
    /// When set, all API requests must include this token in the Authorization header
    /// Format: "Bearer <token>"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    /// `daemon` opens the API and web UI listener. A bool or `true`/`false`/`1`/`0`,
    /// checked by [`Config::resolve_api`]. Default `true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listen: Option<Flag>,
    /// Listener addresses for `daemon` and `server`: one entry or a list of `ip:port`,
    /// `[ipv6]:port`, `interface:port` or `tailnet:port` (feature 037). Default
    /// `0.0.0.0:15432`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind: Option<BindList>,
}

/// A boolean setting written either as a TOML bool or as a string, so a bad string
/// gets the same error from the file as from the environment.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum Flag {
    /// `listen = true`
    Bool(bool),
    /// `listen = "1"`
    Text(String),
}

/// Default `[api] bind`
pub const DEFAULT_API_BIND: &str = "0.0.0.0:15432";

/// Default `[metrics] bind`
pub const DEFAULT_METRICS_BIND: &str = "0.0.0.0:9742";

/// Port used when nothing else gives one
pub const DEFAULT_API_PORT: u16 = 15432;

/// `[api]` listener settings after env overrides and validation
#[derive(Clone, PartialEq, Eq)]
pub struct ApiSettings {
    /// Open the listener (`daemon` only)
    pub listen: bool,
    /// Listener addresses, before tailnet-only (see [`Config::api_listen`])
    pub bind: Vec<ListenAddr>,
    /// Bearer token every protected route requires
    pub token: Option<String>,
}

impl ApiSettings {
    /// The first entry's port: the one other programs are told about
    pub fn port(&self) -> u16 {
        self.bind.first().map_or(DEFAULT_API_PORT, ListenAddr::port)
    }
}

/// `[metrics]` after env overrides: the server settings and where it listens
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsListen {
    /// Enabled, path, detail
    pub settings: MetricsSettings,
    /// Listener addresses
    pub bind: Vec<ListenAddr>,
}

impl std::fmt::Debug for ApiSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiSettings")
            .field("listen", &self.listen)
            .field(
                "bind",
                &self.bind.iter().map(|b| b.to_string()).collect::<Vec<_>>(),
            )
            .field(
                "token",
                &format_args!(
                    "{}",
                    if self.token.is_some() {
                        "<set>"
                    } else {
                        "<unset>"
                    }
                ),
            )
            .finish()
    }
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            local_path: PathBuf::from("./putio-downloads"),
            remote_parent_id: Some(0), // Root folder
            delete_after_download: false,
        }
    }
}

impl Default for DownloadConfig {
    fn default() -> Self {
        DownloadConfig {
            workers: 2,
            target_directory: PathBuf::from("./putio-downloads"),
            max_concurrent: None,
            buffer_size_mb: 16,
            chunk_size_mb: None, // Disabled - Put.io doesn't support range requests
            resume_enabled: true,
            on_existing: None,
            skip_existing: None,
            delete_after_download: false,
            mode: SyncMode::Download,
            stream_mp4: true,
            progress_style: ProgressStyle::PerFile,
            sort_order: SortOrder::ByDate,
            reverse_sort: false,
            whitelist_patterns: None,
            blacklist_patterns: None,
            bandwidth_limit_bytes_per_sec: None, // Unlimited by default
            bandwidth_schedule: Vec::new(),
            slow_limit_bytes_per_sec: default_slow_limit(),
            verify: true,
            verify_max_redownloads: Self::DEFAULT_VERIFY_MAX_REDOWNLOADS,
            verify_read_limit_mb_per_sec: Self::DEFAULT_VERIFY_READ_LIMIT_MB,
            verify_follow_bandwidth_pause: true,
            max_retries: 3,              // 3 retries by default
            retry_initial_delay_secs: 1, // Start with 1 second delay
            delete_empty_folders: true,  // Delete empty folders by default
            blocked_extensions: default_blocked_extensions(),
            preset: None,
            rules: Vec::new(),
            presets: BTreeMap::new(),
        }
    }
}

impl DownloadConfig {
    /// Default `verify_max_redownloads`
    pub const DEFAULT_VERIFY_MAX_REDOWNLOADS: u32 = 2;
    /// Largest `verify_max_redownloads`
    pub const MAX_VERIFY_REDOWNLOADS: u32 = 5;
    /// Default `verify_read_limit_mb_per_sec`
    pub const DEFAULT_VERIFY_READ_LIMIT_MB: u64 = 50;
    /// Largest `verify_read_limit_mb_per_sec`
    pub const MAX_VERIFY_READ_LIMIT_MB: u64 = 10_000;

    /// The `verify_*` keys are in range (feature 034)
    pub fn validate_verify(&self) -> Result<(), String> {
        if self.verify_max_redownloads > Self::MAX_VERIFY_REDOWNLOADS {
            return Err(format!(
                "download.verify_max_redownloads must be 0..={} (got {})",
                Self::MAX_VERIFY_REDOWNLOADS,
                self.verify_max_redownloads
            ));
        }
        if !(1..=Self::MAX_VERIFY_READ_LIMIT_MB).contains(&self.verify_read_limit_mb_per_sec) {
            return Err(format!(
                "download.verify_read_limit_mb_per_sec must be 1..={} (got {})",
                Self::MAX_VERIFY_READ_LIMIT_MB,
                self.verify_read_limit_mb_per_sec
            ));
        }
        Ok(())
    }

    /// The verify run's read limit in bytes per second (MB = 1 000 000 bytes)
    pub fn verify_read_limit_bytes(&self) -> u64 {
        self.verify_read_limit_mb_per_sec
            .clamp(1, Self::MAX_VERIFY_READ_LIMIT_MB)
            * 1_000_000
    }

    /// The global choice for existing files: `on_existing`, else the old
    /// `skip_existing`, else resume
    pub fn on_existing(&self) -> ExistingFile {
        self.on_existing
            .unwrap_or_else(|| self.skip_existing.unwrap_or(true).into())
    }

    /// Set the global choice, dropping the old `skip_existing` so a save writes one key
    pub fn set_on_existing(&mut self, choice: ExistingFile) {
        self.on_existing = Some(choice);
        self.skip_existing = None;
    }

    /// Where both `on_existing` and the old `skip_existing` are set (the `[download]`
    /// table or a rule path); `on_existing` wins in each
    pub fn both_existing_keys(&self) -> Vec<String> {
        let mut found = Vec::new();
        if self.on_existing.is_some() && self.skip_existing.is_some() {
            found.push("download".to_string());
        }
        for rule in &self.rules {
            if rule.on_existing.is_some() && rule.skip_existing.is_some() {
                found.push(format!("download.rules {}", rule.path));
            }
        }
        found
    }
}

impl WatchConfig {
    /// Validate the watch configuration
    pub fn validate(&self) -> CoreResult<()> {
        // Cannot have both archive and delete enabled
        if self.archive_directory.is_some() && self.delete_after_upload {
            return Err(CoreError::ConfigError(
                "Cannot have both archive_directory and delete_after_upload enabled. Choose one or neither.".to_string(),
            ));
        }

        // If archive directory is set, validate it can be created
        if let Some(ref archive_dir) = self.archive_directory {
            if let Some(parent) = archive_dir.parent() {
                if !parent.exists() {
                    return Err(CoreError::ConfigError(format!(
                        "Archive directory parent does not exist: {}",
                        parent.display()
                    )));
                }
            }
        }

        Ok(())
    }
}

impl Default for WatchConfig {
    fn default() -> Self {
        WatchConfig {
            enabled: false,
            paths: vec![],
            refresh_cycle: 60,         // 60 seconds
            parent_folder_id: Some(0), // Root folder
            archive_directory: None,
            delete_after_upload: true,
        }
    }
}

impl Default for NotificationConfig {
    fn default() -> Self {
        NotificationConfig {
            enabled: false,
            backend: NotificationBackend::None,
            webhook_url: None,
            email: None,
            smtp: crate::email::SmtpConfig::default(),
        }
    }
}

impl Config {
    /// Returns the default configuration file path
    ///
    /// Default location: `~/.putiosync/config.toml`
    ///
    /// Can be overridden by:
    /// - Environment variable: `PUTIO_SYNC_CONFIG`
    /// - CLI flag: `--config <path>`
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_core::config::Config;
    ///
    /// let path = Config::default_path().unwrap();
    /// println!("Config path: {}", path.display());
    /// ```
    pub fn default_path() -> CoreResult<PathBuf> {
        // Check environment variable first
        if let Ok(env_path) = std::env::var("PUTIO_SYNC_CONFIG") {
            return Ok(PathBuf::from(env_path));
        }

        // Use home directory
        let home = dirs::home_dir().ok_or_else(|| {
            CoreError::ConfigError("Could not determine home directory".to_string())
        })?;

        Ok(home.join(".putiosync").join("config.toml"))
    }

    /// Load configuration from a file
    ///
    /// Creates the parent directory if it doesn't exist.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the configuration file
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_core::config::Config;
    /// use std::path::PathBuf;
    ///
    /// let config = Config::load(&PathBuf::from("config.toml")).unwrap();
    /// ```
    pub fn load<P: AsRef<Path>>(path: P) -> CoreResult<Self> {
        let path = path.as_ref();
        eprintln!("🔍 Config::load - checking path: {}", path.display());

        if !path.exists() {
            eprintln!("❌ Config file does not exist: {}", path.display());
            return Err(CoreError::config_not_found(path));
        }
        eprintln!("✅ Config file exists");

        eprintln!("📖 Reading config file contents...");
        let contents = fs::read_to_string(path).map_err(|e| {
            eprintln!("❌ Failed to read config file: {}", e);
            CoreError::ConfigError(format!("Failed to read config file: {}", e))
        })?;

        eprintln!("📝 Config file size: {} bytes", contents.len());
        eprintln!("🔧 Parsing TOML configuration...");
        let mut config: Config = toml::from_str(&contents).map_err(|e| {
            eprintln!("❌ Failed to parse TOML: {}", e);
            e
        })?;

        eprintln!("✅ TOML parsing successful");
        eprintln!("📁 Expanding paths in configuration...");
        // Automatically expand paths during loading
        config.expand_paths().map_err(|e| {
            eprintln!("❌ Path expansion failed: {}", e);
            e
        })?;

        for table in config.download.both_existing_keys() {
            tracing::warn!(
                table,
                "Both on_existing and skip_existing are set; on_existing wins"
            );
        }

        eprintln!("✅ Configuration loaded and processed successfully");
        Ok(config)
    }

    /// Save configuration to a file
    ///
    /// Creates the parent directory if it doesn't exist.
    /// Sets file permissions to 0600 (owner read/write only) on Unix systems.
    ///
    /// # Arguments
    ///
    /// * `path` - Path where the configuration should be saved
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_core::config::Config;
    /// use std::path::PathBuf;
    ///
    /// let config = Config::default();
    /// config.save(&PathBuf::from("config.toml")).unwrap();
    /// ```
    pub fn save<P: AsRef<Path>>(&self, path: P) -> CoreResult<()> {
        let path = path.as_ref();

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| {
                    CoreError::ConfigError(format!("Failed to create config directory: {}", e))
                })?;
            }
        }

        // Serialize config to TOML
        let contents = toml::to_string_pretty(self)
            .map_err(|e| CoreError::ConfigError(format!("Failed to serialize config: {}", e)))?;

        // Write to file
        fs::write(path, contents)
            .map_err(|e| CoreError::ConfigError(format!("Failed to write config file: {}", e)))?;

        // Set file permissions to 0600 on Unix systems
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let permissions = fs::Permissions::from_mode(0o600);
            fs::set_permissions(path, permissions).map_err(|e| {
                CoreError::ConfigError(format!("Failed to set config file permissions: {}", e))
            })?;
        }

        Ok(())
    }

    /// Save the settings to `path`, editing only the keys whose value changed. Comments,
    /// blank lines, ordering and keys this version doesn't know stay as they are.
    ///
    /// The credentials are never written from memory: `[auth]` and `api.auth_token` stay
    /// exactly as they are on disk, or absent when the file has none. Used by the settings
    /// routes. In memory, `api.auth_token` may hold the `PUTIO_SYNC_API_TOKEN` override,
    /// which must never be written, and `[auth]` may be older than a `login` run since
    /// startup.
    ///
    /// A key changed when its value differs from what the file holds once loaded (paths
    /// expanded, defaults filled in), so an unchanged `~/...` path keeps its `~`. A key
    /// the config no longer writes (an option set back to none) is removed.
    ///
    /// A file that can't be read or parsed is left alone; the error names no file content,
    /// so no secret leaks into it. The write goes to a temp file renamed over the
    /// original, which keeps its mode (a new file is 0600).
    pub fn save_keeping_file_credentials<P: AsRef<Path>>(&self, path: P) -> CoreResult<()> {
        let path = path.as_ref();
        let unreadable = |what: &str| {
            CoreError::ConfigError(format!(
                "Failed to {what} {}; settings not saved",
                path.display()
            ))
        };
        let after = settings_table(self)?;

        if !path.exists() {
            let contents = toml::to_string_pretty(&after).map_err(|e| {
                CoreError::ConfigError(format!("Failed to serialize config: {}", e))
            })?;
            return write_atomically(path, &contents, None);
        }

        // Write through a symlink to the file it points at
        let path = fs::canonicalize(path).map_err(|_| unreadable("read"))?;
        let contents = fs::read_to_string(&path).map_err(|_| unreadable("read"))?;
        let mut document: toml_edit::DocumentMut =
            contents.parse().map_err(|_| unreadable("parse"))?;
        let mut on_disk: Config = toml::from_str(&contents).map_err(|_| unreadable("parse"))?;
        on_disk.expand_paths().map_err(|_| unreadable("parse"))?;
        let before = settings_table(&on_disk)?;

        edit_table(document.as_table_mut(), &before, &after);
        let permissions = fs::metadata(&path).ok().map(|m| m.permissions());
        write_atomically(&path, &document.to_string(), permissions)
    }

    /// Validate the configuration
    ///
    /// Checks that all required fields are present and valid.
    ///
    /// # Examples
    ///
    /// ```
    /// use putio_sync_core::config::Config;
    ///
    /// let config = Config::default();
    /// assert!(config.validate().is_ok());
    /// ```
    pub fn expand_paths(&mut self) -> CoreResult<()> {
        // Expand download target directory
        self.download.target_directory = expand_path(&self.download.target_directory)?;

        // Expand sync local path
        self.sync.local_path = expand_path(&self.sync.local_path)?;

        // Expand watch paths
        let mut expanded_watch_paths = Vec::new();
        for path in &self.watch.paths {
            expanded_watch_paths.push(expand_path(path)?);
        }
        self.watch.paths = expanded_watch_paths;

        // The daemon moves uploaded watch files here (`~/processed` in the README)
        if let Some(archive) = &self.watch.archive_directory {
            self.watch.archive_directory = Some(expand_path(archive)?);
        }

        Ok(())
    }

    /// Validate the configuration
    ///
    /// This validates the logical consistency of the configuration
    /// and directory paths
    ///
    /// # Example
    ///
    /// ```no_run
    pub fn validate(&self) -> CoreResult<()> {
        crate::bandwidth::validate(&self.download).map_err(CoreError::ConfigError)?;
        self.download
            .validate_verify()
            .map_err(CoreError::ConfigError)?;

        // Validate workers is at least 1
        if self.download.workers == 0 {
            return Err(CoreError::ConfigError(
                "workers must be at least 1".to_string(),
            ));
        }

        // Validate buffer_size_mb is at least 1
        if self.download.buffer_size_mb == 0 {
            return Err(CoreError::ConfigError(
                "buffer_size_mb must be at least 1".to_string(),
            ));
        }

        // Validate chunk_size_mb is reasonable if set
        if let Some(chunk_size) = self.download.chunk_size_mb {
            if chunk_size == 0 {
                return Err(CoreError::ConfigError(
                    "chunk_size_mb must be greater than 0 if set".to_string(),
                ));
            }
            if chunk_size > 10240 {
                // 10GB max chunk size
                return Err(CoreError::ConfigError(
                    "chunk_size_mb must not exceed 10240 (10GB)".to_string(),
                ));
            }
        }

        // Validate refresh_cycle is positive
        if self.watch.refresh_cycle <= 0 {
            return Err(CoreError::ConfigError(
                "refresh_cycle must be greater than 0".to_string(),
            ));
        }

        // Validate watch paths exist if watch is enabled
        if self.watch.enabled {
            for path in &self.watch.paths {
                if !path.exists() {
                    return Err(CoreError::ConfigError(format!(
                        "Watch path does not exist: {}",
                        path.display()
                    )));
                }
                if !path.is_dir() {
                    return Err(CoreError::ConfigError(format!(
                        "Watch path is not a directory: {}",
                        path.display()
                    )));
                }
            }
        }

        // Validate watch parent_folder_id is non-negative if set
        if let Some(parent_id) = self.watch.parent_folder_id {
            if parent_id < 0 {
                return Err(CoreError::ConfigError(
                    "watch parent_folder_id must be non-negative".to_string(),
                ));
            }
        }

        // Validate remote_parent_id is non-negative if set
        if let Some(parent_id) = self.sync.remote_parent_id {
            if parent_id < 0 {
                return Err(CoreError::ConfigError(
                    "remote_parent_id must be non-negative".to_string(),
                ));
            }
        }

        crate::rules::validate(&self.download.rules, &self.download.presets)?;
        crate::rules::validate_root_patterns(&self.download)?;
        crate::rules::validate_root_preset(
            self.download.preset.as_deref(),
            &self.download.presets,
        )?;
        self.cleanup.validate()?;
        self.arr.validate()?;
        self.transfers.validate()?;
        self.pipeline.validate()?;
        self.subtitles.validate()?;
        self.translation.validate()?;
        self.notifications.smtp.validate()?;
        self.precheck.validate(&self.watch.paths)?;
        self.storage.validate()?;
        self.tailscale.validate()?;
        self.sign_in.validate()?;
        self.dlna.validate()?;
        self.validate_dlna_with(&self.dlna)?;
        self.organise.validate()?;
        self.putio.validate()?;
        crate::hooks::validate(&self.hooks)?;
        crate::plugins::validate(&self.plugins)?;
        self.validate_binds()?;

        Ok(())
    }

    /// Create a new config builder
    ///
    /// # Examples
    ///
    /// ```
    /// use putio_sync_core::config::Config;
    /// use std::path::PathBuf;
    ///
    /// let config = Config::builder()
    ///     .local_path(PathBuf::from("/downloads"))
    ///     .max_concurrent(5)
    ///     .build();
    /// ```
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }
}

/// Builder for creating Config instances
#[derive(Default)]
pub struct ConfigBuilder {
    auth: Option<AuthConfig>,
    sync: SyncConfig,
    download: DownloadConfig,
    watch: WatchConfig,
}

impl ConfigBuilder {
    /// Set the authentication token (OAuth)
    pub fn token(mut self, token: String) -> Self {
        self.auth = Some(AuthConfig::with_token(token));
        self
    }

    /// Set authentication with username and app-specific password
    pub fn credentials(mut self, username: String, password: String) -> Self {
        self.auth = Some(AuthConfig::with_app_password(username, password));
        self
    }

    /// Set the authentication method
    pub fn auth_method(mut self, method: AuthMethod) -> Self {
        if let Some(auth) = &mut self.auth {
            auth.auth_method = method;
        }
        self
    }

    /// Set username (for AppPassword authentication)
    pub fn username(mut self, username: String) -> Self {
        if let Some(auth) = &mut self.auth {
            auth.username = Some(username);
        } else {
            self.auth = Some(AuthConfig {
                auth_method: AuthMethod::AppPassword,
                token: None,
                username: Some(username),
                password: None,
            });
        }
        self
    }

    /// Set password (for AppPassword authentication)
    pub fn password(mut self, password: String) -> Self {
        if let Some(auth) = &mut self.auth {
            auth.password = Some(password);
        } else {
            self.auth = Some(AuthConfig {
                auth_method: AuthMethod::AppPassword,
                token: None,
                username: None,
                password: Some(password),
            });
        }
        self
    }

    /// Set the local sync path
    pub fn local_path(mut self, path: PathBuf) -> Self {
        self.sync.local_path = path;
        self
    }

    /// Set the remote parent folder ID
    pub fn remote_parent_id(mut self, id: i64) -> Self {
        self.sync.remote_parent_id = Some(id);
        self
    }

    /// Set whether to delete files after download
    pub fn delete_after_download(mut self, delete: bool) -> Self {
        self.sync.delete_after_download = delete;
        self
    }

    /// Set the number of concurrent download workers
    pub fn workers(mut self, workers: usize) -> Self {
        self.download.workers = workers;
        self
    }

    /// Set the download target directory
    pub fn target_directory(mut self, path: PathBuf) -> Self {
        self.download.target_directory = path;
        self
    }

    /// Set the buffer size in MB for streaming downloads
    pub fn buffer_size_mb(mut self, size: usize) -> Self {
        self.download.buffer_size_mb = size;
        self
    }

    /// Set whether to skip existing downloads (the old key: `true` is resume)
    pub fn skip_existing(mut self, skip: bool) -> Self {
        self.download.skip_existing = Some(skip);
        self
    }

    /// Set what to do when a file already exists locally
    pub fn on_existing(mut self, choice: ExistingFile) -> Self {
        self.download.set_on_existing(choice);
        self
    }

    /// Set whether to delete from Put.io after download
    pub fn delete_after_download_config(mut self, delete: bool) -> Self {
        self.download.delete_after_download = delete;
        self
    }

    /// Set the progress display style
    pub fn progress_style(mut self, style: ProgressStyle) -> Self {
        self.download.progress_style = style;
        self
    }

    /// Set the sort order for downloads
    pub fn sort_order(mut self, order: SortOrder) -> Self {
        self.download.sort_order = order;
        self
    }

    /// Set whether to reverse the sort order
    pub fn reverse_sort(mut self, reverse: bool) -> Self {
        self.download.reverse_sort = reverse;
        self
    }

    /// Set the maximum number of concurrent downloads (deprecated, use workers instead)
    #[deprecated(since = "0.2.0", note = "Use `workers` instead")]
    pub fn max_concurrent(mut self, max: usize) -> Self {
        self.download.workers = max;
        self.download.max_concurrent = Some(max);
        self
    }

    /// Set the chunk size in MB
    pub fn chunk_size_mb(mut self, size: usize) -> Self {
        self.download.chunk_size_mb = Some(size);
        self
    }

    /// Enable or disable resume support
    pub fn resume_enabled(mut self, enabled: bool) -> Self {
        self.download.resume_enabled = enabled;
        self
    }

    /// Set whitelist patterns
    pub fn whitelist_patterns(mut self, patterns: String) -> Self {
        self.download.whitelist_patterns = Some(patterns);
        self
    }

    /// Set blacklist patterns
    pub fn blacklist_patterns(mut self, patterns: String) -> Self {
        self.download.blacklist_patterns = Some(patterns);
        self
    }

    /// Set bandwidth limit in bytes per second
    pub fn bandwidth_limit_bytes_per_sec(mut self, limit: u64) -> Self {
        self.download.bandwidth_limit_bytes_per_sec = Some(limit);
        self
    }

    /// Set maximum number of retry attempts
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.download.max_retries = retries;
        self
    }

    /// Set initial retry delay in seconds
    pub fn retry_initial_delay_secs(mut self, delay: u64) -> Self {
        self.download.retry_initial_delay_secs = delay;
        self
    }

    /// Enable or disable watch mode
    pub fn watch_enabled(mut self, enabled: bool) -> Self {
        self.watch.enabled = enabled;
        self
    }

    /// Set the paths to watch
    pub fn watch_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.watch.paths = paths;
        self
    }

    /// Set the refresh cycle in seconds
    pub fn refresh_cycle(mut self, seconds: i8) -> Self {
        self.watch.refresh_cycle = seconds;
        self
    }

    /// Set the parent folder ID for watch mode uploads
    pub fn watch_parent_folder_id(mut self, id: i64) -> Self {
        self.watch.parent_folder_id = Some(id);
        self
    }

    /// Build the Config
    pub fn build(self) -> Config {
        Config {
            auth: self.auth,
            sync: self.sync,
            download: self.download,
            watch: self.watch,
            notifications: NotificationConfig::default(),
            api: ApiConfig::default(),
            logging: crate::logging::LoggingConfig::default(),
            metrics: MetricsConfig::default(),
            cleanup: CleanupConfig::default(),
            transfers: TransfersConfig::default(),
            pipeline: PipelineConfig::default(),
            subtitles: Default::default(),
            translation: Default::default(),
            arr: ArrConfig::default(),
            doctor: DoctorConfig::default(),
            putio: PutioConfig::default(),
            precheck: PrecheckConfig::default(),
            storage: StorageConfig::default(),
            tailscale: TailscaleConfig::default(),
            sign_in: crate::sign_in::SignInConfig::default(),
            dlna: DlnaConfig::default(),
            updates: UpdatesConfig::default(),
            organise: Default::default(),
            hooks: Vec::new(),
            plugins_dir: None,
            plugins: Vec::new(),
            plugin_editor: false,
        }
    }
}

/// `[metrics]` settings as written in the config file. Values are kept as written and
/// checked by [`Config::resolve_metrics`], so a bad value gets the same error from TOML
/// and from the environment.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct MetricsConfig {
    /// Start the metrics listener
    pub enabled: bool,
    /// Listener addresses, one entry or a list (forms as `[api] bind`)
    pub bind: BindList,
    /// Scrape path
    pub path: String,
    /// `basic` or `detailed`
    pub detail: String,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        let d = MetricsSettings::default();
        Self {
            enabled: d.enabled,
            bind: BindList::from(DEFAULT_METRICS_BIND),
            path: d.path,
            detail: d.detail.as_str().to_string(),
        }
    }
}

/// Largest `min_age_hours` accepted: one year
pub const MAX_CLEANUP_MIN_AGE_HOURS: u64 = 8760;

/// `[cleanup]`: removing empty folders from the Put.io account. The daemon runs it once
/// a day at `run_at` (local time) when `enabled`; manual runs ignore `enabled`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct CleanupConfig {
    /// Run the cleanup once a day in the daemon
    pub enabled: bool,
    /// Daily run time, 24h `HH:MM` local time
    pub run_at: String,
    /// Folders created less than this many hours ago are kept
    pub min_age_hours: u64,
    /// Also remove folders whose sub-folders are all empty, in the same run. Off: only
    /// folders with nothing in them go (their parents on a later run).
    pub nested: bool,
    /// Keep folders matching any of these patterns (feature 020 language, tested against
    /// the folder's name and path), and everything inside them (feature 021)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keep_patterns: Vec<String>,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            run_at: "03:00".to_string(),
            min_age_hours: 24,
            nested: true,
            keep_patterns: Vec::new(),
        }
    }
}

impl CleanupConfig {
    /// `run_at` as a time of day; an error names the bad value
    pub fn run_time(&self) -> CoreResult<chrono::NaiveTime> {
        parse_run_at(&self.run_at)
    }

    /// Check `run_at`, `min_age_hours` and `keep_patterns`
    pub fn validate(&self) -> CoreResult<()> {
        self.run_time()?;
        for p in &self.keep_patterns {
            crate::rules::check_pattern(Some(p), "[cleanup] keep_patterns")?;
        }
        if self.min_age_hours > MAX_CLEANUP_MIN_AGE_HOURS {
            return Err(CoreError::ConfigError(format!(
                "cleanup min_age_hours must not exceed {}",
                MAX_CLEANUP_MIN_AGE_HOURS
            )));
        }
        Ok(())
    }
}

/// Largest `target_free_percent` accepted
pub const MAX_STORAGE_TARGET_PERCENT: u8 = 90;

/// `[storage]`: Put.io storage levels (feature 033), as percent of the account size free.
/// Below `warn_free_percent` the UI and the doctor warn; a free-up run deletes old files
/// that are already downloaded until `target_free_percent` is free. The daemon runs one
/// daily at `run_at` (local time) when `auto_free` is on and free space is below the
/// warning level; manual runs ignore `auto_free`.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct StorageConfig {
    /// Warn when less than this percent of the account is free
    pub warn_free_percent: u8,
    /// A free-up run stops once this percent is free
    pub target_free_percent: u8,
    /// Free up space once a day in the daemon when below the warning level
    pub auto_free: bool,
    /// Daily run time, 24h `HH:MM` local time
    pub run_at: String,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            warn_free_percent: 10,
            target_free_percent: 20,
            auto_free: false,
            run_at: "02:30".to_string(),
        }
    }
}

impl StorageConfig {
    /// `run_at` as a time of day; an error names the bad value
    pub fn run_time(&self) -> CoreResult<chrono::NaiveTime> {
        parse_time("storage", &self.run_at)
    }

    /// Check `run_at` and that 1 <= warn < target <= 90
    pub fn validate(&self) -> CoreResult<()> {
        self.run_time()?;
        if self.warn_free_percent == 0 {
            return Err(CoreError::ConfigError(
                "storage warn_free_percent must be at least 1".to_string(),
            ));
        }
        if self.target_free_percent <= self.warn_free_percent {
            return Err(CoreError::ConfigError(
                "storage target_free_percent must be above warn_free_percent".to_string(),
            ));
        }
        if self.target_free_percent > MAX_STORAGE_TARGET_PERCENT {
            return Err(CoreError::ConfigError(format!(
                "storage target_free_percent must not exceed {}",
                MAX_STORAGE_TARGET_PERCENT
            )));
        }
        Ok(())
    }
}

/// Default `[tailscale] socket`: where tailscaled's LocalAPI listens on Linux
pub const DEFAULT_TAILSCALE_SOCKET: &str = "/var/run/tailscale/tailscaled.sock";

/// `[tailscale]` (feature 036). putio-sync only reads Tailscale's state; it never
/// changes the Tailscale setup.
///
/// - `tailnet_only`: the daemon's listener binds this node's tailnet IPv4 address with
///   `[api] bind`'s port instead of `[api] bind` (applies at the next start)
/// - `identity_sign_in`: a request without a valid token is let in when its Tailscale
///   user is in `allowed_users` (applies at once)
/// - `socket`: tailscaled's LocalAPI socket; `PUTIO_SYNC_TAILSCALE_SOCKET` wins
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct TailscaleConfig {
    /// Listen on the tailnet address only
    pub tailnet_only: bool,
    /// Sign in allowed Tailscale users without the token
    pub identity_sign_in: bool,
    /// Tailscale logins (e.g. `alice@example.com`) identity sign-in lets in
    pub allowed_users: Vec<String>,
    /// LocalAPI socket path; default [`DEFAULT_TAILSCALE_SOCKET`]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket: Option<String>,
}

impl TailscaleConfig {
    /// Check that each login is non-empty without whitespace, and `socket` is non-empty
    pub fn validate(&self) -> CoreResult<()> {
        for login in &self.allowed_users {
            if login.trim().is_empty() || login.trim().contains(char::is_whitespace) {
                return Err(CoreError::ConfigError(format!(
                    "invalid tailscale allowed_users entry '{}': expected a login without spaces, e.g. alice@example.com",
                    login
                )));
            }
        }
        if self.socket.as_deref().is_some_and(|s| s.trim().is_empty()) {
            return Err(CoreError::ConfigError(
                "tailscale socket must not be empty; leave it out for the default".to_string(),
            ));
        }
        Ok(())
    }

    /// The LocalAPI socket: `PUTIO_SYNC_TAILSCALE_SOCKET`, then `socket`, then the default
    pub fn socket_path(&self) -> PathBuf {
        env_value("PUTIO_SYNC_TAILSCALE_SOCKET")
            .or_else(|| self.socket.clone())
            .unwrap_or_else(|| DEFAULT_TAILSCALE_SOCKET.to_string())
            .into()
    }

    /// Whether `login` is allowed, ignoring case and surrounding spaces
    pub fn allows(&self, login: &str) -> bool {
        let login = login.trim();
        !login.is_empty()
            && self
                .allowed_users
                .iter()
                .any(|a| a.trim().eq_ignore_ascii_case(login))
    }
}

/// Default `[dlna] name`
pub const DEFAULT_DLNA_NAME: &str = "putio-sync";

/// Default `[dlna] port`
pub const DEFAULT_DLNA_PORT: u16 = 8200;

/// Longest friendly name players get (characters)
pub const DLNA_NAME_MAX: usize = 64;

/// The UDP port UPnP discovery uses; the media server's HTTP port must not be it
pub const SSDP_PORT: u16 = 1900;

fn default_dlna_name() -> String {
    DEFAULT_DLNA_NAME.to_string()
}

fn default_dlna_port() -> u16 {
    DEFAULT_DLNA_PORT
}

fn default_dlna_roots() -> Vec<String> {
    vec!["downloads".to_string()]
}

fn empty_bind() -> BindList {
    BindList::Many(Vec::new())
}

/// `[dlna]`: the media server that shows downloaded files, and optionally one Put.io
/// folder, to TVs and players on the home network (feature 022).
///
/// - `roots`: `downloads`, `cat:<category>` (a subfolder of the download folder),
///   `watch:<path>` (one of `[watch] paths`) or `putio`
/// - `bind`: the addresses to serve on, in the `[api] bind` syntax; only the host part
///   of an entry is used, the port is `port`. Empty is every LAN IPv4 address.
/// - `putio_folder`: the Put.io folder shown as "Put.io"; unset, the synced folder (the
///   account root), which is where stream mode keeps files (feature 022)
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct DlnaConfig {
    /// Serve media to players
    pub enabled: bool,
    /// The name players show
    #[serde(default = "default_dlna_name")]
    pub name: String,
    /// HTTP port for descriptions, browsing and streams
    #[serde(default = "default_dlna_port")]
    pub port: u16,
    /// Where to listen; empty is every LAN IPv4 address
    #[serde(default = "empty_bind")]
    pub bind: BindList,
    /// What players see
    #[serde(default = "default_dlna_roots")]
    pub roots: Vec<String>,
    /// The Put.io folder id shown as the "Put.io" root; unset, the synced folder
    #[serde(skip_serializing_if = "Option::is_none")]
    pub putio_folder: Option<i64>,
}

impl Default for DlnaConfig {
    fn default() -> Self {
        DlnaConfig {
            enabled: false,
            name: default_dlna_name(),
            port: default_dlna_port(),
            bind: empty_bind(),
            roots: default_dlna_roots(),
            putio_folder: None,
        }
    }
}

impl DlnaConfig {
    /// The name players show: trimmed, `putio-sync` when empty, cut to 64 characters
    pub fn effective_name(&self) -> String {
        let name: String = self.name.trim().chars().take(DLNA_NAME_MAX).collect();
        let name = name.trim_end().to_string();
        if name.is_empty() {
            default_dlna_name()
        } else {
            name
        }
    }

    /// Whether `roots` has the Put.io root
    pub fn shares_putio(&self) -> bool {
        self.roots.iter().any(|r| r == "putio")
    }

    /// The Put.io folder shown as "Put.io": `putio_folder`, else the synced folder (`0`,
    /// the account root the sync mirrors)
    pub fn putio_root(&self) -> i64 {
        self.putio_folder.unwrap_or(0)
    }

    /// Whether players can see files kept on Put.io: on, and sharing the Put.io root
    pub fn shows_putio(&self) -> bool {
        self.enabled && self.shares_putio()
    }

    /// The `bind` entries, parsed; empty when `bind` is empty (every LAN address)
    pub fn bind_entries(&self) -> CoreResult<Vec<ListenAddr>> {
        if self.bind.entries().is_empty() {
            return Ok(Vec::new());
        }
        self.bind.parse("dlna.bind").map_err(CoreError::ConfigError)
    }

    /// Check every rule that does not depend on the rest of the config
    pub fn validate(&self) -> CoreResult<()> {
        let bad = |msg: String| Err(CoreError::ConfigError(msg));
        if self.port == 0 {
            return bad("invalid dlna.port: expected 1 to 65535".to_string());
        }
        if self.port == SSDP_PORT {
            return bad(format!(
                "invalid dlna.port {SSDP_PORT}: that port is used for discovery; pick another"
            ));
        }
        self.bind_entries()?;
        for (i, root) in self.roots.iter().enumerate() {
            if self.roots[..i].contains(root) {
                return bad(format!("invalid dlna.roots: '{root}' is listed twice"));
            }
            let ok = match root.as_str() {
                "downloads" | "putio" => true,
                other => match other.strip_prefix("cat:") {
                    Some(name) => {
                        !name.trim().is_empty() && !name.contains(['/', '\\', '\0']) && name != ".."
                    }
                    None => other
                        .strip_prefix("watch:")
                        .is_some_and(|p| !p.trim().is_empty()),
                },
            };
            if !ok {
                return bad(format!(
                    "invalid dlna.roots entry '{root}': expected downloads, cat:<category>, watch:<path> or putio"
                ));
            }
        }
        if self.enabled && self.roots.is_empty() {
            return bad("invalid dlna.roots: share at least one folder".to_string());
        }
        if self.putio_folder.is_some_and(|id| id < 0) {
            return bad("invalid dlna.putio_folder: expected a folder id (0 or more)".to_string());
        }
        Ok(())
    }
}

impl Config {
    /// `[dlna]` after the `PUTIO_SYNC_DLNA_ENABLED`, `PUTIO_SYNC_DLNA_NAME` and
    /// `PUTIO_SYNC_DLNA_PORT` overrides (an empty variable counts as unset), checked
    /// against the rest of the config
    pub fn resolve_dlna(&self) -> CoreResult<DlnaConfig> {
        let mut out = self.dlna.clone();
        if let Some(v) = env_value("PUTIO_SYNC_DLNA_ENABLED") {
            out.enabled = match v.as_str() {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => return Err(invalid("dlna.enabled", &v, "true, false, 1, 0")),
            };
        }
        if let Some(v) = env_value("PUTIO_SYNC_DLNA_NAME") {
            out.name = v;
        }
        if let Some(v) = env_value("PUTIO_SYNC_DLNA_PORT") {
            out.port = v
                .trim()
                .parse()
                .map_err(|_| invalid("dlna.port", &v, "a port from 1 to 65535"))?;
        }
        out.validate()?;
        self.validate_dlna_with(&out)?;
        Ok(out)
    }

    /// The rules for `dlna` that need the rest of the config: its port is not the API's
    /// or the metrics', and `watch:` roots are watch folders
    fn validate_dlna_with(&self, dlna: &DlnaConfig) -> CoreResult<()> {
        if !dlna.enabled {
            return Ok(());
        }
        let others = self
            .api_bind_saved()
            .unwrap_or_default()
            .into_iter()
            .map(|a| ("api", a.port()))
            .chain(
                self.metrics
                    .bind
                    .parse("metrics.bind")
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| ("metrics", a.port())),
            );
        for (who, port) in others {
            if port == dlna.port {
                return Err(CoreError::ConfigError(format!(
                    "invalid dlna.port {port}: the {who} already listens on it; pick another"
                )));
            }
        }
        for root in &dlna.roots {
            if let Some(path) = root.strip_prefix("watch:") {
                if !self.watch.paths.iter().any(|p| p.to_string_lossy() == path) {
                    return Err(CoreError::ConfigError(format!(
                        "invalid dlna.roots entry '{root}': {path} is not one of [watch] paths"
                    )));
                }
            }
        }
        Ok(())
    }
}

/// Parse a 24h `HH:MM` time, two digits each
fn parse_run_at(value: &str) -> CoreResult<chrono::NaiveTime> {
    parse_time("cleanup", value)
}

/// Parse a 24h `HH:MM` time for `[section] run_at`, two digits each
fn parse_time(section: &str, value: &str) -> CoreResult<chrono::NaiveTime> {
    let bad = || {
        CoreError::ConfigError(format!(
            "invalid {} run_at '{}': expected 24h HH:MM, e.g. 03:00",
            section, value
        ))
    };
    let (h, m) = value.split_once(':').ok_or_else(bad)?;
    if h.len() != 2 || m.len() != 2 {
        return Err(bad());
    }
    let h: u32 = h.parse().map_err(|_| bad())?;
    let m: u32 = m.parse().map_err(|_| bad())?;
    chrono::NaiveTime::from_hms_opt(h, m, 0).ok_or_else(bad)
}

/// A resolved setting and where it came from. `source` is `Env` when any environment
/// variable overrode the config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved<T> {
    /// The effective value
    pub value: T,
    /// Where it came from
    pub source: LevelSource,
}

fn invalid(setting: &str, value: &str, allowed: &str) -> CoreError {
    CoreError::ConfigError(format!(
        "invalid {} '{}': expected one of {}",
        setting, value, allowed
    ))
}

impl Config {
    /// Resolve `[metrics]`, applying the `PUTIO_SYNC_METRICS_*` overrides. Invalid
    /// values in either place are an error, so startup fails. `PUTIO_SYNC_METRICS_BIND`
    /// takes a comma-separated list.
    pub fn resolve_metrics(&self) -> CoreResult<Resolved<MetricsListen>> {
        let m = &self.metrics;
        let mut source = LevelSource::Config;
        let mut env = |name: &str| {
            let v = std::env::var(name).ok();
            if v.is_some() {
                source = LevelSource::Env;
            }
            v
        };

        let enabled = match env("PUTIO_SYNC_METRICS_ENABLED") {
            None => m.enabled,
            Some(v) => match v.as_str() {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => return Err(invalid("metrics.enabled", &v, "true, false, 1, 0")),
            },
        };
        let bind = env("PUTIO_SYNC_METRICS_BIND")
            .map(|v| BindList::from_env(&v))
            .unwrap_or_else(|| m.bind.clone())
            .parse("metrics.bind")
            .map_err(CoreError::ConfigError)?;
        let path = env("PUTIO_SYNC_METRICS_PATH").unwrap_or_else(|| m.path.clone());
        if !path.starts_with('/') || path.chars().any(char::is_whitespace) {
            return Err(invalid(
                "metrics.path",
                &path,
                "a path starting with / and no spaces",
            ));
        }
        let detail_str = env("PUTIO_SYNC_METRICS_DETAIL").unwrap_or_else(|| m.detail.clone());
        let detail: Detail = detail_str.parse().map_err(CoreError::ConfigError)?;

        Ok(Resolved {
            value: MetricsListen {
                settings: MetricsSettings {
                    enabled,
                    path,
                    detail,
                },
                bind,
            },
            source,
        })
    }
}

impl Config {
    /// Resolve `[api]`, applying the `PUTIO_SYNC_API_*` overrides. Invalid values in
    /// either place are an error, so startup fails. Token errors never echo the value.
    /// `PUTIO_SYNC_API_BIND` takes a comma-separated list.
    pub fn resolve_api(&self) -> CoreResult<Resolved<ApiSettings>> {
        let a = &self.api;
        let mut source = LevelSource::Config;
        let mut env = |name: &str| {
            let v = std::env::var(name).ok();
            if v.is_some() {
                source = LevelSource::Env;
            }
            v
        };

        let listen_str = match env("PUTIO_SYNC_API_LISTEN") {
            Some(v) => Some(v),
            None => match &a.listen {
                None => None,
                Some(Flag::Bool(b)) => Some(b.to_string()),
                Some(Flag::Text(t)) => Some(t.clone()),
            },
        };
        let listen = match listen_str.as_deref() {
            None | Some("true") | Some("1") => true,
            Some("false") | Some("0") => false,
            Some(v) => return Err(invalid("api.listen", v, "true, false, 1, 0")),
        };
        let bind = env("PUTIO_SYNC_API_BIND")
            .map(|v| BindList::from_env(&v))
            .or_else(|| a.bind.clone())
            .unwrap_or_else(|| BindList::from(DEFAULT_API_BIND))
            .parse("api.bind")
            .map_err(CoreError::ConfigError)?;
        let token = env("PUTIO_SYNC_API_TOKEN").or_else(|| a.auth_token.clone());
        if let Some(t) = &token {
            if t.is_empty() || t.chars().any(char::is_whitespace) {
                return Err(CoreError::ConfigError(
                    "invalid api.auth_token: expected a non-empty value without whitespace"
                        .to_string(),
                ));
            }
        }

        Ok(Resolved {
            value: ApiSettings {
                listen,
                bind,
                token,
            },
            source,
        })
    }
}

impl Config {
    /// The addresses the API listens on: `[api] bind` after env, with `tailnet_only`
    /// turning it into `tailnet:<port>` for each distinct port (feature 037)
    pub fn api_listen(&self) -> CoreResult<Vec<ListenAddr>> {
        Ok(self.with_tailnet_only(self.resolve_api()?.value.bind))
    }

    /// The saved `[api] bind` list (no env), parsed and checked
    pub fn api_bind_saved(&self) -> CoreResult<Vec<ListenAddr>> {
        self.api
            .bind
            .clone()
            .unwrap_or_else(|| BindList::from(DEFAULT_API_BIND))
            .parse("api.bind")
            .map_err(CoreError::ConfigError)
    }

    /// `bind` as the API listens on it: with `tailnet_only`, one `tailnet:<port>` per
    /// distinct port
    pub fn with_tailnet_only(&self, bind: Vec<ListenAddr>) -> Vec<ListenAddr> {
        if !self.tailscale.tailnet_only {
            return bind;
        }
        let mut out: Vec<ListenAddr> = Vec::new();
        for entry in bind {
            let t = ListenAddr::Tailnet { port: entry.port() };
            if !out.contains(&t) {
                out.push(t);
            }
        }
        out
    }

    /// The first API entry's port (after env and tailnet-only), else 15432: what other
    /// programs are told when no listener reports a bound port
    pub fn api_port(&self) -> u16 {
        self.api_listen()
            .ok()
            .and_then(|l| l.first().map(ListenAddr::port))
            .unwrap_or(DEFAULT_API_PORT)
    }

    /// Check both bind lists as written in the file, and that they stay apart
    fn validate_binds(&self) -> CoreResult<()> {
        let api = self.api_bind_saved()?;
        let metrics = self
            .metrics
            .bind
            .parse("metrics.bind")
            .map_err(CoreError::ConfigError)?;
        if self.metrics.enabled {
            crate::listen::validate_apart(&api, &metrics).map_err(CoreError::ConfigError)?;
        }
        Ok(())
    }
}

impl Config {
    /// Resolve `[arr]`, applying `PUTIO_SYNC_ARR_PASSWORD` and
    /// `PUTIO_SYNC_RADARR_API_KEY` / `PUTIO_SYNC_SONARR_API_KEY` (the first server of that
    /// kind). An empty variable counts as unset. Errors never echo a secret.
    pub fn resolve_arr(&self) -> CoreResult<ArrSettings> {
        self.arr.validate()?;

        let password = env_value(ARR_PASSWORD_VAR)
            .or_else(|| self.arr.password.clone())
            .filter(|p| !p.is_empty());
        let mut servers = self.arr.servers.clone();
        for kind in [ArrKind::Radarr, ArrKind::Sonarr] {
            if let Some(key) = env_value(arr_key_var(kind)) {
                if let Some(server) = servers.iter_mut().find(|s| s.kind == kind) {
                    server.api_key = Some(key);
                }
            }
        }
        let mut names = std::collections::HashSet::new();
        for server in &mut servers {
            server.url = server.url.trim_end_matches('/').to_string();
            // A legacy category that isn't a valid folder is ignored
            server.category = server
                .category
                .as_deref()
                .and_then(normalize_arr_category)
                .filter(|c| !c.is_empty());
            if server.api_key.as_deref() == Some("") {
                server.api_key = None;
            }
            let name = server
                .name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| server.kind.app_name().to_string());
            let mut unique = name.clone();
            let mut n = 2;
            while !names.insert(unique.clone()) {
                unique = format!("{name} ({n})");
                n += 1;
            }
            server.name = Some(unique);
        }

        Ok(ArrSettings {
            enabled: self.arr.enabled,
            username: self.arr.username.clone(),
            password,
            servers,
        })
    }

    /// Write `arr` as the file's `[arr]` section (feature 018). Every other table, its
    /// comments and the credentials stay as on disk; only `[arr]` keys that differ from the
    /// file are edited. The caller merges secrets the form left empty and never passes an
    /// environment override. Same file handling as
    /// [`Config::save_keeping_file_credentials`].
    pub fn save_arr_section<P: AsRef<Path>>(path: P, arr: &ArrConfig) -> CoreResult<()> {
        let path = path.as_ref();
        let unreadable = |what: &str| {
            CoreError::ConfigError(format!(
                "Failed to {what} {}; settings not saved",
                path.display()
            ))
        };
        let section = |arr: &ArrConfig| -> CoreResult<toml::Table> {
            let value = toml::Value::try_from(arr).map_err(|e| {
                CoreError::ConfigError(format!("Failed to serialize config: {}", e))
            })?;
            Ok(toml::Table::from_iter([("arr".to_string(), value)]))
        };
        let after = section(arr)?;

        if !path.exists() {
            let contents = toml::to_string_pretty(&after).map_err(|e| {
                CoreError::ConfigError(format!("Failed to serialize config: {}", e))
            })?;
            return write_atomically(path, &contents, None);
        }

        let path = fs::canonicalize(path).map_err(|_| unreadable("read"))?;
        let contents = fs::read_to_string(&path).map_err(|_| unreadable("read"))?;
        let mut document: toml_edit::DocumentMut =
            contents.parse().map_err(|_| unreadable("parse"))?;
        let on_disk: Config = toml::from_str(&contents).map_err(|_| unreadable("parse"))?;
        let before = section(&on_disk.arr)?;

        edit_table(document.as_table_mut(), &before, &after);
        let permissions = fs::metadata(&path).ok().map(|m| m.permissions());
        write_atomically(&path, &document.to_string(), permissions)
    }
}

impl Config {
    /// Save the result of first-run setup (feature 013) in one write. Like
    /// [`Config::save_keeping_file_credentials`] it edits only the keys that changed, so
    /// comments and unknown keys stay; unlike it, `[auth]` and `[arr]` are written from
    /// memory, and `api.auth_token` too when `with_api_token` (setup created one).
    ///
    /// A file that does not parse (setup was started because of it) is kept next to the
    /// new one as `<name>.bak` and replaced. A new file is 0600; an existing one keeps
    /// its mode.
    pub fn save_setup<P: AsRef<Path>>(&self, path: P, with_api_token: bool) -> CoreResult<()> {
        let path = path.as_ref();
        let table = |config: &Config| -> CoreResult<toml::Table> {
            let mut table = toml::Table::try_from(config).map_err(|e| {
                CoreError::ConfigError(format!("Failed to serialize config: {}", e))
            })?;
            if !with_api_token {
                if let Some(toml::Value::Table(api)) = table.get_mut("api") {
                    api.remove("auth_token");
                }
            }
            Ok(table)
        };
        let after = table(self)?;
        let fresh = |path: &Path, permissions| {
            let contents = toml::to_string_pretty(&after).map_err(|e| {
                CoreError::ConfigError(format!("Failed to serialize config: {}", e))
            })?;
            write_atomically(path, &contents, permissions)
        };
        if !path.exists() {
            return fresh(path, None);
        }

        let unreadable = || {
            CoreError::ConfigError(format!(
                "Failed to read {}; settings not saved",
                path.display()
            ))
        };
        let path = fs::canonicalize(path).map_err(|_| unreadable())?;
        let contents = fs::read_to_string(&path).map_err(|_| unreadable())?;
        let permissions = fs::metadata(&path).ok().map(|m| m.permissions());
        let parsed = contents
            .parse::<toml_edit::DocumentMut>()
            .ok()
            .zip(toml::from_str::<Config>(&contents).ok());
        let Some((mut document, mut on_disk)) = parsed else {
            let mut backup = path.clone().into_os_string();
            backup.push(".bak");
            fs::copy(&path, &backup).map_err(|e| {
                CoreError::ConfigError(format!("Failed to keep the old config: {}", e))
            })?;
            return fresh(&path, permissions);
        };
        on_disk.expand_paths().map_err(|_| unreadable())?;
        let before = table(&on_disk)?;
        edit_table(document.as_table_mut(), &before, &after);
        write_atomically(&path, &document.to_string(), permissions)
    }
}

impl Config {
    /// Resolve `[precheck]`, applying `PUTIO_SYNC_PRECHECK_ENABLED` and
    /// `PUTIO_SYNC_PRECHECK_MAGNET_LOOKUP`. An empty variable counts as unset.
    pub fn resolve_precheck(&self) -> CoreResult<PrecheckConfig> {
        self.precheck.validate(&self.watch.paths)?;
        let mut out = self.precheck.clone();
        for (name, setting, field) in [
            (
                "PUTIO_SYNC_PRECHECK_ENABLED",
                "precheck.enabled",
                &mut out.enabled,
            ),
            (
                "PUTIO_SYNC_PRECHECK_MAGNET_LOOKUP",
                "precheck.magnet_lookup",
                &mut out.magnet_lookup,
            ),
        ] {
            if let Some(v) = std::env::var(name).ok().filter(|v| !v.is_empty()) {
                *field = match v.as_str() {
                    "true" | "1" => true,
                    "false" | "0" => false,
                    _ => return Err(invalid(setting, &v, "true, false, 1, 0")),
                };
            }
        }
        Ok(out)
    }
}

/// Poll the config file and pass its `[logging]` section to `on_change` whenever the
/// file's modified time or length changes.
///
/// Load and metadata errors are logged once per distinct file state, and polling carries
/// on. Production uses a 10 s interval, so a level change applies within 30 s.
pub fn spawn_reload_poller(
    path: PathBuf,
    interval: std::time::Duration,
    on_change: impl Fn(crate::logging::LoggingConfig) + Send + 'static,
) -> tokio::task::JoinHandle<()> {
    type Stamp = Option<(std::time::SystemTime, u64)>;

    async fn stamp(path: &Path) -> std::io::Result<Stamp> {
        let meta = tokio::fs::metadata(path).await?;
        Ok(Some((meta.modified()?, meta.len())))
    }

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The state at startup is the one already applied.
        ticker.tick().await;
        let mut last: Stamp = stamp(&path).await.unwrap_or(None);
        let mut missing = false;

        loop {
            ticker.tick().await;
            let current = match stamp(&path).await {
                Ok(current) => current,
                Err(e) => {
                    if !missing {
                        tracing::error!(path = %path.display(), error = %e, "Config file unreadable; logging settings not reloaded");
                        missing = true;
                    }
                    last = None;
                    continue;
                }
            };
            if current == last {
                continue;
            }
            last = current;
            missing = false;

            let loaded = {
                let path = path.clone();
                tokio::task::spawn_blocking(move || Config::load(&path)).await
            };
            match loaded {
                Ok(Ok(config)) => on_change(config.logging),
                Ok(Err(e)) => {
                    tracing::error!(path = %path.display(), error = %e, "Config reload failed; logging settings not changed");
                }
                Err(e) => {
                    tracing::error!(error = %e, "Config reload task failed");
                }
            }
        }
    })
}

/// `config` as a TOML table without the credentials (`[auth]`, `api.auth_token`), which
/// the settings save never writes
pub(crate) fn settings_table(config: &Config) -> CoreResult<toml::Table> {
    let mut table = toml::Table::try_from(config)
        .map_err(|e| CoreError::ConfigError(format!("Failed to serialize config: {}", e)))?;
    table.remove("auth");
    // `[arr]` has its own writer (`save_arr_section`), so this save keeps its password
    // and API keys as on disk
    table.remove("arr");
    if let Some(toml::Value::Table(api)) = table.get_mut("api") {
        api.remove("auth_token");
    }
    Ok(table)
}

/// Bring `document` from `before` to `after`, touching only the keys that differ. Keys
/// in neither (unknown to this version) are left alone.
fn edit_table(document: &mut toml_edit::Table, before: &toml::Table, after: &toml::Table) {
    let empty = toml::Table::new();
    for (key, new) in after {
        let old = before.get(key);
        if old == Some(new) {
            continue;
        }
        if let toml::Value::Table(new_table) = new {
            let old_table = match old {
                Some(toml::Value::Table(table)) => table,
                _ => &empty,
            };
            match document.get_mut(key) {
                Some(toml_edit::Item::Table(table)) => {
                    edit_table(table, old_table, new_table);
                    continue;
                }
                None => {
                    document.insert(key, toml_edit::Item::Table(new_edit_table(new_table)));
                    continue;
                }
                // `api = { ... }`: edited key by key too, so its token stays
                Some(toml_edit::Item::Value(toml_edit::Value::InlineTable(inline))) => {
                    let mut table = inline.clone().into_table();
                    edit_table(&mut table, old_table, new_table);
                    let mut edited = table.into_inline_table();
                    *edited.decor_mut() = inline.decor().clone();
                    *inline = edited;
                    continue;
                }
                // Any other value: replaced whole below
                Some(_) => {}
            }
        }
        // `[[key]]` entries: edited in place by index, extra ones appended, surplus removed
        if let Some(new_tables) = tables_of(new) {
            let old_tables = old.and_then(tables_of).unwrap_or_default();
            match document.get_mut(key) {
                Some(toml_edit::Item::ArrayOfTables(array)) => {
                    for (i, new_table) in new_tables.iter().enumerate() {
                        match array.get_mut(i) {
                            Some(table) => edit_table(
                                table,
                                old_tables.get(i).copied().unwrap_or(&empty),
                                new_table,
                            ),
                            None => array.push(new_edit_table(new_table)),
                        }
                    }
                    while array.len() > new_tables.len() {
                        array.remove(array.len() - 1);
                    }
                    continue;
                }
                None => {
                    let mut array = toml_edit::ArrayOfTables::new();
                    for new_table in new_tables {
                        array.push(new_edit_table(new_table));
                    }
                    document.insert(key, toml_edit::Item::ArrayOfTables(array));
                    continue;
                }
                // `key = [{ ... }]`: replaced whole below
                Some(_) => {}
            }
        }
        let mut value = edit_value(new);
        match document.get_mut(key) {
            Some(toml_edit::Item::Value(existing)) => {
                // Keep the spacing and any trailing comment
                *value.decor_mut() = existing.decor().clone();
                *existing = value;
            }
            _ => {
                document.insert(key, toml_edit::Item::Value(value));
            }
        }
    }
    for key in before.keys() {
        if !after.contains_key(key) {
            document.remove(key);
        }
    }
}

/// A new document table holding `table`; its header is left out when it holds only
/// tables (`[download.presets]` above `[download.presets.x]`)
fn new_edit_table(table: &toml::Table) -> toml_edit::Table {
    let mut edited = toml_edit::Table::new();
    edited.set_implicit(true);
    edit_table(&mut edited, &toml::Table::new(), table);
    edited
}

/// The tables of a non-empty array holding only tables (a `[[...]]` array)
fn tables_of(value: &toml::Value) -> Option<Vec<&toml::Table>> {
    let toml::Value::Array(items) = value else {
        return None;
    };
    if items.is_empty() {
        return None;
    }
    items.iter().map(|item| item.as_table()).collect()
}

/// A `toml` value as a `toml_edit` one (tables become inline tables)
fn edit_value(value: &toml::Value) -> toml_edit::Value {
    match value {
        toml::Value::String(s) => s.as_str().into(),
        toml::Value::Integer(i) => (*i).into(),
        toml::Value::Float(f) => (*f).into(),
        toml::Value::Boolean(b) => (*b).into(),
        toml::Value::Datetime(d) => (*d).into(),
        toml::Value::Array(items) => {
            toml_edit::Value::Array(items.iter().map(edit_value).collect())
        }
        toml::Value::Table(table) => toml_edit::Value::InlineTable(
            table
                .iter()
                .map(|(k, v)| (k.as_str(), edit_value(v)))
                .collect(),
        ),
    }
}

/// Write `contents` to a temp file next to `path` and rename it over `path`, so a reader
/// never sees half a file. The file gets `permissions`, or 0600 on Unix when `None`.
fn write_atomically(
    path: &Path,
    contents: &str,
    permissions: Option<fs::Permissions>,
) -> CoreResult<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);

    let failed =
        |e: std::io::Error| CoreError::ConfigError(format!("Failed to write config file: {}", e));
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    fs::create_dir_all(dir)
        .map_err(|e| CoreError::ConfigError(format!("Failed to create config directory: {}", e)))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.toml".to_string());
    let temp = dir.join(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));

    let write = || -> std::io::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        #[cfg(unix)]
        let permissions = permissions.or_else(|| {
            use std::os::unix::fs::PermissionsExt;
            Some(fs::Permissions::from_mode(0o600))
        });
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, path)
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&temp);
        failed(e)
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn updates_section_defaults_and_round_trips() {
        // feature 055: a config without [updates] checks, and saves without the section
        let saved = toml::to_string(&super::Config::default()).unwrap();
        assert!(!saved.contains("[updates]"), "{saved}");
        let config: super::Config = toml::from_str(&saved).unwrap();
        assert!(config.updates.check);
        assert!(!config.updates.auto_install);
        assert_eq!(toml::to_string(&config).unwrap(), saved);

        let text = format!("{saved}\n[updates]\ncheck = false\nauto_install = true\n");
        let config: super::Config = toml::from_str(&text).unwrap();
        assert!(!config.updates.check);
        assert!(config.updates.auto_install);
        let saved = toml::to_string(&config).unwrap();
        assert!(saved.contains("[updates]"), "{saved}");
    }

    #[test]
    fn old_pattern_values_load_and_bad_ones_are_refused() {
        // feature 020: comma lists and braces mean what they always meant
        for text in ["*.mkv,*.mp4", "*.{mkv,mp4}", "*.mkv, *.mp4"] {
            let mut config = super::Config::default();
            config.download.whitelist_patterns = Some(text.to_string());
            config.validate().unwrap();
            let rules = crate::rules::RuleSet::from_config(&config.download);
            assert_eq!(rules.decide("", "a.mkv"), crate::rules::Decision::Download);
            assert_eq!(rules.decide("", "b.MP4"), crate::rules::Decision::Download);
            assert_ne!(rules.decide("", "c.avi"), crate::rules::Decision::Download);
        }
        let mut config = super::Config::default();
        config.download.blacklist_patterns = Some("\"abc".to_string());
        let err = config.validate().unwrap_err().to_string();
        assert!(
            err.contains("[download] blacklist_patterns (Never download): unclosed quote"),
            "{err}"
        );
    }

    #[test]
    fn verify_keys_default_parse_and_validate() {
        // feature 034: an older config without the keys gets the defaults
        let text: String = toml::to_string(&super::DownloadConfig::default())
            .unwrap()
            .lines()
            .filter(|l| !l.starts_with("verify"))
            .map(|l| format!("{l}\n"))
            .collect();
        let download: super::DownloadConfig = toml::from_str(&text).unwrap();
        assert!(download.verify);
        assert_eq!(download.verify_max_redownloads, 2);
        assert_eq!(download.verify_read_limit_mb_per_sec, 50);
        assert!(download.verify_follow_bandwidth_pause);
        assert_eq!(download.verify_read_limit_bytes(), 50_000_000);

        let mut config = super::Config::default();
        config.download.verify = false;
        config.download.verify_max_redownloads = 0;
        config.download.verify_read_limit_mb_per_sec = 10_000;
        config.download.verify_follow_bandwidth_pause = false;
        config.validate().unwrap();

        for (redownloads, read_mb, want) in [
            (
                9,
                50,
                "download.verify_max_redownloads must be 0..=5 (got 9)",
            ),
            (
                2,
                0,
                "download.verify_read_limit_mb_per_sec must be 1..=10000 (got 0)",
            ),
            (
                2,
                10_001,
                "download.verify_read_limit_mb_per_sec must be 1..=10000 (got 10001)",
            ),
        ] {
            let mut config = super::Config::default();
            config.download.verify_max_redownloads = redownloads;
            config.download.verify_read_limit_mb_per_sec = read_mb;
            let err = config.validate().unwrap_err().to_string();
            assert!(err.contains(want), "{err}");
        }
    }

    #[test]
    fn transfers_section_defaults_and_overrides() {
        let config: super::Config = toml::from_str("").unwrap();
        assert_eq!(config.transfers, super::TransfersConfig::default());
        assert_eq!(config.transfers.stall_threshold_hours, 6);
        assert_eq!(config.transfers.stall_check_interval_secs, 600);

        let config: super::Config =
            toml::from_str("[transfers]\nstall_threshold_hours = 12\n").unwrap();
        assert_eq!(config.transfers.stall_threshold_hours, 12);
        assert_eq!(config.transfers.stall_check_interval_secs, 600);
    }

    #[test]
    fn finished_retention_hours_default_and_range() {
        let config: super::Config = toml::from_str("").unwrap();
        assert_eq!(config.transfers.finished_retention_hours, 24);
        assert!(config.transfers.validate().is_ok());

        for (hours, ok) in [(1, true), (720, true), (0, false), (721, false)] {
            let transfers = super::TransfersConfig {
                finished_retention_hours: hours,
                ..Default::default()
            };
            match transfers.validate() {
                Ok(()) => assert!(ok, "{hours} should be refused"),
                Err(e) => {
                    assert!(!ok, "{hours} should be accepted");
                    assert!(e.to_string().contains("must be from 1 to 720"), "{e}");
                }
            }
        }
    }

    #[test]
    fn pipeline_defaults_and_rules() {
        let config: super::Config = toml::from_str("").unwrap();
        assert_eq!(config.pipeline.concurrency, 2);
        assert_eq!(config.pipeline.optional_retries, vec![60, 300, 600]);
        assert_eq!(config.pipeline.staging, super::StagingMode::Auto);
        assert!(config.validate().is_ok());

        let config: super::Config = toml::from_str(
            "[pipeline]\nconcurrency = 4\noptional_retries = []\nstaging = \"off\"\n",
        )
        .unwrap();
        assert_eq!(config.pipeline.concurrency, 4);
        assert!(config.pipeline.optional_retries.is_empty());
        assert_eq!(config.pipeline.staging, super::StagingMode::Off);

        for (n, ok) in [(1, true), (16, true), (0, false), (17, false)] {
            let pipeline = super::PipelineConfig {
                concurrency: n,
                ..Default::default()
            };
            assert_eq!(pipeline.validate().is_ok(), ok, "concurrency {n}");
        }
        let many = super::PipelineConfig {
            optional_retries: vec![1; 11],
            ..Default::default()
        };
        assert!(many
            .validate()
            .unwrap_err()
            .to_string()
            .contains("at most 10"));
        assert!(toml::from_str::<super::Config>("[pipeline]\nstaging = \"maybe\"\n").is_err());
    }

    use super::*;
    use tempfile::TempDir;

    fn create_test_config() -> Config {
        Config {
            auth: Some(AuthConfig::with_token("test_token_123".to_string())),
            sync: SyncConfig {
                local_path: PathBuf::from("/tmp/putio"),
                remote_parent_id: Some(0),
                delete_after_download: true,
            },
            download: DownloadConfig {
                workers: 3,
                target_directory: PathBuf::from("/tmp/putio"),
                max_concurrent: Some(3),
                buffer_size_mb: 16,
                chunk_size_mb: Some(256),
                resume_enabled: true,
                on_existing: None,
                skip_existing: Some(false),
                delete_after_download: false,
                mode: SyncMode::Download,
                stream_mp4: true,
                progress_style: ProgressStyle::PerFile,
                sort_order: SortOrder::ByDate,
                reverse_sort: false,
                whitelist_patterns: Some("*.mkv".to_string()),
                blacklist_patterns: Some("*.exe".to_string()),
                bandwidth_limit_bytes_per_sec: None,
                bandwidth_schedule: Vec::new(),
                slow_limit_bytes_per_sec: crate::bandwidth::DEFAULT_SLOW_LIMIT,
                verify: true,
                verify_max_redownloads: 2,
                verify_read_limit_mb_per_sec: 50,
                verify_follow_bandwidth_pause: true,
                max_retries: 3,
                retry_initial_delay_secs: 1,
                delete_empty_folders: true,
                blocked_extensions: default_blocked_extensions(),
                preset: None,
                rules: Vec::new(),
                presets: BTreeMap::new(),
            },
            watch: WatchConfig {
                enabled: false,
                paths: vec![PathBuf::from("/remote/movies")],
                refresh_cycle: 60,
                parent_folder_id: Some(0),
                archive_directory: None,
                delete_after_upload: false,
            },
            notifications: NotificationConfig::default(),
            api: ApiConfig::default(),
            logging: crate::logging::LoggingConfig::default(),
            metrics: MetricsConfig::default(),
            cleanup: CleanupConfig::default(),
            transfers: TransfersConfig::default(),
            pipeline: PipelineConfig::default(),
            subtitles: Default::default(),
            translation: Default::default(),
            arr: ArrConfig::default(),
            doctor: DoctorConfig::default(),
            putio: PutioConfig::default(),
            precheck: PrecheckConfig::default(),
            storage: StorageConfig::default(),
            tailscale: TailscaleConfig::default(),
            sign_in: crate::sign_in::SignInConfig::default(),
            dlna: DlnaConfig::default(),
            updates: UpdatesConfig::default(),
            organise: Default::default(),
            hooks: Vec::new(),
            plugins_dir: None,
            plugins: Vec::new(),
            plugin_editor: false,
        }
    }

    #[test]
    fn test_default_path() {
        let path = Config::default_path().unwrap();
        assert!(path.to_string_lossy().contains(".putiosync"));
        assert!(path.to_string_lossy().ends_with("config.toml"));
    }

    #[test]
    fn test_save_and_load_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let config = create_test_config();

        // Save config
        config.save(&config_path).unwrap();
        assert!(config_path.exists());

        // Load config
        let loaded = Config::load(&config_path).unwrap();

        // Verify loaded config matches
        let auth = loaded.auth.as_ref().unwrap();
        assert_eq!(auth.auth_method, AuthMethod::Token);
        assert_eq!(auth.token.as_ref().unwrap(), "test_token_123");
        assert_eq!(loaded.sync.local_path, PathBuf::from("/tmp/putio"));
        assert_eq!(loaded.sync.remote_parent_id, Some(0));
        assert!(loaded.sync.delete_after_download);
        assert_eq!(loaded.download.workers, 3);
        assert_eq!(loaded.download.max_concurrent, Some(3));
        assert_eq!(loaded.download.chunk_size_mb, Some(256));
        assert!(loaded.download.resume_enabled);
        assert_eq!(
            loaded.download.whitelist_patterns,
            Some("*.mkv".to_string())
        );
        assert_eq!(
            loaded.download.blacklist_patterns,
            Some("*.exe".to_string())
        );
        assert!(!loaded.watch.enabled);
        assert_eq!(loaded.watch.paths, vec![PathBuf::from("/remote/movies")]);
        assert_eq!(loaded.watch.refresh_cycle, 60);
        assert_eq!(loaded.watch.parent_folder_id, Some(0));
    }

    #[test]
    fn test_load_nonexistent_file() {
        let result = Config::load("/nonexistent/path/config.toml");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::ConfigError(_)));
    }

    #[test]
    fn test_save_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir
            .path()
            .join("nested")
            .join("dir")
            .join("config.toml");

        let config = create_test_config();
        config.save(&config_path).unwrap();

        assert!(config_path.exists());
        assert!(config_path.parent().unwrap().exists());
    }

    #[test]
    fn test_config_serialization() {
        let config = create_test_config();
        let toml_str = toml::to_string_pretty(&config).unwrap();

        // Verify TOML contains expected fields
        assert!(toml_str.contains("token"));
        assert!(toml_str.contains("local_path"));
        assert!(toml_str.contains("max_concurrent"));
        assert!(toml_str.contains("refresh_cycle"));
    }

    #[cfg(unix)]
    #[test]
    fn test_config_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let config = create_test_config();
        config.save(&config_path).unwrap();

        let metadata = fs::metadata(&config_path).unwrap();
        let permissions = metadata.permissions();
        assert_eq!(permissions.mode() & 0o777, 0o600);
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.auth.is_none());
        assert_eq!(config.sync.local_path, PathBuf::from("./putio-downloads"));
        assert_eq!(config.sync.remote_parent_id, Some(0));
        assert!(!config.sync.delete_after_download);
        assert_eq!(config.download.workers, 2);
        assert_eq!(config.download.max_concurrent, None);
        assert_eq!(config.download.buffer_size_mb, 16);
        assert_eq!(config.download.chunk_size_mb, None);
        assert!(config.download.resume_enabled);
        assert_eq!(config.download.on_existing(), ExistingFile::Resume);
        assert!(!config.download.delete_after_download);
        assert_eq!(config.download.progress_style, ProgressStyle::PerFile);
        assert_eq!(config.download.sort_order, SortOrder::ByDate);
        assert!(!config.download.reverse_sort);
        assert!(!config.watch.enabled);
        assert_eq!(config.watch.paths.len(), 0);
        assert_eq!(config.watch.refresh_cycle, 60);
        assert_eq!(config.watch.parent_folder_id, Some(0));
    }

    #[test]
    fn test_config_validate_success() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validate_workers_zero() {
        let mut config = Config::default();
        config.download.workers = 0;
        let result = config.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CoreError::ConfigError(_)));
    }

    #[test]
    fn test_config_validate_chunk_size_zero() {
        let mut config = Config::default();
        config.download.chunk_size_mb = Some(0);
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_config_validate_chunk_size_too_large() {
        let mut config = Config::default();
        config.download.chunk_size_mb = Some(20000);
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_config_validate_negative_refresh_cycle() {
        let mut config = Config::default();
        config.watch.refresh_cycle = -1;
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    fn test_config_validate_negative_parent_id() {
        let mut config = Config::default();
        config.sync.remote_parent_id = Some(-5);
        let result = config.validate();
        assert!(result.is_err());
    }

    #[test]
    #[allow(deprecated)]
    fn test_config_builder() {
        let config = Config::builder()
            .token("test_token".to_string())
            .local_path(PathBuf::from("/test"))
            .remote_parent_id(123)
            .delete_after_download(true)
            .workers(5)
            .chunk_size_mb(512)
            .resume_enabled(false)
            .whitelist_patterns("*.mp4".to_string())
            .blacklist_patterns("*.exe".to_string())
            .watch_enabled(true)
            .watch_paths(vec![PathBuf::from("/path1"), PathBuf::from("/path2")])
            .refresh_cycle(30)
            .watch_parent_folder_id(123)
            .build();

        let auth = config.auth.as_ref().unwrap();
        assert_eq!(auth.auth_method, AuthMethod::Token);
        assert_eq!(auth.token.as_ref().unwrap(), "test_token");
        assert_eq!(config.sync.local_path, PathBuf::from("/test"));
        assert_eq!(config.sync.remote_parent_id, Some(123));
        assert!(config.sync.delete_after_download);
        assert_eq!(config.download.workers, 5);
        assert_eq!(config.download.chunk_size_mb, Some(512));
        assert!(!config.download.resume_enabled);
        assert_eq!(
            config.download.whitelist_patterns,
            Some("*.mp4".to_string())
        );
        assert_eq!(
            config.download.blacklist_patterns,
            Some("*.exe".to_string())
        );
        assert!(config.watch.enabled);
        assert_eq!(
            config.watch.paths,
            vec![PathBuf::from("/path1"), PathBuf::from("/path2")]
        );
        assert_eq!(config.watch.refresh_cycle, 30);
        assert_eq!(config.watch.parent_folder_id, Some(123));
    }

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert_eq!(config.local_path, PathBuf::from("./putio-downloads"));
        assert_eq!(config.remote_parent_id, Some(0));
        assert!(!config.delete_after_download);
    }

    #[test]
    fn test_download_config_default() {
        let config = DownloadConfig::default();
        assert_eq!(config.workers, 2);
        assert_eq!(config.max_concurrent, None);
        assert_eq!(config.buffer_size_mb, 16);
        assert_eq!(config.chunk_size_mb, None);
        assert!(config.resume_enabled);
        assert_eq!(config.on_existing(), ExistingFile::Resume);
        assert!(!config.delete_after_download);
        assert_eq!(config.progress_style, ProgressStyle::PerFile);
        assert_eq!(config.sort_order, SortOrder::ByDate);
        assert!(!config.reverse_sort);
        assert_eq!(config.whitelist_patterns, None);
        assert_eq!(config.blacklist_patterns, None);
        assert_eq!(config.bandwidth_limit_bytes_per_sec, None);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.retry_initial_delay_secs, 1);
    }

    fn download_toml_without_blocked() -> String {
        let mut value = toml::Value::try_from(DownloadConfig::default()).unwrap();
        value.as_table_mut().unwrap().remove("blocked_extensions");
        toml::to_string(&value).unwrap()
    }

    #[test]
    fn test_blocked_extensions_missing_key_uses_default() {
        let config: DownloadConfig = toml::from_str(&download_toml_without_blocked()).unwrap();
        assert_eq!(
            config.blocked_extensions,
            [
                "exe", "msi", "bat", "cmd", "com", "scr", "pif", "vbs", "js", "jar", "ps1", "lnk",
                "apk", "dmg", "pkg"
            ]
        );
    }

    #[test]
    fn test_existing_keys_missing_default_to_resume() {
        let config: DownloadConfig =
            toml::from_str(&toml::to_string(&DownloadConfig::default()).unwrap()).unwrap();
        assert_eq!(config.on_existing, None);
        assert_eq!(config.skip_existing, None);
        assert_eq!(config.on_existing(), ExistingFile::Resume);
    }

    fn with_download_key(extra: &str) -> CoreResult<DownloadConfig> {
        Ok(toml::from_str(&format!(
            "{}{extra}\n",
            download_toml_without_blocked()
        ))?)
    }

    #[test]
    fn test_old_skip_existing_maps_to_on_existing() {
        let config = with_download_key("skip_existing = false").unwrap();
        assert_eq!(config.on_existing(), ExistingFile::Overwrite);
        let config = with_download_key("skip_existing = true").unwrap();
        assert_eq!(config.on_existing(), ExistingFile::Resume);
    }

    #[test]
    fn test_on_existing_values() {
        let config = with_download_key("on_existing = \"keep_both\"").unwrap();
        assert_eq!(config.on_existing(), ExistingFile::KeepBoth);
        let config = with_download_key("on_existing = \"rename\"").unwrap();
        assert_eq!(config.on_existing(), ExistingFile::KeepBoth);
        let config = with_download_key("on_existing = \"overwrite\"").unwrap();
        assert_eq!(config.on_existing(), ExistingFile::Overwrite);
    }

    #[test]
    fn test_on_existing_unknown_value_lists_choices() {
        let message = with_download_key("on_existing = \"replace\"")
            .unwrap_err()
            .to_string();
        for value in ["resume", "overwrite", "keep_both"] {
            assert!(message.contains(value), "{message}");
        }
    }

    #[test]
    fn test_on_existing_wins_over_skip_existing() {
        let config =
            with_download_key("on_existing = \"keep_both\"\nskip_existing = false").unwrap();
        assert_eq!(config.on_existing(), ExistingFile::KeepBoth);
        assert_eq!(config.both_existing_keys(), ["download"]);
    }

    #[test]
    fn test_blocked_extensions_empty_disables() {
        let toml = format!(
            "{}blocked_extensions = []\n",
            download_toml_without_blocked()
        );
        let config: DownloadConfig = toml::from_str(&toml).unwrap();
        assert!(config.blocked_extensions.is_empty());
    }

    #[test]
    fn test_blocked_extensions_replaces_default() {
        let toml = format!(
            "{}blocked_extensions = [\"iso\"]\n",
            download_toml_without_blocked()
        );
        let config: DownloadConfig = toml::from_str(&toml).unwrap();
        assert_eq!(config.blocked_extensions, ["iso"]);
    }

    #[test]
    fn test_watch_config_default() {
        let config = WatchConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.paths.len(), 0);
        assert_eq!(config.refresh_cycle, 60);
        assert_eq!(config.parent_folder_id, Some(0));
    }

    #[test]
    fn test_auth_config_with_token() {
        let auth = AuthConfig::with_token("test_token".to_string());
        assert_eq!(auth.auth_method, AuthMethod::Token);
        assert_eq!(auth.token.as_ref().unwrap(), "test_token");
        assert!(auth.username.is_none());
        assert!(auth.password.is_none());
        assert!(auth.validate().is_ok());
    }

    #[test]
    fn test_auth_config_with_app_password() {
        let auth =
            AuthConfig::with_app_password("user@example.com".to_string(), "pass123".to_string());
        assert_eq!(auth.auth_method, AuthMethod::AppPassword);
        assert!(auth.token.is_none());
        assert_eq!(auth.username.as_ref().unwrap(), "user@example.com");
        assert_eq!(auth.password.as_ref().unwrap(), "pass123");
        assert!(auth.validate().is_ok());
    }

    #[test]
    fn test_auth_config_validate_token_missing() {
        let auth = AuthConfig {
            auth_method: AuthMethod::Token,
            token: None,
            username: None,
            password: None,
        };
        assert!(auth.validate().is_err());
    }

    #[test]
    fn test_auth_config_validate_token_empty() {
        let auth = AuthConfig {
            auth_method: AuthMethod::Token,
            token: Some("".to_string()),
            username: None,
            password: None,
        };
        assert!(auth.validate().is_err());
    }

    #[test]
    fn test_auth_config_validate_username_missing() {
        let auth = AuthConfig {
            auth_method: AuthMethod::AppPassword,
            token: None,
            username: None,
            password: Some("pass".to_string()),
        };
        assert!(auth.validate().is_err());
    }

    #[test]
    fn test_auth_config_validate_password_missing() {
        let auth = AuthConfig {
            auth_method: AuthMethod::AppPassword,
            token: None,
            username: Some("user@example.com".to_string()),
            password: None,
        };
        assert!(auth.validate().is_err());
    }

    #[test]
    fn test_config_builder_credentials() {
        let config = Config::builder()
            .credentials("user@example.com".to_string(), "pass123".to_string())
            .local_path(PathBuf::from("/test"))
            .build();

        let auth = config.auth.as_ref().unwrap();
        assert_eq!(auth.auth_method, AuthMethod::AppPassword);
        assert_eq!(auth.username.as_ref().unwrap(), "user@example.com");
        assert_eq!(auth.password.as_ref().unwrap(), "pass123");
        assert!(auth.token.is_none());
    }

    #[test]
    fn test_config_builder_username_password_separate() {
        let config = Config::builder()
            .username("user@example.com".to_string())
            .password("pass123".to_string())
            .build();

        let auth = config.auth.as_ref().unwrap();
        assert_eq!(auth.auth_method, AuthMethod::AppPassword);
        assert_eq!(auth.username.as_ref().unwrap(), "user@example.com");
        assert_eq!(auth.password.as_ref().unwrap(), "pass123");
    }

    #[test]
    fn test_auth_method_default() {
        let method = AuthMethod::default();
        assert_eq!(method, AuthMethod::Token);
    }
}

#[cfg(test)]
mod metrics_tests {
    use super::*;

    /// Serialises tests that change PUTIO_SYNC_METRICS_*
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const VARS: [&str; 7] = [
        "PUTIO_SYNC_METRICS_ENABLED",
        "PUTIO_SYNC_METRICS_BIND",
        "PUTIO_SYNC_METRICS_PATH",
        "PUTIO_SYNC_METRICS_DETAIL",
        "PUTIO_SYNC_API_LISTEN",
        "PUTIO_SYNC_API_BIND",
        "PUTIO_SYNC_API_TOKEN",
    ];

    fn with_env<R>(vars: &[(&str, &str)], f: impl FnOnce() -> R) -> R {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for v in VARS {
            std::env::remove_var(v);
        }
        for (k, v) in vars {
            std::env::set_var(k, v);
        }
        let out = f();
        for v in VARS {
            std::env::remove_var(v);
        }
        out
    }

    fn err(config: &Config) -> String {
        match config.resolve_metrics() {
            Err(CoreError::ConfigError(msg)) => msg,
            other => panic!("expected a config error, got {:?}", other),
        }
    }

    #[test]
    fn defaults() {
        let r = with_env(&[], || Config::default().resolve_metrics().unwrap());
        assert_eq!(r.value.settings, MetricsSettings::default());
        assert_eq!(r.value.bind, vec!["0.0.0.0:9742".parse().unwrap()]);
        assert_eq!(r.source, LevelSource::Config);
    }

    #[test]
    fn toml_table_is_read() {
        let config: Config = toml::from_str(
            "[metrics]\nenabled = false\nbind = \"127.0.0.1:9100\"\ndetail = \"detailed\"\n",
        )
        .unwrap();
        let r = with_env(&[], || config.resolve_metrics().unwrap());
        assert!(!r.value.settings.enabled);
        assert_eq!(r.value.bind, vec!["127.0.0.1:9100".parse().unwrap()]);
        assert_eq!(r.value.settings.path, "/metrics");
        assert_eq!(r.value.settings.detail, Detail::Detailed);
    }

    #[test]
    fn env_beats_config() {
        let mut config = Config::default();
        config.metrics.enabled = false;
        let r = with_env(
            &[
                ("PUTIO_SYNC_METRICS_ENABLED", "1"),
                ("PUTIO_SYNC_METRICS_BIND", "127.0.0.1:0"),
                ("PUTIO_SYNC_METRICS_PATH", "/prom"),
                ("PUTIO_SYNC_METRICS_DETAIL", "detailed"),
            ],
            || config.resolve_metrics().unwrap(),
        );
        assert!(r.value.settings.enabled);
        assert_eq!(r.value.bind, vec!["127.0.0.1:0".parse().unwrap()]);
        assert_eq!(r.value.settings.path, "/prom");
        assert_eq!(r.value.settings.detail, Detail::Detailed);
        assert_eq!(r.source, LevelSource::Env);
    }

    #[test]
    fn invalid_env_values_give_exact_messages() {
        let config = Config::default();
        let cases = [
            (
                "PUTIO_SYNC_METRICS_ENABLED",
                "yes",
                "invalid metrics.enabled 'yes': expected one of true, false, 1, 0",
            ),
            (
                "PUTIO_SYNC_METRICS_BIND",
                "na s:9742",
                "invalid metrics.bind 'na s:9742': 'na s' is not an IPv4 address or interface name (expected ip:port, [ipv6]:port, interface:port or tailnet:port)",
            ),
            (
                "PUTIO_SYNC_METRICS_PATH",
                "metrics",
                "invalid metrics.path 'metrics': expected one of a path starting with / and no spaces",
            ),
            (
                "PUTIO_SYNC_METRICS_PATH",
                "/my metrics",
                "invalid metrics.path '/my metrics': expected one of a path starting with / and no spaces",
            ),
            (
                "PUTIO_SYNC_METRICS_DETAIL",
                "max",
                "invalid metrics.detail 'max': expected one of basic, detailed",
            ),
        ];
        for (var, value, message) in cases {
            assert_eq!(with_env(&[(var, value)], || err(&config)), message);
        }
    }

    #[test]
    fn invalid_toml_value_gives_the_same_message() {
        let mut config = Config::default();
        config.metrics.detail = "max".to_string();
        assert_eq!(
            with_env(&[], || err(&config)),
            "invalid metrics.detail 'max': expected one of basic, detailed"
        );
    }

    fn api_err(config: &Config) -> String {
        match config.resolve_api() {
            Err(CoreError::ConfigError(msg)) => msg,
            other => panic!("expected a config error, got {:?}", other),
        }
    }

    #[test]
    fn api_defaults() {
        let r = with_env(&[], || Config::default().resolve_api().unwrap());
        assert!(r.value.listen);
        assert_eq!(r.value.bind[0].to_string(), "0.0.0.0:15432");
        assert_eq!(r.value.token, None);
        assert_eq!(r.source, LevelSource::Config);
    }

    #[test]
    fn api_toml_table_is_read() {
        let config: Config = toml::from_str(
            "[api]\nlisten = \"false\"\nbind = \"127.0.0.1:8080\"\nauth_token = \"t0k\"\n",
        )
        .unwrap();
        let r = with_env(&[], || config.resolve_api().unwrap());
        assert!(!r.value.listen);
        assert_eq!(r.value.bind[0].to_string(), "127.0.0.1:8080");
        assert_eq!(r.value.port(), 8080);
        assert_eq!(r.value.token.as_deref(), Some("t0k"));
        assert_eq!(r.source, LevelSource::Config);

        let config: Config = toml::from_str("[api]\nlisten = false\n").unwrap();
        let r = with_env(&[], || config.resolve_api().unwrap());
        assert!(!r.value.listen);
    }

    #[test]
    fn api_env_overrides() {
        let mut config = Config::default();
        config.api.auth_token = Some("from-file".to_string());
        let r = with_env(
            &[
                ("PUTIO_SYNC_API_LISTEN", "0"),
                ("PUTIO_SYNC_API_BIND", "127.0.0.1:1"),
                ("PUTIO_SYNC_API_TOKEN", "abc"),
            ],
            || config.resolve_api().unwrap(),
        );
        assert!(!r.value.listen);
        assert_eq!(r.value.bind[0].to_string(), "127.0.0.1:1");
        assert_eq!(r.value.token.as_deref(), Some("abc"));
        assert_eq!(r.source, LevelSource::Env);
    }

    #[test]
    fn api_invalid_values() {
        let config: Config = toml::from_str("[api]\nlisten = \"maybe\"\n").unwrap();
        assert_eq!(
            with_env(&[], || api_err(&config)),
            "invalid api.listen 'maybe': expected one of true, false, 1, 0"
        );

        assert_eq!(
            with_env(&[("PUTIO_SYNC_API_BIND", "nope")], || api_err(
                &Config::default()
            )),
            "invalid api.bind 'nope': missing :port (expected ip:port, [ipv6]:port, interface:port or tailnet:port)"
        );

        let mut config = Config::default();
        config.api.auth_token = Some("has space".to_string());
        let msg = with_env(&[], || api_err(&config));
        assert_eq!(
            msg,
            "invalid api.auth_token: expected a non-empty value without whitespace"
        );
        assert!(!msg.contains("has space"));

        assert_eq!(
            with_env(&[("PUTIO_SYNC_API_TOKEN", "")], || api_err(
                &Config::default()
            )),
            "invalid api.auth_token: expected a non-empty value without whitespace"
        );
    }

    #[test]
    fn bind_lists_from_file_and_env() {
        let config: Config = toml::from_str(
            "[api]\nbind = [\"eth0:8080\", \"[::1]:8080\", \"tailnet:9000\"]\n\
             [metrics]\nbind = [\"127.0.0.1:9100\", \"[::1]:9100\"]\n",
        )
        .unwrap();
        config.validate_binds().unwrap();
        let api = with_env(&[], || config.resolve_api().unwrap()).value;
        let names: Vec<String> = api.bind.iter().map(|b| b.to_string()).collect();
        assert_eq!(names, vec!["eth0:8080", "[::1]:8080", "tailnet:9000"]);
        assert_eq!(api.port(), 8080);
        let m = with_env(&[], || config.resolve_metrics().unwrap()).value;
        assert_eq!(m.bind.len(), 2);

        let env = with_env(&[("PUTIO_SYNC_API_BIND", "127.0.0.1:1, [::1]:1")], || {
            Config::default().resolve_api().unwrap()
        });
        assert_eq!(env.value.bind.len(), 2);
        assert_eq!(env.source, LevelSource::Env);

        // A single entry is written back as a plain string
        let mut c = Config::default();
        c.api.bind = Some(BindList::from_entries(vec!["127.0.0.1:2".into()]));
        let text = toml::to_string(&c).unwrap();
        assert!(text.contains("bind = \"127.0.0.1:2\""), "{text}");
    }

    #[test]
    fn tailnet_only_makes_one_tailnet_entry_per_port() {
        let mut config: Config =
            toml::from_str("[api]\nbind = [\"eth0:1\", \"127.0.0.1:1\", \"[::1]:2\"]\n").unwrap();
        let plain = with_env(&[], || config.api_listen().unwrap());
        assert_eq!(plain.len(), 3);
        config.tailscale.tailnet_only = true;
        let only = with_env(&[], || config.api_listen().unwrap());
        assert_eq!(
            only,
            vec![
                ListenAddr::Tailnet { port: 1 },
                ListenAddr::Tailnet { port: 2 }
            ]
        );
    }

    #[test]
    fn validate_refuses_bad_or_overlapping_binds() {
        let config: Config =
            toml::from_str("[api]\nbind = [\"0.0.0.0:1\", \"10.0.0.1:1\"]\n").unwrap();
        assert!(config
            .validate_binds()
            .unwrap_err()
            .to_string()
            .contains("overlap"));
        let config: Config = toml::from_str("[api]\nbind = \"0.0.0.0:9742\"\n").unwrap();
        assert!(config
            .validate_binds()
            .unwrap_err()
            .to_string()
            .contains("metrics.bind"));
        let mut config = config;
        config.metrics.enabled = false;
        assert!(config.validate_binds().is_ok());
        let config: Config = toml::from_str("[metrics]\nbind = []\n").unwrap();
        assert!(config.validate_binds().is_err());
    }

    #[test]
    fn api_token_never_in_debug() {
        let mut config = Config::default();
        config.api.auth_token = Some("super-secret-value".to_string());
        let settings = with_env(&[], || config.resolve_api().unwrap()).value;
        let debug = format!("{:?}", settings);
        assert!(!debug.contains("super-secret-value"));
        assert!(debug.contains("<set>"));
    }
}

#[cfg(test)]
mod save_keeping_file_credentials_tests {
    use super::*;

    fn written(path: &Path) -> Config {
        toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    /// In memory the API token may be the `PUTIO_SYNC_API_TOKEN` override; it must not
    /// reach a file that had none
    #[test]
    fn env_only_api_token_is_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let on_disk = Config {
            auth: Some(AuthConfig::with_token("file-putio".to_string())),
            ..Default::default()
        };
        on_disk.save(&path).unwrap();

        let mut memory = on_disk.clone();
        memory.api.auth_token = Some("env-api".to_string());
        memory.download.workers = 7;
        memory.save_keeping_file_credentials(&path).unwrap();

        let saved = written(&path);
        assert_eq!(saved.download.workers, 7);
        assert_eq!(saved.api.auth_token, None);
        assert!(!fs::read_to_string(&path).unwrap().contains("env-api"));
    }

    /// A UI save of the new key drops the old one and keeps the comments
    #[test]
    fn on_existing_save_drops_skip_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut on_disk = Config::default();
        on_disk.download.skip_existing = Some(false);
        on_disk.save(&path).unwrap();
        let contents = fs::read_to_string(&path)
            .unwrap()
            .replace("[download]\n", "[download]\n# my downloads\n");
        fs::write(&path, contents).unwrap();

        let mut memory: Config = written(&path);
        memory.download.set_on_existing(ExistingFile::Overwrite);
        memory.save_keeping_file_credentials(&path).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(
            contents.contains("on_existing = \"overwrite\""),
            "{contents}"
        );
        assert!(!contents.contains("skip_existing"), "{contents}");
        assert!(contents.contains("# my downloads"), "{contents}");
        assert_eq!(
            written(&path).download.on_existing(),
            ExistingFile::Overwrite
        );
    }

    #[test]
    fn file_tokens_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut on_disk = Config {
            auth: Some(AuthConfig::with_token("rotated-putio".to_string())),
            ..Default::default()
        };
        on_disk.api.auth_token = Some("file-api".to_string());
        on_disk.save(&path).unwrap();

        // Memory holds the env override and a Put.io token from before a re-login
        let mut memory = on_disk.clone();
        memory.api.auth_token = Some("env-api".to_string());
        memory.auth = Some(AuthConfig::with_token("old-putio".to_string()));
        memory.save_keeping_file_credentials(&path).unwrap();

        let saved = written(&path);
        assert_eq!(saved.api.auth_token.as_deref(), Some("file-api"));
        assert_eq!(
            saved.auth.and_then(|a| a.token).as_deref(),
            Some("rotated-putio")
        );
    }

    #[test]
    fn new_file_gets_no_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut memory = Config {
            auth: Some(AuthConfig::with_token("memory-putio".to_string())),
            ..Default::default()
        };
        memory.api.auth_token = Some("env-api".to_string());
        memory.save_keeping_file_credentials(&path).unwrap();

        let saved = written(&path);
        assert_eq!(saved.api.auth_token, None);
        assert!(saved.auth.is_none());
    }

    #[test]
    fn archive_directory_is_expanded_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = Config::default();
        config.watch.archive_directory = Some(PathBuf::from("~/processed"));
        config.save(&path).unwrap();
        let archive = Config::load(&path)
            .unwrap()
            .watch
            .archive_directory
            .unwrap();
        assert!(!archive.starts_with("~"), "{}", archive.display());
        assert!(archive.ends_with("processed"));
    }

    /// `Config::default()` as written by `save`, with comments, blank lines, odd
    /// spacing on the credentials, an unknown key and an unknown table added by hand
    fn hand_edited() -> String {
        let mut config = Config {
            auth: Some(AuthConfig::with_token("file-putio".to_string())),
            ..Default::default()
        };
        config.api.auth_token = Some("file-api".to_string());
        config.watch.archive_directory = Some(PathBuf::from("/srv/processed"));
        let text = toml::to_string_pretty(&config).unwrap();
        let text = text
            .replace(
                "token = \"file-putio\"",
                "token   =   \"file-putio\"   # from `login`",
            )
            .replace("[download]\n", "[download]\n# How many at once\n\n")
            .replace(
                "\nworkers = ",
                "\nfuture_knob = \"keep me\" # not known yet\nworkers = ",
            );
        format!("# My putio-sync config\n\n{text}\n# Trailing notes\n[extra]\nnote = \"unknown table\"\n")
    }

    /// `text` as `Config::load` gives it: paths expanded
    fn loaded(text: &str) -> Config {
        let mut config: Config = toml::from_str(text).unwrap();
        config.expand_paths().unwrap();
        config
    }

    fn write_hand_edited(path: &Path) -> String {
        let text = hand_edited();
        fs::write(path, &text).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        text
    }

    /// Only the changed key is rewritten: comments, blank lines, ordering, unknown keys
    /// and the credentials stay byte for byte
    #[test]
    fn only_changed_keys_are_edited() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let before = write_hand_edited(&path);
        assert!(before.contains("\nworkers = 2\n"), "{before}");

        let mut memory = loaded(&before);
        memory.api.auth_token = Some("env-api".to_string());
        memory.auth = Some(AuthConfig::with_token("old-putio".to_string()));
        memory.download.workers = 7;
        memory.save_keeping_file_credentials(&path).unwrap();

        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(after, before.replace("\nworkers = 2\n", "\nworkers = 7\n"));
    }

    #[test]
    fn unchanged_config_leaves_the_file_identical() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let before = write_hand_edited(&path);
        let memory = loaded(&before);
        memory.save_keeping_file_credentials(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    /// A key the config no longer serialises (an option set back to none) is removed,
    /// and a key new to the file is added to its table
    #[test]
    fn cleared_and_new_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let before = write_hand_edited(&path);
        let mut memory = loaded(&before);
        memory.watch.archive_directory = None;
        memory.notifications.webhook_url = Some("http://hook.local/x".to_string());
        memory.save_keeping_file_credentials(&path).unwrap();

        let after = fs::read_to_string(&path).unwrap();
        assert!(!after.contains("archive_directory"), "{after}");
        assert!(after.contains("# How many at once"), "{after}");
        let saved = written(&path);
        assert_eq!(
            saved.notifications.webhook_url.as_deref(),
            Some("http://hook.local/x")
        );
        assert_eq!(saved.watch.archive_directory, None);
    }

    /// `~` paths are expanded in memory; an unchanged one stays as written
    #[test]
    fn unexpanded_paths_are_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = Config::default();
        config.download.target_directory = PathBuf::from("~/Downloads/putio");
        config.save(&path).unwrap();
        let before = fs::read_to_string(&path).unwrap();

        let mut memory = Config::load(&path).unwrap();
        memory.download.workers = 9;
        memory.save_keeping_file_credentials(&path).unwrap();

        let after = fs::read_to_string(&path).unwrap();
        assert!(after.contains("\"~/Downloads/putio\""), "{after}");
        assert_eq!(written(&path).download.workers, 9);
        assert_eq!(
            after.lines().count(),
            before.lines().count(),
            "{before}\n---\n{after}"
        );
    }

    /// The write goes through a temp file and a rename: no leftovers, mode kept
    #[cfg(unix)]
    #[test]
    fn write_is_atomic_and_keeps_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let before = write_hand_edited(&path);
        let mut memory = loaded(&before);
        memory.download.workers = 3;
        memory.save_keeping_file_credentials(&path).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("config.toml")]);

        // A new file is created 0600 too
        let fresh = dir.path().join("sub").join("config.toml");
        memory.save_keeping_file_credentials(&fresh).unwrap();
        let mode = fs::metadata(&fresh).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    /// A file that no longer parses is not overwritten, and the error shows no content
    #[test]
    fn unreadable_file_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[api]\nauth_token = \"secret-value\n").unwrap();

        let err = Config::default()
            .save_keeping_file_credentials(&path)
            .unwrap_err()
            .to_string();
        assert!(!err.contains("secret-value"), "{err}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "[api]\nauth_token = \"secret-value\n"
        );
    }

    #[test]
    fn storage_defaults_when_section_missing() {
        let config: Config = toml::from_str("[api]\nport = 9000\n").unwrap();
        assert_eq!(config.storage, StorageConfig::default());
        assert_eq!(config.storage.warn_free_percent, 10);
        assert_eq!(config.storage.target_free_percent, 20);
        assert!(!config.storage.auto_free);
        assert_eq!(config.storage.run_at, "02:30");

        let partial: Config = toml::from_str("[storage]\nauto_free = true\n").unwrap();
        assert!(partial.storage.auto_free);
        assert_eq!(partial.storage.target_free_percent, 20);
    }

    #[test]
    fn storage_levels_and_time_are_validated() {
        let ok = |warn, target| StorageConfig {
            warn_free_percent: warn,
            target_free_percent: target,
            ..Default::default()
        };
        assert!(ok(10, 20).validate().is_ok());
        assert!(ok(1, 90).validate().is_ok());
        assert!(ok(0, 20).validate().is_err());
        assert!(ok(20, 20).validate().is_err());
        assert!(ok(30, 20).validate().is_err());
        assert!(ok(10, 91).validate().is_err());
        let bad_time = StorageConfig {
            run_at: "2:30".to_string(),
            ..Default::default()
        };
        let err = bad_time.validate().unwrap_err().to_string();
        assert!(err.contains("storage run_at"), "{err}");
        let mut config = Config::default();
        config.storage.target_free_percent = 5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn cleanup_defaults_when_section_missing() {
        let config: Config = toml::from_str("[api]\nport = 9000\n").unwrap();
        assert_eq!(config.cleanup, CleanupConfig::default());
        assert!(!config.cleanup.enabled);
        assert_eq!(config.cleanup.run_at, "03:00");
        assert_eq!(config.cleanup.min_age_hours, 24);
        assert!(config.cleanup.nested);
        assert_eq!(
            config.cleanup.run_time().unwrap(),
            chrono::NaiveTime::from_hms_opt(3, 0, 0).unwrap()
        );

        let partial: Config = toml::from_str("[cleanup]\nenabled = true\n").unwrap();
        assert!(partial.cleanup.enabled);
        assert_eq!(partial.cleanup.run_at, "03:00");
        assert!(partial.cleanup.nested);

        let flat: Config = toml::from_str("[cleanup]\nnested = false\n").unwrap();
        assert!(!flat.cleanup.nested);
    }

    #[test]
    fn cleanup_run_at_is_validated() {
        for good in ["00:00", "03:00", "23:59", "04:30"] {
            let mut config = Config::default();
            config.cleanup.run_at = good.to_string();
            assert!(config.validate().is_ok(), "{good}");
        }
        for bad in [
            "25:00", "3:00", "03:60", "0300", "", "ab:cd", "03:00:00", "-1:00",
        ] {
            let mut config = Config::default();
            config.cleanup.run_at = bad.to_string();
            let err = config.validate().unwrap_err().to_string();
            assert!(err.contains("run_at"), "{bad}: {err}");
        }
    }

    #[test]
    fn cleanup_min_age_is_bounded() {
        let mut config = Config::default();
        config.cleanup.min_age_hours = 0;
        assert!(config.validate().is_ok());
        config.cleanup.min_age_hours = MAX_CLEANUP_MIN_AGE_HOURS;
        assert!(config.validate().is_ok());
        config.cleanup.min_age_hours = MAX_CLEANUP_MIN_AGE_HOURS + 1;
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("min_age_hours"), "{err}");
    }

    #[test]
    fn cleanup_keep_patterns_are_checked() {
        let mut config = Config::default();
        config.cleanup.keep_patterns = vec!["Season*".into(), "Software/*".into()];
        assert!(config.validate().is_ok());
        config.cleanup.keep_patterns.push("\"Old".into());
        let err = config.validate().unwrap_err().to_string();
        assert!(
            err.contains("keep_patterns") && err.contains("unclosed"),
            "{err}"
        );
    }

    fn rule(path: &str, preset: Option<&str>) -> FolderRule {
        FolderRule {
            path: path.to_string(),
            preset: preset.map(str::to_string),
            ..FolderRule::default()
        }
    }

    /// `[[download.rules]]` and `[download.presets.x]`: added as tables, edited in place,
    /// removed; comments, other keys and the credentials stay
    #[test]
    fn rules_are_added_edited_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let before = write_hand_edited(&path);

        let mut memory = loaded(&before);
        memory.download.rules = vec![rule("Movies", Some("video")), rule("Music", Some("audio"))];
        memory.download.presets.insert(
            "comics".to_string(),
            Preset {
                only: Some(vec!["cbz".to_string()]),
                block: vec![],
            },
        );
        memory.save_keeping_file_credentials(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("[[download.rules]]").count(), 2, "{text}");
        assert!(text.contains("[download.presets.comics]"), "{text}");
        assert!(!text.contains("[download.presets]\n"), "{text}");
        assert!(text.contains("# How many at once"), "{text}");
        assert!(text.contains("# from `login`"), "{text}");
        assert_eq!(loaded(&text).download.rules, memory.download.rules);

        // A comment on the second rule survives an edit of the first
        fs::write(
            &path,
            text.replace("path = \"Music\"", "path = \"Music\" # mine"),
        )
        .unwrap();
        let mut memory = loaded(&fs::read_to_string(&path).unwrap());
        memory.download.rules[0].preset = Some("everything".to_string());
        memory.download.rules[0].delete_blocked = Some(true);
        memory.save_keeping_file_credentials(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("path = \"Music\" # mine"), "{text}");
        let saved = written(&path);
        assert_eq!(
            saved.download.rules[0].preset.as_deref(),
            Some("everything")
        );
        assert_eq!(saved.download.rules[0].delete_blocked, Some(true));
        assert_eq!(saved.auth.unwrap().token.as_deref(), Some("file-putio"));

        // Removing one, then all
        let mut memory = loaded(&text);
        memory.download.rules.remove(0);
        memory.save_keeping_file_credentials(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("[[download.rules]]").count(), 1, "{text}");
        assert_eq!(
            written(&path).download.rules,
            vec![rule("Music", Some("audio"))]
        );
        let mut memory = loaded(&text);
        memory.download.rules.clear();
        memory.download.presets.clear();
        memory.save_keeping_file_credentials(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("download.rules"), "{text}");
        assert!(!text.contains("comics"), "{text}");
        assert!(text.contains("# How many at once"), "{text}");
    }

    /// Rules are validated at load
    #[test]
    fn bad_rules_are_refused() {
        let mut config = Config::default();
        config.download.rules = vec![rule("Movies", Some("nope"))];
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("unknown preset 'nope'"), "{err}");
        config.download.rules = vec![rule("Movies", None), rule("/Movies", None)];
        assert!(config.validate().is_err());
        config.download.rules = vec![rule("Movies", Some("video"))];
        config
            .download
            .presets
            .insert("Bad".to_string(), Preset::default());
        assert!(config.validate().is_err());
    }

    /// Setup's save writes the credentials and keeps comments (feature 013)
    #[test]
    fn save_setup_writes_credentials_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "# my comment\n[cleanup]\nenabled = true # on\n[api]\nauth_token = \"keep\"\n",
        )
        .unwrap();
        let mut memory = loaded(&fs::read_to_string(&path).unwrap());
        memory.auth = Some(AuthConfig::with_token("putio-tok".to_string()));
        memory.download.preset = Some("video".to_string());
        memory.api.auth_token = Some("env-override".to_string());
        memory.save_setup(&path, false).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("# my comment") && text.contains("# on"),
            "{text}"
        );
        assert!(text.contains("auth_token = \"keep\""), "{text}");
        assert!(!text.contains("env-override"), "{text}");
        let saved = written(&path);
        assert_eq!(saved.auth.unwrap().token.as_deref(), Some("putio-tok"));
        assert_eq!(saved.download.preset.as_deref(), Some("video"));

        memory.api.auth_token = Some("new-api".to_string());
        memory.save_setup(&path, true).unwrap();
        assert_eq!(written(&path).api.auth_token.as_deref(), Some("new-api"));
    }

    #[test]
    fn save_setup_replaces_an_invalid_file_and_keeps_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "broken = = 1\n").unwrap();
        let memory = Config {
            auth: Some(AuthConfig::with_token("t".to_string())),
            ..Config::default()
        };
        memory.save_setup(&path, false).unwrap();
        assert_eq!(written(&path).auth.unwrap().token.as_deref(), Some("t"));
        assert_eq!(
            fs::read_to_string(dir.path().join("config.toml.bak")).unwrap(),
            "broken = = 1\n"
        );

        let new = dir.path().join("new.toml");
        memory.save_setup(&new, false).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&new).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    /// `[putio] app_id` and `[download] preset` (feature 013)
    #[test]
    fn putio_app_id_and_root_preset() {
        let mut config = Config::default();
        config.putio.app_id = Some("1234".to_string());
        config.download.preset = Some("video".to_string());
        let text = toml::to_string_pretty(&config).unwrap();
        assert!(text.contains("[putio]\napp_id = \"1234\""), "{text}");
        let mut config: Config = toml::from_str(&text).unwrap();
        assert_eq!(config.putio.app_id.as_deref(), Some("1234"));
        assert_eq!(config.download.preset.as_deref(), Some("video"));
        config.validate().unwrap();
        for bad in ["abc", "12a", "1234567890123"] {
            config.putio.app_id = Some(bad.to_string());
            assert!(config.validate().is_err(), "{bad:?}");
        }
        for good in [" 42 ", ""] {
            config.putio.app_id = Some(good.to_string());
            config.validate().unwrap();
        }

        let empty: Config = toml::from_str("").unwrap();
        assert_eq!(empty.putio, PutioConfig::default());
        assert_eq!(empty.download.preset, None);
        let text = toml::to_string_pretty(&empty).unwrap();
        assert!(!text.contains("[putio]"), "{text}");
        assert!(!text.contains("preset"), "{text}");

        let mut bad = Config::default();
        bad.download.preset = Some("nope".to_string());
        let err = bad.validate().unwrap_err().to_string();
        assert!(err.contains("[download] preset"), "{err}");
    }

    /// A file without `[cleanup]` gains the section on save; comments elsewhere stay
    #[test]
    fn cleanup_save_adds_section_and_keeps_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "# my settings\n[api]\n# not the default\nport = 9000\n",
        )
        .unwrap();

        let mut memory: Config = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        memory.cleanup.enabled = true;
        memory.cleanup.run_at = "04:30".to_string();
        memory.save_keeping_file_credentials(&path).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my settings"), "{text}");
        assert!(text.contains("# not the default"), "{text}");
        let saved = written(&path);
        assert!(saved.cleanup.enabled);
        assert_eq!(saved.cleanup.run_at, "04:30");
        assert_eq!(saved.cleanup.min_age_hours, 24);
    }
}

#[cfg(test)]
mod arr_tests {
    use super::*;

    /// Serialises tests that change the `[arr]` env overrides
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const VARS: [&str; 3] = [
        "PUTIO_SYNC_ARR_PASSWORD",
        "PUTIO_SYNC_RADARR_API_KEY",
        "PUTIO_SYNC_SONARR_API_KEY",
    ];

    fn with_env<R>(vars: &[(&str, &str)], f: impl FnOnce() -> R) -> R {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for v in VARS {
            std::env::remove_var(v);
        }
        for (k, v) in vars {
            std::env::set_var(k, v);
        }
        let out = f();
        for v in VARS {
            std::env::remove_var(v);
        }
        out
    }

    const TWO_SERVERS: &str = r#"
[arr]
enabled = true
password = "file-pass"

[[arr.servers]]
kind = "sonarr"
url = "http://nas:8989/"
api_key = "file-sonarr-key"
category = "/tv/"

[[arr.servers]]
kind = "radarr"
url = "https://nas:7878"
category = "movies"
"#;

    #[test]
    fn defaults_are_off() {
        let config = Config::default();
        assert!(!config.arr.enabled);
        assert_eq!(config.arr.username, "putio-sync");
        let r = with_env(&[], || config.resolve_arr().unwrap());
        assert!(!r.enabled);
        assert_eq!(r.password, None);
        assert!(r.servers.is_empty());
    }

    #[test]
    fn file_values_are_normalised() {
        let config: Config = toml::from_str(TWO_SERVERS).unwrap();
        let r = with_env(&[], || config.resolve_arr().unwrap());
        assert!(r.enabled);
        assert_eq!(r.password.as_deref(), Some("file-pass"));
        assert_eq!(r.servers[0].url, "http://nas:8989");
        assert_eq!(r.servers[0].category.as_deref(), Some("tv"));
        assert_eq!(r.servers[0].name.as_deref(), Some("Sonarr"));
        assert_eq!(r.servers[1].name.as_deref(), Some("Radarr"));
        assert_eq!(r.servers[0].api_key.as_deref(), Some("file-sonarr-key"));
        assert_eq!(r.servers[1].api_key, None);
        assert_eq!(r.server_for("/tv").map(|s| s.kind), Some(ArrKind::Sonarr));
        assert!(r.server_for("music").is_none());
    }

    #[test]
    fn env_beats_file() {
        let config: Config = toml::from_str(TWO_SERVERS).unwrap();
        let r = with_env(
            &[
                ("PUTIO_SYNC_ARR_PASSWORD", "env-pass"),
                ("PUTIO_SYNC_RADARR_API_KEY", "env-radarr"),
                ("PUTIO_SYNC_SONARR_API_KEY", ""),
            ],
            || config.resolve_arr().unwrap(),
        );
        assert_eq!(r.password.as_deref(), Some("env-pass"));
        assert_eq!(r.servers[1].api_key.as_deref(), Some("env-radarr"));
        // An empty variable counts as unset
        assert_eq!(r.servers[0].api_key.as_deref(), Some("file-sonarr-key"));
    }

    #[test]
    fn debug_hides_secrets() {
        let config: Config = toml::from_str(TWO_SERVERS).unwrap();
        let shown = format!(
            "{:?} {:?}",
            config,
            with_env(&[], || config.resolve_arr().unwrap())
        );
        assert!(!shown.contains("file-pass"));
        assert!(!shown.contains("file-sonarr-key"));
        assert!(shown.contains("<set>"));
        assert!(shown.contains("<unset>"));
    }

    fn arr_error(toml_text: &str) -> String {
        let config: Config = toml::from_str(toml_text).unwrap();
        match config.validate() {
            Err(CoreError::ConfigError(msg)) => msg,
            other => panic!("expected a config error, got {:?}", other),
        }
    }

    #[test]
    fn bad_servers_are_rejected() {
        let server = |url: &str| format!("[[arr.servers]]\nkind = \"sonarr\"\nurl = \"{url}\"\n");
        assert!(arr_error(&server("nas:8989")).contains("arr server url"));
        assert!(arr_error(&server("ftp://nas")).contains("arr server url"));
        let twice = format!("{}{}", server("http://a:8989"), server("http://A:8989/"));
        assert!(arr_error(&twice).contains("used twice"));
        assert!(arr_error("[arr]\nusername = \"a:b\"\n").contains("arr username"));
    }

    /// Feature 018: the typed category is legacy; an invalid one no longer stops loading
    #[test]
    fn legacy_category_is_optional() {
        let config: Config = toml::from_str(
            "[[arr.servers]]\nkind = \"radarr\"\nurl = \"http://a\"\ncategory = \"../x\"\n\n[[arr.servers]]\nkind = \"radarr\"\nurl = \"http://b\"\n",
        )
        .unwrap();
        config.validate().unwrap();
        let r = with_env(&[], || config.resolve_arr().unwrap());
        assert_eq!(r.servers[0].category, None);
        assert_eq!(r.servers[1].category, None);
        assert_eq!(r.servers[0].name.as_deref(), Some("Radarr"));
        assert_eq!(r.servers[1].name.as_deref(), Some("Radarr (2)"));
    }

    #[test]
    fn env_flags() {
        let config: Config = toml::from_str(TWO_SERVERS).unwrap();
        let servers = &config.arr.servers;
        with_env(&[("PUTIO_SYNC_SONARR_API_KEY", "k")], || {
            assert!(!ArrSettings::password_from_env());
            assert!(ArrSettings::api_key_from_env(servers, 0));
            assert!(!ArrSettings::api_key_from_env(servers, 1));
            assert!(!ArrSettings::api_key_from_env(servers, 9));
        });
        with_env(&[("PUTIO_SYNC_ARR_PASSWORD", "p")], || {
            assert!(ArrSettings::password_from_env());
            assert!(!ArrSettings::api_key_from_env(servers, 0));
        });
    }

    /// Feature 018: `[arr]` is written on its own; the rest of the file is untouched
    #[test]
    fn save_arr_section_edits_only_arr() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let other = "# keep me\n[cleanup]\nenabled = true # on\n";
        fs::write(&path, format!("{other}{TWO_SERVERS}")).unwrap();

        let mut arr: ArrConfig = toml::from_str::<Config>(TWO_SERVERS).unwrap().arr;
        arr.enabled = false;
        arr.password = Some("new-pass".to_string());
        arr.servers.remove(1);
        arr.servers[0].name = Some("TV".to_string());
        Config::save_arr_section(&path, &arr).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(other), "{text}");
        let saved: Config = toml::from_str(&text).unwrap();
        assert_eq!(saved.arr, arr);
        assert!(text.contains("api_key = \"file-sonarr-key\""));
        assert!(!text.contains("radarr"));
    }

    #[test]
    fn save_arr_section_creates_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let arr = ArrConfig {
            enabled: true,
            password: Some("p".to_string()),
            ..ArrConfig::default()
        };
        Config::save_arr_section(&path, &arr).unwrap();
        let saved: Config = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.arr, arr);
    }

    #[test]
    fn save_arr_section_refuses_a_bad_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "not = [toml").unwrap();
        let err = Config::save_arr_section(&path, &ArrConfig::default()).unwrap_err();
        assert!(err.to_string().contains("settings not saved"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "not = [toml");
    }

    #[test]
    fn categories_normalise() {
        assert_eq!(normalize_arr_category("/a/b/").as_deref(), Some("a/b"));
        assert_eq!(normalize_arr_category("").as_deref(), Some(""));
        assert_eq!(normalize_arr_category("a/./b"), None);
        assert_eq!(normalize_arr_category("a\\b"), None);
    }

    /// The settings routes never write `[arr]`: its secrets stay as on disk
    #[test]
    fn settings_save_keeps_arr_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, TWO_SERVERS).unwrap();

        let mut memory: Config = toml::from_str(TWO_SERVERS).unwrap();
        memory.arr.password = Some("memory-pass".to_string());
        memory.arr.servers.clear();
        memory.download.workers = 9;
        memory.save_keeping_file_credentials(&path).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("workers = 9"));
        assert!(!text.contains("memory-pass"));
        assert!(text.contains(TWO_SERVERS.trim()));
    }
}

#[cfg(test)]
mod precheck_tests {
    use super::*;

    /// Serialises tests that change the `[precheck]` env overrides
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const VARS: [&str; 2] = [
        "PUTIO_SYNC_PRECHECK_ENABLED",
        "PUTIO_SYNC_PRECHECK_MAGNET_LOOKUP",
    ];

    fn with_env<R>(vars: &[(&str, &str)], f: impl FnOnce() -> R) -> R {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for v in VARS {
            std::env::remove_var(v);
        }
        for (k, v) in vars {
            std::env::set_var(k, v);
        }
        let out = f();
        for v in VARS {
            std::env::remove_var(v);
        }
        out
    }

    fn error(text: &str) -> String {
        let config: Config = toml::from_str(text).unwrap();
        match config.precheck.validate(&config.watch.paths) {
            Err(CoreError::ConfigError(msg)) => msg,
            other => panic!("expected a config error, got {:?}", other),
        }
    }

    #[test]
    fn defaults() {
        let p = Config::default().precheck;
        assert!(p.enabled);
        assert!(!p.dry_run);
        assert_eq!(p.min_size_mb, 50);
        assert!(!p.allow_archives);
        assert!(p.magnet_lookup);
        assert_eq!(p.lookup_timeout_secs, 10);
        assert_eq!(p.caches, vec![DEFAULT_PRECHECK_CACHE.to_string()]);
        assert_eq!(p.hold_magnet_secs, 300);
        assert_eq!(p.hold_retry_secs, 30);
        assert_eq!(p.first_lookup_secs, 3);
        assert!(p.holding());
        assert!(p.validate(&[]).is_ok());
    }

    #[test]
    fn overrides_parse() {
        let config: Config = toml::from_str(
            "[precheck]\nmin_size_mb = 80\n\n[precheck.categories.software]\nenabled = false\n\n[precheck.watch.\"/watch/tv\"]\nmin_size_mb = 100\n",
        )
        .unwrap();
        assert_eq!(config.precheck.min_size_mb, 80);
        assert_eq!(config.precheck.categories["software"].enabled, Some(false));
        assert_eq!(config.precheck.watch["/watch/tv"].min_size_mb, Some(100));
        // A watch key that is not a watch path only warns
        assert!(config.precheck.validate(&[]).is_ok());
    }

    #[test]
    fn validation() {
        assert!(error("[precheck]\nmin_size_mb = 100001\n").contains("0..=100000"));
        assert!(error("[precheck.categories.tv]\nmin_size_mb = 200000\n").contains("0..=100000"));
        assert!(error("[precheck]\nlookup_timeout_secs = 0\n").contains("1..=30"));
        assert!(error("[precheck]\nlookup_timeout_secs = 31\n").contains("1..=30"));
        assert!(error("[precheck]\nhold_magnet_secs = 3601\n").contains("0..=3600"));
        assert!(error("[precheck]\nhold_retry_secs = 9\n").contains("10..=600"));
        assert!(error("[precheck]\nhold_retry_secs = 601\n").contains("10..=600"));
        assert!(error("[precheck]\nfirst_lookup_secs = 0\n").contains("1..=10"));
        assert!(
            error("[precheck]\nlookup_timeout_secs = 5\nfirst_lookup_secs = 6\n").contains("1..=5")
        );
        let off: Config = toml::from_str("[precheck]\nhold_magnet_secs = 0\n").unwrap();
        assert!(off.validate().is_ok());
        assert!(!off.precheck.holding());
        assert!(error("[precheck]\ncaches = [\"ftp://x/{HASH}\"]\n").contains("precheck cache"));
        assert!(error("[precheck]\ncaches = [\"https://x/a.torrent\"]\n").contains("{HASH}"));
        let ok: Config =
            toml::from_str("[precheck]\ncaches = [\"https://c.example/{hash}.torrent\"]\n")
                .unwrap();
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn env_overrides() {
        let config = Config::default();
        let p = with_env(
            &[
                ("PUTIO_SYNC_PRECHECK_ENABLED", "0"),
                ("PUTIO_SYNC_PRECHECK_MAGNET_LOOKUP", ""),
            ],
            || config.resolve_precheck().unwrap(),
        );
        assert!(!p.enabled);
        assert!(p.magnet_lookup);
        let err = with_env(&[("PUTIO_SYNC_PRECHECK_MAGNET_LOOKUP", "no")], || {
            config.resolve_precheck().unwrap_err().to_string()
        });
        assert!(err.contains("precheck.magnet_lookup 'no'"), "{err}");
    }

    /// FR-006: the check rejects archives; the sync's block list stays as it was
    #[test]
    fn blocked_extensions_unchanged() {
        for ext in ["7z", "zip", "rar"] {
            assert!(!DEFAULT_BLOCKED_EXTENSIONS.contains(&ext), "{ext}");
        }
        assert_eq!(DEFAULT_BLOCKED_EXTENSIONS.len(), 15);
    }

    #[test]
    fn tailscale_config_defaults_validation_and_allow_list() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.tailscale, TailscaleConfig::default());
        assert!(!config.tailscale.tailnet_only);
        assert!(!config.tailscale.identity_sign_in);

        let config: Config = toml::from_str(
            "[tailscale]\nidentity_sign_in = true\nallowed_users = [\"Alice@Example.com\"]\nsocket = \"/tmp/ts.sock\"\n",
        )
        .unwrap();
        assert!(config.tailscale.validate().is_ok());
        assert!(config.tailscale.allows(" alice@example.com "));
        assert!(!config.tailscale.allows("mallory@example.com"));
        assert!(!config.tailscale.allows(""));
        assert_eq!(
            config.tailscale.socket_path(),
            PathBuf::from("/tmp/ts.sock")
        );
        assert_eq!(
            TailscaleConfig::default().socket_path(),
            PathBuf::from(DEFAULT_TAILSCALE_SOCKET)
        );

        let bad = TailscaleConfig {
            allowed_users: vec!["has space@x".into()],
            ..Default::default()
        };
        assert!(bad.validate().is_err());
        let bad = TailscaleConfig {
            allowed_users: vec!["  ".into()],
            ..Default::default()
        };
        assert!(bad.validate().is_err());
        let bad = TailscaleConfig {
            socket: Some(" ".into()),
            ..Default::default()
        };
        assert!(bad.validate().is_err());
    }
}

/// Environment variables that can hold a secret, masked in captured log lines (feature 047)
const SECRET_VARS: [&str; 9] = [
    "PUTIO_TOKEN",
    "PUTIO_SYNC_PUTIO_TOKEN",
    "PUTIO_SYNC_API_TOKEN",
    "PUTIO_SYNC_ARR_PASSWORD",
    "PUTIO_SYNC_RADARR_API_KEY",
    "PUTIO_SYNC_SONARR_API_KEY",
    "PUTIO_CLIENT_SECRET",
    "PUTIO_SYNC_TRANSCRIPTION_API_KEY",
    "PUTIO_SYNC_TRANSLATION_API_KEY",
];

impl Config {
    /// Every secret value in the config and the environment, for
    /// [`crate::log_redact::set_secrets`]: the Put.io token and password, the API token, the
    /// *arr password and API keys, the notification webhook, hook URLs, headers and env,
    /// and plugin secrets.
    pub fn secret_values(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut add = |value: Option<&String>| {
            if let Some(v) = value.filter(|v| !v.trim().is_empty()) {
                out.push(v.clone());
            }
        };
        if let Some(auth) = &self.auth {
            add(auth.token.as_ref());
            add(auth.password.as_ref());
        }
        add(self.api.auth_token.as_ref());
        add(self.arr.password.as_ref());
        for server in &self.arr.servers {
            add(server.api_key.as_ref());
        }
        add(self.notifications.webhook_url.as_ref());
        add(Some(&self.notifications.smtp.password));
        for v in self.sign_in.secret_values() {
            add(Some(&v));
        }
        for hook in &self.hooks {
            add(hook.url.as_ref());
            hook.headers.values().for_each(|v| add(Some(v)));
            hook.env.values().for_each(|v| add(Some(v)));
        }
        for plugin in &self.plugins {
            plugin.secrets.values().for_each(|v| add(Some(v)));
        }
        add(Some(&self.subtitles.transcription.api_key));
        for v in self.translation.secret_values() {
            add(Some(&v));
        }
        for var in SECRET_VARS {
            add(std::env::var(var).ok().as_ref());
        }
        out
    }
}

#[cfg(test)]
mod secret_values_tests {
    use super::*;

    #[test]
    fn every_secret_source_is_listed() {
        let config: Config = toml::from_str(
            r#"
            [auth]
            token = "s-putio-token"
            password = "s-putio-password"
            [api]
            auth_token = "s-api-token"
            [arr]
            password = "s-arr-password"
            [[arr.servers]]
            kind = "sonarr"
            url = "http://nas:8989"
            api_key = "s-arr-key"
            [[hooks]]
            name = "h"
            events = ["*"]
            url = "https://hook.lan/s-hook-url"
            headers = { Authorization = "s-hook-header" }
            [[hooks]]
            name = "c"
            events = ["*"]
            command = ["/bin/true"]
            env = { KEY = "s-hook-env" }
            [[plugins]]
            name = "p"
            secrets = { key = "s-plugin-secret" }
            [subtitles.transcription]
            api_key = "s-transcription-key"
            [translation.chat]
            api_key = "s-translation-key"
            [translation.callback.headers]
            Authorization = "s-translation-header"
            [notifications.smtp]
            password = "s-smtp-password"
            "#,
        )
        .unwrap();
        let mut config = config;
        config.notifications.webhook_url = Some("https://ha.lan/s-webhook".to_string());
        let values = config.secret_values();
        for expected in [
            "s-putio-token",
            "s-putio-password",
            "s-api-token",
            "https://ha.lan/s-webhook",
            "s-arr-password",
            "s-arr-key",
            "s-transcription-key",
            "s-translation-key",
            "s-translation-header",
            "https://hook.lan/s-hook-url",
            "s-hook-header",
            "s-hook-env",
            "s-plugin-secret",
            "s-smtp-password",
        ] {
            assert!(values.iter().any(|v| v == expected), "{expected} missing");
        }
    }

    #[test]
    fn secret_env_vars_are_listed() {
        let previous = std::env::var("PUTIO_CLIENT_SECRET").ok();
        std::env::set_var("PUTIO_CLIENT_SECRET", "s-client-secret");
        let config: Config = toml::from_str("").unwrap();
        let values = config.secret_values();
        match previous {
            Some(v) => std::env::set_var("PUTIO_CLIENT_SECRET", v),
            None => std::env::remove_var("PUTIO_CLIENT_SECRET"),
        }
        assert!(values.iter().any(|v| v == "s-client-secret"));
    }
}

#[cfg(test)]
mod dlna_tests {
    use super::*;

    /// Serialises tests that change the `[dlna]` env overrides
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const VARS: [&str; 3] = [
        "PUTIO_SYNC_DLNA_ENABLED",
        "PUTIO_SYNC_DLNA_NAME",
        "PUTIO_SYNC_DLNA_PORT",
    ];

    fn with_env<R>(vars: &[(&str, &str)], f: impl FnOnce() -> R) -> R {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for v in VARS {
            std::env::remove_var(v);
        }
        for (k, v) in vars {
            std::env::set_var(k, v);
        }
        let out = f();
        for v in VARS {
            std::env::remove_var(v);
        }
        out
    }

    fn parse(text: &str) -> Config {
        toml::from_str(text).unwrap()
    }

    fn error(text: &str) -> String {
        match parse(text).validate() {
            Err(CoreError::ConfigError(msg)) => msg,
            other => panic!("expected a config error, got {:?}", other),
        }
    }

    #[test]
    fn defaults_are_off_and_share_downloads() {
        let d = Config::default().dlna;
        assert!(!d.enabled);
        assert_eq!(d.name, "putio-sync");
        assert_eq!(d.port, 8200);
        assert!(d.bind.entries().is_empty());
        assert_eq!(d.roots, vec!["downloads".to_string()]);
        assert_eq!(d.putio_folder, None);
        assert!(Config::default().validate().is_ok());
        assert_eq!(parse("[dlna]\nenabled = true\n").dlna.roots, d.roots);
    }

    #[test]
    fn name_is_trimmed_defaulted_and_cut() {
        let name = |n: &str| DlnaConfig {
            name: n.to_string(),
            ..DlnaConfig::default()
        };
        assert_eq!(name("  NAS putio-sync ").effective_name(), "NAS putio-sync");
        assert_eq!(name("   ").effective_name(), "putio-sync");
        assert_eq!(name("").effective_name(), "putio-sync");
        let long = "é".repeat(100);
        assert_eq!(name(&long).effective_name().chars().count(), 64);
    }

    #[test]
    fn port_rules() {
        assert!(error("[dlna]\nport = 0\n").contains("dlna.port"));
        assert!(error("[dlna]\nport = 1900\n").contains("discovery"));
        let api = error("[dlna]\nenabled = true\nport = 15432\n");
        assert!(api.contains("dlna.port") && api.contains("api"), "{api}");
        let metrics = error("[dlna]\nenabled = true\nport = 9742\n");
        assert!(metrics.contains("metrics"), "{metrics}");
        // Off, the port is not compared with the others
        assert!(parse("[dlna]\nport = 15432\n").validate().is_ok());
        assert!(parse("[dlna]\nenabled = true\nport = 8300\n")
            .validate()
            .is_ok());
    }

    #[test]
    fn root_rules() {
        let ok = |roots: &str| {
            parse(&format!(
                "[watch]\nenabled = true\npaths = [\"/tmp\"]\nrefresh_cycle = 60\n[dlna]\nenabled = true\nroots = {roots}\nputio_folder = 5\n"
            ))
            .validate()
        };
        assert!(ok(r#"["downloads", "cat:radarr", "watch:/tmp", "putio"]"#).is_ok());
        for bad in [
            r#"["movies"]"#,
            r#"["cat:"]"#,
            r#"["cat:a/b"]"#,
            r#"["cat:.."]"#,
            r#"["watch:"]"#,
            r#"["downloads", "downloads"]"#,
        ] {
            match ok(bad) {
                Err(CoreError::ConfigError(msg)) => assert!(msg.contains("dlna.roots"), "{msg}"),
                other => panic!("{bad}: expected an error, got {other:?}"),
            }
        }
        // A watch root must be one of [watch] paths
        match ok(r#"["watch:/elsewhere"]"#) {
            Err(CoreError::ConfigError(msg)) => assert!(msg.contains("[watch] paths"), "{msg}"),
            other => panic!("expected an error, got {other:?}"),
        }
        // Enabled needs at least one root
        let msg = error("[dlna]\nenabled = true\nroots = []\n");
        assert!(msg.contains("at least one"), "{msg}");
        assert!(parse("[dlna]\nroots = []\n").validate().is_ok());
    }

    #[test]
    fn putio_root_defaults_to_the_synced_folder() {
        // Feature 022 (stream mode): no folder chosen is the account root
        let d = parse("[dlna]\nroots = [\"putio\"]\n");
        assert!(d.validate().is_ok());
        assert_eq!(d.dlna.putio_root(), 0);
        let d = parse("[dlna]\nroots = [\"putio\"]\nputio_folder = 7\n");
        assert!(d.validate().is_ok());
        assert_eq!(d.dlna.putio_root(), 7);
        assert!(error("[dlna]\nputio_folder = -3\n").contains("dlna.putio_folder"));
    }

    #[test]
    fn bind_entries_are_checked() {
        assert!(
            parse("[dlna]\nbind = [\"192.168.1.5:8200\", \"eth0:8200\"]\n")
                .validate()
                .is_ok()
        );
        let msg = error("[dlna]\nbind = [\"nonsense\"]\n");
        assert!(msg.contains("dlna.bind"), "{msg}");
    }

    #[test]
    fn env_overrides_apply() {
        let c = parse("[dlna]\nname = \"file\"\n");
        let r = with_env(
            &[
                ("PUTIO_SYNC_DLNA_ENABLED", "1"),
                ("PUTIO_SYNC_DLNA_NAME", "from env"),
                ("PUTIO_SYNC_DLNA_PORT", "8300"),
            ],
            || c.resolve_dlna().unwrap(),
        );
        assert!(r.enabled);
        assert_eq!(r.name, "from env");
        assert_eq!(r.port, 8300);
        let r = with_env(&[("PUTIO_SYNC_DLNA_NAME", "")], || {
            c.resolve_dlna().unwrap()
        });
        assert_eq!(r.name, "file");
        assert!(!r.enabled);
    }

    #[test]
    fn bad_env_values_are_errors() {
        let c = Config::default();
        for (var, value) in [
            ("PUTIO_SYNC_DLNA_ENABLED", "maybe"),
            ("PUTIO_SYNC_DLNA_PORT", "eighty"),
            ("PUTIO_SYNC_DLNA_PORT", "1900"),
        ] {
            let err = with_env(&[(var, value)], || c.resolve_dlna());
            assert!(err.is_err(), "{var}={value}");
        }
    }

    #[test]
    fn section_round_trips_through_toml() {
        let c = parse(
            "[dlna]\nenabled = true\nname = \"TV\"\nport = 8300\nbind = [\"eth0:8300\"]\nroots = [\"downloads\", \"putio\"]\nputio_folder = 42\n",
        );
        let text = toml::to_string(&c).unwrap();
        let again: Config = toml::from_str(&text).unwrap();
        assert_eq!(again.dlna, c.dlna);
    }
}
