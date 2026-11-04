//! Settings management routes
//!
//! Provides REST API endpoints for system settings:
//! - GET  /settings - Get current settings
//! - PUT  /settings - Update settings
//! - GET  /settings/watcher - Get file watcher configuration
//! - PUT  /settings/watcher - Update file watcher configuration
//! - GET  /settings/cleanup - Get the Put.io empty-folder cleanup settings
//! - PUT  /settings/cleanup - Update the Put.io empty-folder cleanup settings
//! - GET  /settings/storage - Get the Put.io storage levels and automatic free-up
//! - PUT  /settings/storage - Update the Put.io storage levels and automatic free-up
//! - GET  /settings/precheck - Get the release check settings and what they come to
//! - PUT  /settings/precheck - Update the release check settings
//! - GET  /settings/tailscale - Get the Tailscale settings (feature 036)
//! - PUT  /settings/tailscale - Update the Tailscale settings
//! - POST /settings/{group}/preview - What a PUT of that body would do, without saving
//!   (feature 015; the assistant, export and import routes are in `assistant.rs`)

use super::assistant::{self, Preview};
use crate::error::ApiError;
use crate::models::SuccessResponse;
use axum::{
    extract::{rejection::JsonRejection, State},
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Re-export AppState from parent
pub use crate::AppState;
use crate::Engine;
use putio_sync_core::assistant::Change;
use putio_sync_core::config::{
    CleanupConfig, Config, NotificationBackend, PrecheckConfig, PrecheckOverride, ProgressStyle,
    SortOrder, StorageConfig, TailscaleConfig,
};
use putio_sync_core::error::CoreError;
use putio_sync_core::logging::ConsoleFormat;
use putio_sync_precheck::policy::Key;
use putio_sync_precheck::rule::Policy;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// A malformed or mistyped body is a 400 with a JSON `error`, like a failed validation
pub(crate) fn body<T>(body: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    body.map(|Json(value)| value)
        .map_err(|rejection| ApiError::BadRequest(rejection.body_text()))
}

pub(crate) fn invalid(e: CoreError) -> ApiError {
    match e {
        CoreError::ConfigError(msg) => ApiError::BadRequest(msg),
        other => ApiError::BadRequest(other.to_string()),
    }
}

/// Apply `change` to a copy of the config and keep it only when the whole config still
/// validates and, with a config file, was written to it. A refused PUT therefore changes
/// nothing in memory or on disk.
///
/// The file keeps its own credentials (`Config::save_keeping_file_credentials`): the API
/// token in memory may come from `PUTIO_SYNC_API_TOKEN` and must not be written.
///
/// Returns the names of the fields of `view` (the group's GET body) that changed.
pub(crate) async fn update<V: Serialize>(
    state: &AppState,
    view: fn(&Config) -> V,
    change: impl FnOnce(&mut Config) -> Result<(), ApiError>,
) -> Result<Vec<String>, ApiError> {
    let mut config = state.config.write().await;
    let mut candidate = config.clone();
    change(&mut candidate)?;
    candidate.validate().map_err(invalid)?;
    if let Some(ref path) = state.config_path {
        candidate
            .save_keeping_file_credentials(path.as_ref())
            .map_err(|e| ApiError::InternalError(format!("Failed to persist config: {}", e)))?;
    }
    let changed = changed_fields(&view(&config), &view(&candidate));
    state.doctor.configure(&candidate);
    *config = candidate;
    Ok(changed)
}

/// Top-level fields that differ between two serialised group bodies
fn changed_fields<V: Serialize>(before: &V, after: &V) -> Vec<String> {
    field_changes(before, after, &[])
        .into_iter()
        .map(|c| c.key)
        .collect()
}

/// Top-level fields that differ between two serialised group bodies, with their values;
/// `live` lists the fields that apply without a restart
fn field_changes<V: Serialize>(before: &V, after: &V, live: &[&str]) -> Vec<Change> {
    let (Ok(serde_json::Value::Object(before)), Ok(serde_json::Value::Object(after))) =
        (serde_json::to_value(before), serde_json::to_value(after))
    else {
        return Vec::new();
    };
    after
        .iter()
        .filter(|(name, value)| before.get(*name) != Some(*value))
        .map(|(name, value)| Change {
            key: name.clone(),
            before: before.get(name).cloned().filter(|v| !v.is_null()),
            after: Some(value.clone()).filter(|v| !v.is_null()),
            live: live.contains(&name.as_str()),
        })
        .collect()
}

/// What a save of `change` would do (feature 015): the same apply and validation as
/// [`update`] on a copy, without writing anything
async fn preview<V: Serialize>(
    state: &AppState,
    view: fn(&Config) -> V,
    (live, when): (&[&str], &'static str),
    change: impl FnOnce(&mut Config) -> Result<(), ApiError>,
) -> Json<Preview> {
    let config = state.config.read().await.clone();
    let mut candidate = config.clone();
    if let Err(e) = change(&mut candidate).and_then(|()| candidate.validate().map_err(invalid)) {
        return Json(Preview::refused(e, when));
    }
    Json(Preview {
        valid: true,
        changes: field_changes(&view(&config), &view(&candidate), live),
        when,
        risks: putio_sync_core::assistant::risks(&candidate),
        ..Preview::default()
    })
}

/// One settings group, for the assistant route: its defaults and what applies live
pub(crate) struct GroupInfo {
    pub key: &'static str,
    pub defaults: serde_json::Value,
    pub live: &'static [&'static str],
    pub when: &'static str,
}

/// The seven groups. Defaults are the default config's, with the current download
/// folder kept: resetting it is almost never what a user means.
pub(crate) fn groups(state: &AppState, current: &Config) -> Vec<GroupInfo> {
    let mut defaults = Config::default();
    let _ = defaults.expand_paths();
    defaults.download.target_directory = current.download.target_directory.clone();
    let json = |v: serde_json::Result<serde_json::Value>| v.unwrap_or_default();
    let group = |key, defaults, (live, when)| GroupInfo {
        key,
        defaults,
        live,
        when,
    };
    vec![
        group(
            "download",
            json(serde_json::to_value(download_view(&defaults))),
            live_download(state),
        ),
        group(
            "watcher",
            json(serde_json::to_value(watcher_view(&defaults))),
            live_watcher(state),
        ),
        group(
            "cleanup",
            json(serde_json::to_value(cleanup_view(&defaults))),
            live_cleanup(state),
        ),
        group(
            "storage",
            json(serde_json::to_value(storage_view(&defaults))),
            live_storage(state),
        ),
        group(
            "tailscale",
            json(serde_json::to_value(tailscale_view(&defaults))),
            live_tailscale(state),
        ),
        group(
            "logging",
            json(serde_json::to_value(logging_view(&defaults))),
            live_logging(state),
        ),
        group(
            "notifications",
            json(serde_json::to_value(notification_view(&defaults))),
            (&[], ""),
        ),
        group(
            "precheck",
            json(serde_json::to_value(precheck_view(&defaults))),
            (&[], ""),
        ),
    ]
}

/// Hand the running watcher its settings: a watcher shared with the daemon (it started
/// with a watch folder) reads paths, parent folder and archive folder at its next scan.
/// `enabled` and `delete_after_upload` are read by the daemon's watch loop only at startup.
pub(crate) async fn push_watcher(state: &AppState) {
    use putio_sync_file_manager::watcher::WatcherConfig as FileWatcherConfig;
    use std::time::Duration;

    if let Some(ref file_watcher) = state.file_watcher {
        let watch = state.config.read().await.watch.clone();
        file_watcher.update_config(FileWatcherConfig {
            watch_dirs: watch.paths,
            parent_id: watch.parent_folder_id,
            interval: Duration::from_secs(watch.refresh_cycle.max(1) as u64),
            archive_dir: watch.archive_directory,
            delete_after_upload: watch.delete_after_upload,
        });
    }
}

/// The save message (FR-018): which changed fields apply now and which need a restart.
///
/// `live` lists the fields this engine applies without a restart and `when` says when
/// they take effect; every other changed field needs a restart. Fields a note explains
/// are taken out of `changed` by the caller.
fn saved_message(
    group: &str,
    changed: &[String],
    live: &[&str],
    when: &str,
    notes: &[String],
) -> String {
    if changed.is_empty() && notes.is_empty() {
        return format!("{group} settings saved; nothing changed.");
    }
    let (now, later): (Vec<&str>, Vec<&str>) = changed
        .iter()
        .map(String::as_str)
        .partition(|field| live.contains(field));
    let mut parts = vec![format!("{group} settings saved.")];
    if !now.is_empty() {
        parts.push(format!("{when}: {}.", now.join(", ")));
    }
    if !later.is_empty() {
        parts.push(format!("Needs a restart: {}.", later.join(", ")));
    }
    parts.extend(notes.iter().cloned());
    parts.join(" ")
}

/// Remove `field` from `changed`; true when it was there
fn take(changed: &mut Vec<String>, field: &str) -> bool {
    let before = changed.len();
    changed.retain(|f| f != field);
    changed.len() != before
}

/// Parse ProgressStyle from string
fn parse_progress_style(s: &str) -> Result<ProgressStyle, ApiError> {
    match s.to_lowercase().as_str() {
        "perfile" | "per-file" | "per_file" => Ok(ProgressStyle::PerFile),
        "overall" => Ok(ProgressStyle::Overall),
        "individual" => Ok(ProgressStyle::Individual),
        "rotating" => Ok(ProgressStyle::Rotating),
        "none" => Ok(ProgressStyle::None),
        _ => Err(ApiError::BadRequest(format!(
            "Invalid progress_style: '{}'. Valid values: PerFile, Overall, Individual, Rotating, None",
            s
        ))),
    }
}

/// Parse SortOrder from string
fn parse_sort_order(s: &str) -> Result<SortOrder, ApiError> {
    match s.to_lowercase().as_str() {
        "bydate" | "by-date" | "by_date" | "date" => Ok(SortOrder::ByDate),
        "byname" | "by-name" | "by_name" | "name" => Ok(SortOrder::ByName),
        "byextension" | "by-extension" | "by_extension" | "extension" => Ok(SortOrder::ByExtension),
        "bysize" | "by-size" | "by_size" | "size" => Ok(SortOrder::BySize),
        _ => Err(ApiError::BadRequest(format!(
            "Invalid sort_order: '{}'. Valid values: ByDate, ByName, ByExtension, BySize",
            s
        ))),
    }
}

/// Parse ConsoleFormat from string
fn parse_console_format(s: &str) -> Result<ConsoleFormat, ApiError> {
    match s.to_lowercase().as_str() {
        "compact" => Ok(ConsoleFormat::Compact),
        "pretty" => Ok(ConsoleFormat::Pretty),
        "json" => Ok(ConsoleFormat::Json),
        _ => Err(ApiError::BadRequest(format!(
            "Invalid console_format: '{}'. Valid values: Compact, Pretty, Json",
            s
        ))),
    }
}

/// Parse NotificationBackend from string
fn parse_notification_backend(s: &str) -> Result<NotificationBackend, ApiError> {
    match s.to_lowercase().as_str() {
        "none" => Ok(NotificationBackend::None),
        "webhook" => Ok(NotificationBackend::Webhook),
        "email" => Ok(NotificationBackend::Email),
        _ => Err(ApiError::BadRequest(format!(
            "Invalid notification backend: '{}'. Valid values: None, Webhook, Email",
            s
        ))),
    }
}

/// Download settings (comprehensive view of DownloadConfig)
#[derive(Debug, Serialize, Deserialize)]
pub struct DownloadSettings {
    pub workers: usize,
    pub target_directory: String,
    pub buffer_size_mb: usize,
    /// What to do when a file already exists (feature 040). Missing in a PUT body
    /// leaves it unchanged, unless an old body sends `skip_existing`.
    #[serde(default)]
    pub on_existing: Option<putio_sync_core::config::ExistingFile>,
    /// Replaced by `on_existing`; still read from an old UI tab, never sent
    #[serde(default, skip_serializing)]
    pub skip_existing: Option<bool>,
    pub delete_after_download: bool,
    pub resume_enabled: bool,
    pub chunk_size_mb: Option<usize>,
    pub progress_style: String,
    pub sort_order: String,
    pub reverse_sort: bool,
    pub whitelist_patterns: Option<String>,
    pub blacklist_patterns: Option<String>,
    /// Missing in a PUT body means the default list
    #[serde(default = "putio_sync_core::config::default_blocked_extensions")]
    pub blocked_extensions: Vec<String>,
    pub bandwidth_limit_bytes_per_sec: Option<u64>,
    pub max_retries: u32,
    pub retry_initial_delay_secs: u64,
    pub delete_empty_folders: bool,
}

/// File watcher configuration
#[derive(Debug, Serialize, Deserialize)]
pub struct WatcherConfig {
    pub enabled: bool,
    pub paths: Vec<String>,
    pub refresh_cycle: i8,
    pub parent_folder_id: Option<i64>,
    pub archive_directory: Option<String>,
    pub delete_after_upload: bool,
}

/// Logging configuration
#[derive(Debug, Serialize, Deserialize)]
pub struct LoggingSettings {
    pub level: String,
    pub file_enabled: bool,
    pub file_path: String,
    pub console_enabled: bool,
    pub console_format: String,
    pub module_filters: Vec<String>,
}

/// Notification configuration
#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub backend: String,
    pub webhook_url: Option<String>,
    pub email: Option<String>,
}

/// Put.io empty-folder cleanup settings (`[cleanup]`)
#[derive(Debug, Serialize, Deserialize)]
pub struct CleanupSettings {
    pub enabled: bool,
    pub run_at: String,
    pub min_age_hours: u64,
    /// Missing in a PUT body means the default (on)
    #[serde(default = "default_nested")]
    pub nested: bool,
}

fn default_nested() -> bool {
    CleanupConfig::default().nested
}

/// Put.io storage levels and automatic free-up (`[storage]`, feature 033)
#[derive(Debug, Serialize, Deserialize)]
pub struct StorageSettings {
    pub warn_free_percent: u8,
    pub target_free_percent: u8,
    pub auto_free: bool,
    pub run_at: String,
}

/// Tailscale sign-in and listening (`[tailscale]`, feature 036)
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TailscaleSettings {
    pub tailnet_only: bool,
    pub identity_sign_in: bool,
    pub allowed_users: Vec<String>,
    /// tailscaled's socket; `null` for the default
    pub socket: Option<String>,
}

impl Default for TailscaleSettings {
    fn default() -> Self {
        tailscale_view(&Config::default())
    }
}

/// The release check settings (`[precheck]`, feature 042). A PUT body leaves out
/// `effective`; a missing field means its default.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PrecheckSettings {
    pub enabled: bool,
    pub dry_run: bool,
    pub min_size_mb: u64,
    pub allow_archives: bool,
    pub magnet_lookup: bool,
    pub lookup_timeout_secs: u64,
    /// How long a magnet no cache knows is held before Put.io (043); 0 turns it off
    pub hold_magnet_secs: u64,
    pub hold_retry_secs: u64,
    pub first_lookup_secs: u64,
    pub caches: Vec<String>,
    pub categories: BTreeMap<String, PrecheckOverride>,
    pub watch: BTreeMap<String, PrecheckOverride>,
}

impl Default for PrecheckSettings {
    fn default() -> Self {
        Self::from_config(&PrecheckConfig::default())
    }
}

impl PrecheckSettings {
    fn from_config(p: &PrecheckConfig) -> Self {
        Self {
            enabled: p.enabled,
            dry_run: p.dry_run,
            min_size_mb: p.min_size_mb,
            allow_archives: p.allow_archives,
            magnet_lookup: p.magnet_lookup,
            lookup_timeout_secs: p.lookup_timeout_secs,
            hold_magnet_secs: p.hold_magnet_secs,
            hold_retry_secs: p.hold_retry_secs,
            first_lookup_secs: p.first_lookup_secs,
            caches: p.caches.clone(),
            categories: p.categories.clone(),
            watch: p.watch.clone(),
        }
    }

    fn into_config(self) -> PrecheckConfig {
        PrecheckConfig {
            enabled: self.enabled,
            dry_run: self.dry_run,
            min_size_mb: self.min_size_mb,
            allow_archives: self.allow_archives,
            magnet_lookup: self.magnet_lookup,
            lookup_timeout_secs: self.lookup_timeout_secs,
            hold_magnet_secs: self.hold_magnet_secs,
            hold_retry_secs: self.hold_retry_secs,
            first_lookup_secs: self.first_lookup_secs,
            caches: self.caches,
            categories: self.categories,
            watch: self.watch,
        }
    }
}

/// What the check does for one category or watch folder (FR-020); read-only
#[derive(Debug, Serialize)]
pub struct EffectivePrecheck {
    /// "category" or "watch"
    pub kind: &'static str,
    pub key: String,
    pub enabled: bool,
    pub min_size_mb: u64,
    pub allow_archives: bool,
    /// The wanted types. `None` for a watch folder: its Put.io upload folder's rule
    /// decides them.
    pub wanted: Option<Vec<String>>,
    /// The 009 rule path the wanted types come from; `None` for the video default
    pub wanted_from: Option<String>,
}

/// GET /settings/precheck
#[derive(Debug, Serialize)]
pub struct PrecheckView {
    #[serde(flatten)]
    pub settings: PrecheckSettings,
    pub effective: Vec<EffectivePrecheck>,
}

/// Create the settings router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/download", get(get_download_settings))
        .route("/download", put(update_download_settings))
        .route("/watcher", get(get_watcher_config))
        .route("/watcher", put(update_watcher_config))
        .route("/logging", get(get_logging_settings))
        .route("/logging", put(update_logging_settings))
        .route("/notifications", get(get_notification_settings))
        .route("/notifications", put(update_notification_settings))
        .route("/cleanup", get(get_cleanup_settings))
        .route("/cleanup", put(update_cleanup_settings))
        .route("/storage", get(get_storage_settings))
        .route("/storage", put(update_storage_settings))
        .route("/precheck", get(get_precheck_settings))
        .route("/precheck", put(update_precheck_settings))
        .route("/tailscale", get(get_tailscale_settings))
        .route("/tailscale", put(update_tailscale_settings))
        .route("/download/preview", post(preview_download))
        .route("/watcher/preview", post(preview_watcher))
        .route("/logging/preview", post(preview_logging))
        .route("/notifications/preview", post(preview_notifications))
        .route("/cleanup/preview", post(preview_cleanup))
        .route("/storage/preview", post(preview_storage))
        .route("/precheck/preview", post(preview_precheck))
        .route("/tailscale/preview", post(preview_tailscale))
        .merge(assistant::router())
}

// ---- Previews (feature 015): the PUT body in, what the PUT would do out ----------

/// POST /settings/download/preview
async fn preview_download(
    State(state): State<AppState>,
    settings: Result<Json<DownloadSettings>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let settings = body(settings)?;
    let live = live_download(&state);
    Ok(preview(&state, download_view, live, |c| apply_download(settings, c)).await)
}

/// POST /settings/watcher/preview
async fn preview_watcher(
    State(state): State<AppState>,
    settings: Result<Json<WatcherConfig>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let settings = body(settings)?;
    let live = live_watcher(&state);
    Ok(preview(&state, watcher_view, live, |c| apply_watcher(settings, c)).await)
}

/// POST /settings/logging/preview
async fn preview_logging(
    State(state): State<AppState>,
    settings: Result<Json<LoggingSettings>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let settings = body(settings)?;
    let live = live_logging(&state);
    Ok(preview(&state, logging_view, live, |c| apply_logging(settings, c)).await)
}

/// POST /settings/notifications/preview
async fn preview_notifications(
    State(state): State<AppState>,
    settings: Result<Json<NotificationSettings>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let settings = body(settings)?;
    Ok(preview(&state, notification_view, (&[], ""), |c| {
        apply_notifications(settings, c)
    })
    .await)
}

/// POST /settings/cleanup/preview
async fn preview_cleanup(
    State(state): State<AppState>,
    settings: Result<Json<CleanupSettings>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let settings = body(settings)?;
    let live = live_cleanup(&state);
    Ok(preview(&state, cleanup_view, live, |c| apply_cleanup(settings, c)).await)
}

/// POST /settings/storage/preview
async fn preview_storage(
    State(state): State<AppState>,
    settings: Result<Json<StorageSettings>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let settings = body(settings)?;
    let live = live_storage(&state);
    Ok(preview(&state, storage_view, live, |c| apply_storage(settings, c)).await)
}

/// POST /settings/tailscale/preview
async fn preview_tailscale(
    State(state): State<AppState>,
    settings: Result<Json<TailscaleSettings>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let settings = body(settings)?;
    let live = live_tailscale(&state);
    Ok(preview(&state, tailscale_view, live, |c| {
        apply_tailscale(settings, c)
    })
    .await)
}

/// POST /settings/precheck/preview
async fn preview_precheck(
    State(state): State<AppState>,
    settings: Result<Json<PrecheckSettings>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let settings = body(settings)?;
    Ok(preview(&state, precheck_view, (&[], ""), |c| {
        apply_precheck(settings, c)
    })
    .await)
}

/// Download fields the engine's download manager picks up for the next download
/// (README "Settings from the UI", checked against the daemon code)
const DOWNLOAD_LIVE: &[&str] = &[
    "workers",
    "bandwidth_limit_bytes_per_sec",
    "buffer_size_mb",
    "max_retries",
    "retry_initial_delay_secs",
    "on_existing",
    "resume_enabled",
    "delete_after_download",
    "blocked_extensions",
];

/// Watcher fields a shared watcher picks up at its next scan
const WATCHER_LIVE: &[&str] = &["paths", "parent_folder_id", "archive_directory"];

/// Logging fields the config reload poller applies
const LOGGING_LIVE: &[&str] = &["level", "module_filters"];

fn download_view(config: &Config) -> DownloadSettings {
    DownloadSettings {
        workers: config.download.workers,
        target_directory: config
            .download
            .target_directory
            .to_string_lossy()
            .to_string(),
        buffer_size_mb: config.download.buffer_size_mb,
        on_existing: Some(config.download.on_existing()),
        skip_existing: None,
        delete_after_download: config.download.delete_after_download,
        resume_enabled: config.download.resume_enabled,
        chunk_size_mb: config.download.chunk_size_mb,
        progress_style: format!("{:?}", config.download.progress_style),
        sort_order: format!("{:?}", config.download.sort_order),
        reverse_sort: config.download.reverse_sort,
        whitelist_patterns: config.download.whitelist_patterns.clone(),
        blacklist_patterns: config.download.blacklist_patterns.clone(),
        blocked_extensions: config.download.blocked_extensions.clone(),
        bandwidth_limit_bytes_per_sec: config.download.bandwidth_limit_bytes_per_sec,
        max_retries: config.download.max_retries,
        retry_initial_delay_secs: config.download.retry_initial_delay_secs,
        delete_empty_folders: config.download.delete_empty_folders,
    }
}

fn watcher_view(config: &Config) -> WatcherConfig {
    WatcherConfig {
        enabled: config.watch.enabled,
        paths: config
            .watch
            .paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        refresh_cycle: config.watch.refresh_cycle,
        parent_folder_id: config.watch.parent_folder_id,
        archive_directory: config
            .watch
            .archive_directory
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        delete_after_upload: config.watch.delete_after_upload,
    }
}

fn logging_view(config: &Config) -> LoggingSettings {
    LoggingSettings {
        level: config.logging.level.clone(),
        file_enabled: config.logging.file_enabled,
        file_path: config.logging.file_path.to_string_lossy().to_string(),
        console_enabled: config.logging.console_enabled,
        console_format: format!("{:?}", config.logging.console_format),
        module_filters: config.logging.module_filters.clone(),
    }
}

fn cleanup_view(config: &Config) -> CleanupSettings {
    CleanupSettings {
        enabled: config.cleanup.enabled,
        run_at: config.cleanup.run_at.clone(),
        min_age_hours: config.cleanup.min_age_hours,
        nested: config.cleanup.nested,
    }
}

fn storage_view(config: &Config) -> StorageSettings {
    StorageSettings {
        warn_free_percent: config.storage.warn_free_percent,
        target_free_percent: config.storage.target_free_percent,
        auto_free: config.storage.auto_free,
        run_at: config.storage.run_at.clone(),
    }
}

fn tailscale_view(config: &Config) -> TailscaleSettings {
    TailscaleSettings {
        tailnet_only: config.tailscale.tailnet_only,
        identity_sign_in: config.tailscale.identity_sign_in,
        allowed_users: config.tailscale.allowed_users.clone(),
        socket: config.tailscale.socket.clone(),
    }
}

fn precheck_view(config: &Config) -> PrecheckSettings {
    PrecheckSettings::from_config(&config.precheck)
}

/// One `effective` row per category (the `[[arr.servers]]` ones, the ones items used,
/// and the overridden ones) and per watch folder
fn effective_precheck(config: &Config, used: Vec<String>) -> Vec<EffectivePrecheck> {
    let precheck = &config.precheck;
    const MB: u64 = 1024 * 1024;
    let categories: BTreeSet<String> = config
        .arr
        .servers
        .iter()
        // The category is read from the *arr now (018): the legacy one, else setup's default
        .map(|s| {
            s.category
                .clone()
                .unwrap_or_else(|| s.kind.as_str().to_string())
        })
        .chain(used)
        .chain(precheck.categories.keys().cloned())
        .collect();
    let row = |kind, key: String, policy: Policy, wanted| EffectivePrecheck {
        kind,
        key,
        enabled: policy.enabled,
        min_size_mb: policy.min_size / MB,
        allow_archives: policy.allow_archives,
        wanted_from: if wanted { policy.wanted_from } else { None },
        wanted: wanted.then(|| policy.wanted.into_iter().collect()),
    };
    let rules = &putio_sync_core::rules::RuleSet::from_config(&config.download);
    let mut rows: Vec<EffectivePrecheck> = categories
        .into_iter()
        .map(|c| {
            // Arr items land in the Put.io folder named after their category
            let policy = Policy::for_folder(precheck, rules, &c, Key::Category(&c));
            row("category", c, policy, true)
        })
        .collect();
    let watch: BTreeSet<String> = config
        .watch
        .paths
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .chain(precheck.watch.keys().cloned())
        .collect();
    rows.extend(watch.into_iter().map(|w| {
        let policy = Policy::for_folder(precheck, rules, "", Key::Watch(Path::new(&w)));
        row("watch", w, policy, false)
    }));
    rows
}

fn notification_view(config: &Config) -> NotificationSettings {
    NotificationSettings {
        enabled: config.notifications.enabled,
        backend: format!("{:?}", config.notifications.backend),
        webhook_url: config.notifications.webhook_url.clone(),
        email: config.notifications.email.clone(),
    }
}

/// Get download settings
///
/// GET /settings/download
async fn get_download_settings(
    State(state): State<AppState>,
) -> Result<Json<DownloadSettings>, ApiError> {
    Ok(Json(download_view(&*state.config.read().await)))
}

/// Update download settings
///
/// PUT /settings/download
async fn update_download_settings(
    State(state): State<AppState>,
    settings: Result<Json<DownloadSettings>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    let mut changed = update(&state, download_view, |config| {
        apply_download(settings, config)
    })
    .await?;

    // Hand the new settings to the download manager the API holds: the daemon's own
    // under `daemon`, the API's own under `server`. The pool scales to `workers`; the
    // other `DOWNLOAD_LIVE` fields apply from the next download.
    {
        let download_config = {
            let cfg = state.config.read().await;
            crate::extract_download_config(&cfg)
        };
        state.download_manager.update_config(download_config).await;
    }

    let mut notes = Vec::new();
    if take(&mut changed, "target_directory") {
        notes.push(
            "New downloads go to the new target_directory now; the queue database and disk figures stay on the startup folder until a restart."
                .to_string(),
        );
    }
    if state.engine == Engine::Server && (!changed.is_empty() || !notes.is_empty()) {
        notes.push(
            "The server --download background thread keeps its startup settings until a restart."
                .to_string(),
        );
    }
    let (live, when) = live_download(&state);

    Ok(Json(SuccessResponse {
        message: saved_message("Download", &changed, live, when, &notes),
    }))
}

/// Apply a download body to `config` (shared by PUT and preview)
fn apply_download(settings: DownloadSettings, config: &mut Config) -> Result<(), ApiError> {
    let progress_style = parse_progress_style(&settings.progress_style)?;
    let sort_order = parse_sort_order(&settings.sort_order)?;
    config.download.workers = settings.workers;
    config.download.target_directory = settings.target_directory.into();
    config.download.buffer_size_mb = settings.buffer_size_mb;
    // An unchanged choice leaves the file as it is, old `skip_existing` included
    if let Some(choice) = settings
        .on_existing
        .or(settings.skip_existing.map(Into::into))
        .filter(|choice| *choice != config.download.on_existing())
    {
        config.download.set_on_existing(choice);
    }
    config.download.delete_after_download = settings.delete_after_download;
    config.download.resume_enabled = settings.resume_enabled;
    config.download.chunk_size_mb = settings.chunk_size_mb;
    config.download.progress_style = progress_style;
    config.download.sort_order = sort_order;
    config.download.reverse_sort = settings.reverse_sort;
    config.download.whitelist_patterns = settings.whitelist_patterns;
    config.download.blacklist_patterns = settings.blacklist_patterns;
    config.download.blocked_extensions = settings.blocked_extensions;
    config.download.bandwidth_limit_bytes_per_sec = settings.bandwidth_limit_bytes_per_sec;
    config.download.max_retries = settings.max_retries;
    config.download.retry_initial_delay_secs = settings.retry_initial_delay_secs;
    config.download.delete_empty_folders = settings.delete_empty_folders;
    Ok(())
}

/// Download fields applied without a restart, and when
pub(crate) fn live_download(state: &AppState) -> (&'static [&'static str], &'static str) {
    match state.engine {
        Engine::Daemon => (DOWNLOAD_LIVE, "Applies from the next download"),
        Engine::Server => (
            DOWNLOAD_LIVE,
            "Applies to the API's own download manager from the next download",
        ),
    }
}

/// Get file watcher configuration
///
/// GET /settings/watcher
async fn get_watcher_config(
    State(state): State<AppState>,
) -> Result<Json<WatcherConfig>, ApiError> {
    Ok(Json(watcher_view(&*state.config.read().await)))
}

/// Update file watcher configuration
///
/// PUT /settings/watcher
async fn update_watcher_config(
    State(state): State<AppState>,
    watcher: Result<Json<WatcherConfig>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let watcher = body(watcher)?;
    let mut changed = update(&state, watcher_view, |config| {
        apply_watcher(watcher, config)
    })
    .await?;

    push_watcher(&state).await;
    // parent_folder_id is protected from the empty-folder cleanup
    state.cleanup.configure(&*state.config.read().await);

    let mut notes = Vec::new();
    match (state.engine, state.file_watcher.is_some()) {
        (Engine::Daemon, true) => {
            if take(&mut changed, "refresh_cycle") {
                notes.push(
                    "refresh_cycle is saved, but the daemon scans on --watch-interval.".to_string(),
                );
            }
        }
        (Engine::Daemon, false) => {
            if !changed.is_empty() {
                notes.push(
                    "The daemon started without a watch folder, so the watcher picks up changes only after a restart."
                        .to_string(),
                );
            }
        }
        (Engine::Server, _) => {}
    }
    let (live, when) = live_watcher(&state);

    Ok(Json(SuccessResponse {
        message: saved_message("Watcher", &changed, live, when, &notes),
    }))
}

/// Apply a watcher body to `config` (shared by PUT and preview)
fn apply_watcher(watcher: WatcherConfig, config: &mut Config) -> Result<(), ApiError> {
    config.watch.enabled = watcher.enabled;
    config.watch.paths = watcher.paths.into_iter().map(PathBuf::from).collect();
    config.watch.refresh_cycle = watcher.refresh_cycle;
    config.watch.parent_folder_id = watcher.parent_folder_id;
    config.watch.archive_directory = watcher.archive_directory.map(PathBuf::from);
    config.watch.delete_after_upload = watcher.delete_after_upload;
    Ok(())
}

/// Watcher fields applied without a restart, and when: only a watcher shared with the
/// daemon picks changes up
pub(crate) fn live_watcher(state: &AppState) -> (&'static [&'static str], &'static str) {
    let live: &'static [&'static str] = match (state.engine, state.file_watcher.is_some()) {
        (Engine::Daemon, true) => WATCHER_LIVE,
        _ => &[],
    };
    (live, "Applies at the next scan")
}

/// Get logging settings
///
/// GET /settings/logging
async fn get_logging_settings(
    State(state): State<AppState>,
) -> Result<Json<LoggingSettings>, ApiError> {
    Ok(Json(logging_view(&*state.config.read().await)))
}

/// Update logging settings
///
/// PUT /settings/logging
async fn update_logging_settings(
    State(state): State<AppState>,
    settings: Result<Json<LoggingSettings>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    let mut changed = update(&state, logging_view, |config| {
        apply_logging(settings, config)
    })
    .await?;

    let mut notes = Vec::new();
    if state.logging_reload && std::env::var_os("RUST_LOG").is_some() {
        let level = take(&mut changed, "level");
        let filters = take(&mut changed, "module_filters");
        if level || filters {
            notes.push("level and module_filters are ignored while RUST_LOG is set.".to_string());
        }
    }
    let (live, when) = live_logging(&state);

    Ok(Json(SuccessResponse {
        message: saved_message("Logging", &changed, live, when, &notes),
    }))
}

/// Apply a logging body to `config` (shared by PUT and preview)
fn apply_logging(settings: LoggingSettings, config: &mut Config) -> Result<(), ApiError> {
    let console_format = parse_console_format(&settings.console_format)?;
    config.logging.level = settings.level;
    config.logging.file_enabled = settings.file_enabled;
    config.logging.file_path = settings.file_path.into();
    config.logging.console_enabled = settings.console_enabled;
    config.logging.console_format = console_format;
    config.logging.module_filters = settings.module_filters;
    // The poller would refuse a bad level later; refuse it before it is saved
    putio_sync_core::logging::validate_filters(&config.logging).map_err(invalid)
}

/// Logging fields applied without a restart, and when. The config reload poller
/// (feature 003) applies level and module_filters from the saved file, unless RUST_LOG is
/// set; it runs only if the file existed at startup.
pub(crate) fn live_logging(state: &AppState) -> (&'static [&'static str], &'static str) {
    let live: &'static [&'static str] =
        if !state.logging_reload || std::env::var_os("RUST_LOG").is_some() {
            &[]
        } else {
            LOGGING_LIVE
        };
    (live, "Applies within 10 seconds")
}

/// Get notification settings
///
/// GET /settings/notifications
async fn get_notification_settings(
    State(state): State<AppState>,
) -> Result<Json<NotificationSettings>, ApiError> {
    Ok(Json(notification_view(&*state.config.read().await)))
}

/// Update notification settings
///
/// PUT /settings/notifications
async fn update_notification_settings(
    State(state): State<AppState>,
    settings: Result<Json<NotificationSettings>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    // Notifications are set up at startup, so every change needs a restart
    let changed = update(&state, notification_view, |config| {
        apply_notifications(settings, config)
    })
    .await?;

    Ok(Json(SuccessResponse {
        message: saved_message("Notification", &changed, &[], "", &[]),
    }))
}

/// Apply a notifications body to `config` (shared by PUT and preview)
fn apply_notifications(
    settings: NotificationSettings,
    config: &mut Config,
) -> Result<(), ApiError> {
    config.notifications.backend = parse_notification_backend(&settings.backend)?;
    config.notifications.enabled = settings.enabled;
    config.notifications.webhook_url = settings.webhook_url;
    config.notifications.email = settings.email;
    Ok(())
}

/// Get the empty-folder cleanup settings
///
/// GET /settings/cleanup
async fn get_cleanup_settings(
    State(state): State<AppState>,
) -> Result<Json<CleanupSettings>, ApiError> {
    Ok(Json(cleanup_view(&*state.config.read().await)))
}

/// Update the empty-folder cleanup settings
///
/// PUT /settings/cleanup
async fn update_cleanup_settings(
    State(state): State<AppState>,
    settings: Result<Json<CleanupSettings>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;

    let changed = update(&state, cleanup_view, |config| {
        apply_cleanup(settings, config)
    })
    .await?;

    // The cleanup service reads its settings on every run, and the daemon's schedule on
    // every check
    state.cleanup.configure(&*state.config.read().await);

    let mut notes = Vec::new();
    if state.engine == Engine::Server && !changed.is_empty() {
        notes.push(
            "The daily run happens only under `daemon`; manual runs use min_age_hours and nested now."
                .to_string(),
        );
    }
    let (live, when) = live_cleanup(&state);

    Ok(Json(SuccessResponse {
        message: saved_message("Cleanup", &changed, live, when, &notes),
    }))
}

/// Apply a cleanup body to `config` (shared by PUT and preview)
fn apply_cleanup(settings: CleanupSettings, config: &mut Config) -> Result<(), ApiError> {
    config.cleanup = CleanupConfig {
        enabled: settings.enabled,
        run_at: settings.run_at,
        min_age_hours: settings.min_age_hours,
        nested: settings.nested,
    };
    Ok(())
}

/// Cleanup fields applied without a restart, and when
pub(crate) fn live_cleanup(state: &AppState) -> (&'static [&'static str], &'static str) {
    let live: &'static [&'static str] = match state.engine {
        Engine::Daemon => &["enabled", "run_at", "min_age_hours", "nested"],
        Engine::Server => &["min_age_hours", "nested"],
    };
    (live, "Applies from the next run")
}

/// Get the Put.io storage settings
///
/// GET /settings/storage
async fn get_storage_settings(
    State(state): State<AppState>,
) -> Result<Json<StorageSettings>, ApiError> {
    Ok(Json(storage_view(&*state.config.read().await)))
}

/// Update the Put.io storage settings
///
/// PUT /settings/storage
async fn update_storage_settings(
    State(state): State<AppState>,
    settings: Result<Json<StorageSettings>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    let changed = update(&state, storage_view, |config| {
        apply_storage(settings, config)
    })
    .await?;

    // The storage service reads its levels on every summary, plan and run, and the
    // daemon's schedule on every check
    state.storage.configure(&*state.config.read().await);

    let mut notes = Vec::new();
    if state.engine == Engine::Server && changed.iter().any(|c| c == "auto_free" || c == "run_at") {
        notes.push("The automatic run happens only under `daemon`.".to_string());
    }
    let (live, when) = live_storage(&state);

    Ok(Json(SuccessResponse {
        message: saved_message("Storage", &changed, live, when, &notes),
    }))
}

/// Apply a storage body to `config` (shared by PUT and preview)
fn apply_storage(settings: StorageSettings, config: &mut Config) -> Result<(), ApiError> {
    config.storage = StorageConfig {
        warn_free_percent: settings.warn_free_percent,
        target_free_percent: settings.target_free_percent,
        auto_free: settings.auto_free,
        run_at: settings.run_at,
    };
    Ok(())
}

/// Storage fields applied without a restart, and when
pub(crate) fn live_storage(state: &AppState) -> (&'static [&'static str], &'static str) {
    let live: &'static [&'static str] = match state.engine {
        Engine::Daemon => &[
            "warn_free_percent",
            "target_free_percent",
            "auto_free",
            "run_at",
        ],
        Engine::Server => &["warn_free_percent", "target_free_percent"],
    };
    (live, "Applies now")
}

/// Get the Tailscale settings
///
/// GET /settings/tailscale
async fn get_tailscale_settings(
    State(state): State<AppState>,
) -> Result<Json<TailscaleSettings>, ApiError> {
    Ok(Json(tailscale_view(&*state.config.read().await)))
}

/// Update the Tailscale settings
///
/// PUT /settings/tailscale
///
/// Turning `tailnet_only` on is refused unless Tailscale gives a tailnet address now,
/// so a save can't leave the daemon with nothing to listen on after a restart.
async fn update_tailscale_settings(
    State(state): State<AppState>,
    settings: Result<Json<TailscaleSettings>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    let was_on = state.config.read().await.tailscale.tailnet_only;
    let port = state.api_port().await;
    if settings.tailnet_only && !was_on {
        let candidate = TailscaleConfig {
            socket: settings.socket.clone(),
            ..TailscaleConfig::default()
        };
        let status = state
            .tailscale
            .sibling(candidate.socket_path())
            .refresh()
            .await;
        if let Err(reason) = putio_sync_tailscale::tailnet_bind(&status, port) {
            return Err(ApiError::BadRequest(format!(
                "Can't listen on the tailnet only: {reason}."
            )));
        }
    }
    let changed = update(&state, tailscale_view, |config| {
        apply_tailscale(settings, config)
    })
    .await?;

    // Sign-in reads the settings on every request; the service follows the socket
    let socket = state.config.read().await.tailscale.socket_path();
    if state.tailscale.socket() != socket {
        state.tailscale.configure(socket);
    }

    let mut notes = Vec::new();
    if changed.iter().any(|c| c == "tailnet_only") {
        notes.push(match state.engine {
            Engine::Daemon => "Restart putio-sync to change the address it listens on.".to_string(),
            Engine::Server => {
                "Listening on the tailnet only applies under `daemon`; `server` uses its --bind."
                    .to_string()
            }
        });
    }
    let (live, when) = live_tailscale(&state);

    Ok(Json(SuccessResponse {
        message: saved_message("Tailscale", &changed, live, when, &notes),
    }))
}

/// Apply a Tailscale body to `config` (shared by PUT and preview)
fn apply_tailscale(settings: TailscaleSettings, config: &mut Config) -> Result<(), ApiError> {
    config.tailscale = TailscaleConfig {
        tailnet_only: settings.tailnet_only,
        identity_sign_in: settings.identity_sign_in,
        allowed_users: settings
            .allowed_users
            .iter()
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())
            .collect(),
        socket: settings.socket.filter(|s| !s.trim().is_empty()),
    };
    Ok(())
}

/// Tailscale fields applied without a restart, and when
pub(crate) fn live_tailscale(_state: &AppState) -> (&'static [&'static str], &'static str) {
    (
        &["identity_sign_in", "allowed_users", "socket"],
        "Applies now",
    )
}

/// Get the release check settings and what they come to per folder
///
/// GET /settings/precheck
async fn get_precheck_settings(
    State(state): State<AppState>,
) -> Result<Json<PrecheckView>, ApiError> {
    // The categories items used; without a database, only the configured ones
    let used = match state.database.as_deref() {
        Some(db) => db.arr().categories().await.unwrap_or_default(),
        None => Vec::new(),
    };
    let config = state.config.read().await;
    Ok(Json(PrecheckView {
        settings: precheck_view(&config),
        effective: effective_precheck(&config, used),
    }))
}

/// Update the release check settings
///
/// PUT /settings/precheck
async fn update_precheck_settings(
    State(state): State<AppState>,
    settings: Result<Json<PrecheckSettings>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    // The *arr service and the watcher build their check at startup
    let changed = update(&state, precheck_view, |config| {
        apply_precheck(settings, config)
    })
    .await?;

    Ok(Json(SuccessResponse {
        message: saved_message("Release check", &changed, &[], "", &[]),
    }))
}

/// Apply a release check body to `config` (shared by PUT and preview)
fn apply_precheck(settings: PrecheckSettings, config: &mut Config) -> Result<(), ApiError> {
    config.precheck = settings.into_config();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_creation() {
        let _router = router();
    }
}
