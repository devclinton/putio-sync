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
//! - GET  /settings/updates - Get the update check settings (feature 055)
//! - PUT  /settings/updates - Update them; they apply at the next check
//! - GET  /settings/hooks - Get the event hooks, header and env values masked (feature 027)
//! - PUT  /settings/hooks - Replace the event hooks; a masked value keeps the stored one
//! - GET  /settings/pipeline - Get the processing pipeline settings (feature 038)
//! - PUT  /settings/pipeline - Replace them; they apply now
//! - GET  /settings/plugins - Get the plugins folder and plugin settings, secrets masked
//!   (feature 028)
//! - PUT  /settings/plugins - Replace them; checked against the manifests, applied now
//! - GET  /settings/putio - Get the Put.io app ID and where the one in use comes from
//! - PUT  /settings/putio - Update the Put.io app ID; refused while PUTIO_SYNC_APP_ID sets it
//! - GET/PUT /settings/subtitles - Wanted subtitle languages and transcription (feature 025,
//!   in `subtitles.rs`)
//! - GET/PUT /settings/translation - Subtitle translation (feature 026, in `translation.rs`)
//! - GET/PUT /settings/network - The listen addresses (feature 037, in `network.rs`)
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
use putio_sync_core::email::SmtpConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Re-export AppState from parent
pub use crate::AppState;
use crate::Engine;
use putio_sync_core::assistant::Change;
use putio_sync_core::config::{
    CleanupConfig, Config, NotificationBackend, PipelineConfig, PrecheckConfig, PrecheckOverride,
    ProgressStyle, SortOrder, StorageConfig, TailscaleConfig, UpdatesConfig,
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
    putio_sync_core::log_redact::set_secrets(candidate.secret_values());
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
pub(crate) async fn preview<V: Serialize>(
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

/// The fourteen groups. Defaults are the default config's, with the current download
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
            "hooks",
            json(serde_json::to_value(hooks_view(&defaults))),
            live_hooks(),
        ),
        group(
            "pipeline",
            json(serde_json::to_value(pipeline_view(&defaults))),
            live_pipeline(),
        ),
        group(
            "plugins",
            json(serde_json::to_value(plugins_view(&defaults))),
            live_plugins(),
        ),
        group(
            "network",
            json(serde_json::to_value(super::network::network_view(
                &defaults,
            ))),
            (&[], ""),
        ),
        group(
            "logging",
            json(serde_json::to_value(logging_view(&defaults))),
            live_logging(state),
        ),
        group(
            "notifications",
            json(serde_json::to_value(notification_view(&defaults))),
            live_notifications(),
        ),
        group(
            "precheck",
            json(serde_json::to_value(precheck_view(&defaults))),
            (&[], ""),
        ),
        group(
            "putio",
            json(serde_json::to_value(putio_view(&defaults))),
            live_putio(),
        ),
        group(
            "subtitles",
            json(serde_json::to_value(super::subtitles::subtitles_view(
                &defaults,
            ))),
            super::subtitles::live_subtitles(),
        ),
        group(
            "translation",
            json(serde_json::to_value(super::translation::translation_view(
                &defaults,
            ))),
            super::translation::live_translation(),
        ),
        group(
            "organise",
            json(serde_json::to_value(super::organise::organise_view(
                &defaults,
            ))),
            super::organise::live_organise(),
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
pub(crate) fn saved_message(
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
    /// Limits by time of day and day of week (feature 032). Missing in a PUT body leaves
    /// it unchanged.
    #[serde(default)]
    pub bandwidth_schedule: Option<Vec<putio_sync_core::bandwidth::ScheduleEntry>>,
    /// The "Slow" limit. Missing in a PUT body leaves it unchanged.
    #[serde(default)]
    pub slow_limit_bytes_per_sec: Option<u64>,
    pub max_retries: u32,
    pub retry_initial_delay_secs: u64,
    pub delete_empty_folders: bool,
    /// Download verification (feature 034). Each missing in a PUT body leaves it
    /// unchanged.
    #[serde(default)]
    pub verify: Option<bool>,
    #[serde(default)]
    pub verify_max_redownloads: Option<u32>,
    #[serde(default)]
    pub verify_read_limit_mb_per_sec: Option<u64>,
    #[serde(default)]
    pub verify_follow_bandwidth_pause: Option<bool>,
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
    /// Lines kept for the Logs page (feature 047); left out keeps the current value
    #[serde(default)]
    pub buffer_lines: Option<usize>,
}

/// Notification configuration
#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub backend: String,
    pub webhook_url: Option<String>,
    pub email: Option<String>,
    /// Email over SMTP (feature 030), password masked; left out keeps the current one
    #[serde(default)]
    pub smtp: Option<SmtpConfig>,
    /// What the email channel has sent; read-only
    #[serde(default, skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub smtp_status: Option<putio_sync_notify::EmailStatus>,
}

/// The result of a test email
#[derive(Debug, Serialize)]
pub struct EmailTestResult {
    /// Whether the server took it
    pub ok: bool,
    /// Why it failed
    pub error: Option<String>,
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
    /// Keep folders matching these patterns (feature 021); missing means none
    #[serde(default)]
    pub keep_patterns: Vec<String>,
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

/// Event hooks (`[[hooks]]`, feature 027). The view masks every header and env value
/// with `"********"`; a PUT sending it keeps the stored value.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct HooksSettings {
    #[serde(default)]
    pub hooks: Vec<putio_sync_core::hooks::HookConfig>,
}

/// Lua plugins (`plugins_dir` and `[[plugins]]`, feature 028). The view masks every
/// secret value with `"********"`; a PUT sending it keeps the stored value.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PluginsSettings {
    #[serde(default)]
    pub plugins_dir: Option<PathBuf>,
    #[serde(default)]
    pub plugins: Vec<putio_sync_core::plugins::PluginSettings>,
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

/// The update check (`[updates]`, feature 055)
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdatesSettings {
    pub check: bool,
    pub auto_install: bool,
}

impl Default for UpdatesSettings {
    fn default() -> Self {
        updates_view(&Config::default())
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

/// The Put.io app ID (`[putio]`). A PUT body leaves out `status`.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PutioSettings {
    /// The configured app ID; `null` for none
    pub app_id: Option<String>,
}

/// GET /settings/putio
#[derive(Debug, Serialize)]
pub struct PutioView {
    #[serde(flatten)]
    pub settings: PutioSettings,
    pub status: AppIdStatus,
}

/// The app ID in use, which may not be the configured one
#[derive(Debug, Serialize)]
pub struct AppIdStatus {
    /// The app ID the device link uses; `null` when the link is off
    pub in_use: Option<String>,
    /// Where it comes from: `environment`, `config` or `built_in`
    pub source: Option<putio_sync_core::setup::AppIdSource>,
    /// `PUTIO_SYNC_APP_ID` sets it, so a save can't change it
    pub env_override: bool,
    /// Where to register a Put.io app
    pub register_url: &'static str,
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
        .route("/notifications/test", post(test_email))
        .route("/cleanup", get(get_cleanup_settings))
        .route("/cleanup", put(update_cleanup_settings))
        .route("/storage", get(get_storage_settings))
        .route("/storage", put(update_storage_settings))
        .route("/precheck", get(get_precheck_settings))
        .route("/precheck", put(update_precheck_settings))
        .route("/tailscale", get(get_tailscale_settings))
        .route("/tailscale", put(update_tailscale_settings))
        .route("/updates", get(get_updates_settings))
        .route("/updates", put(update_updates_settings))
        .route("/hooks", get(get_hooks_settings))
        .route("/hooks", put(update_hooks_settings))
        .route("/pipeline", get(get_pipeline_settings))
        .route("/pipeline", put(update_pipeline_settings))
        .route("/plugins", get(get_plugins_settings))
        .route("/plugins", put(update_plugins_settings))
        .route("/putio", get(get_putio_settings))
        .route("/putio", put(update_putio_settings))
        .route("/download/preview", post(preview_download))
        .route("/watcher/preview", post(preview_watcher))
        .route("/logging/preview", post(preview_logging))
        .route("/notifications/preview", post(preview_notifications))
        .route("/cleanup/preview", post(preview_cleanup))
        .route("/storage/preview", post(preview_storage))
        .route("/precheck/preview", post(preview_precheck))
        .route("/tailscale/preview", post(preview_tailscale))
        .route("/updates/preview", post(preview_updates))
        .route("/hooks/preview", post(preview_hooks))
        .route("/pipeline/preview", post(preview_pipeline))
        .route("/plugins/preview", post(preview_plugins))
        .route("/putio/preview", post(preview_putio))
        .merge(super::network::settings_router())
        .merge(super::subtitles::settings_router())
        .merge(super::media_server::settings_router())
        .merge(super::organise::settings_router())
        .merge(super::translation::settings_router())
        .merge(super::signin::settings_router())
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
    Ok(
        preview(&state, notification_view, live_notifications(), |c| {
            apply_notifications(settings, c)
        })
        .await,
    )
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

/// POST /settings/hooks/preview
async fn preview_hooks(
    State(state): State<AppState>,
    settings: Result<Json<HooksSettings>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let settings = body(settings)?;
    Ok(preview(&state, hooks_view, live_hooks(), |c| {
        apply_hooks(settings, c)
    })
    .await)
}

/// POST /settings/plugins/preview
async fn preview_plugins(
    State(state): State<AppState>,
    settings: Result<Json<PluginsSettings>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let settings = body(settings)?;
    let apply = apply_plugins(&state, settings);
    Ok(preview(&state, plugins_view, live_plugins(), apply).await)
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

/// POST /settings/updates/preview
async fn preview_updates(
    State(state): State<AppState>,
    settings: Result<Json<UpdatesSettings>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let settings = body(settings)?;
    Ok(preview(&state, updates_view, live_updates(), |c| {
        apply_updates(settings, c)
    })
    .await)
}

/// POST /settings/putio/preview
async fn preview_putio(
    State(state): State<AppState>,
    settings: Result<Json<PutioSettings>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let settings = body(settings)?;
    Ok(preview(&state, putio_view, live_putio(), |c| {
        apply_putio(settings, c)
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
    "bandwidth_schedule",
    "slow_limit_bytes_per_sec",
    "buffer_size_mb",
    "max_retries",
    "retry_initial_delay_secs",
    "on_existing",
    "resume_enabled",
    "delete_after_download",
    "blocked_extensions",
    "verify",
    "verify_max_redownloads",
    "verify_read_limit_mb_per_sec",
    "verify_follow_bandwidth_pause",
];

/// Watcher fields a shared watcher picks up at its next scan
const WATCHER_LIVE: &[&str] = &["paths", "parent_folder_id", "archive_directory"];

/// Logging fields the config reload poller applies
const LOGGING_LIVE: &[&str] = &["level", "module_filters", "buffer_lines"];

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
        bandwidth_schedule: Some(config.download.bandwidth_schedule.clone()),
        slow_limit_bytes_per_sec: Some(config.download.slow_limit_bytes_per_sec),
        max_retries: config.download.max_retries,
        retry_initial_delay_secs: config.download.retry_initial_delay_secs,
        delete_empty_folders: config.download.delete_empty_folders,
        verify: Some(config.download.verify),
        verify_max_redownloads: Some(config.download.verify_max_redownloads),
        verify_read_limit_mb_per_sec: Some(config.download.verify_read_limit_mb_per_sec),
        verify_follow_bandwidth_pause: Some(config.download.verify_follow_bandwidth_pause),
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
        buffer_lines: Some(config.logging.buffer_lines),
    }
}

fn cleanup_view(config: &Config) -> CleanupSettings {
    CleanupSettings {
        enabled: config.cleanup.enabled,
        run_at: config.cleanup.run_at.clone(),
        min_age_hours: config.cleanup.min_age_hours,
        nested: config.cleanup.nested,
        keep_patterns: config.cleanup.keep_patterns.clone(),
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

fn updates_view(config: &Config) -> UpdatesSettings {
    UpdatesSettings {
        check: config.updates.check,
        auto_install: config.updates.auto_install,
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
        smtp: Some(config.notifications.smtp.masked()),
        smtp_status: None,
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
    if changed.iter().any(|c| c == "target_directory") {
        // Plugins see the folder as putio.download_dir (031); rebuild them with it
        let config = state.config.read().await;
        state
            .plugins
            .set_download_dir(Some(config.download.target_directory.clone()));
        state.plugins.configure(config.plugins.clone());
    }
    if take(&mut changed, "target_directory") {
        notes.push(
            "New downloads go to the new target_directory now; the disk figures stay on the startup folder until a restart."
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
    if let Some(schedule) = settings.bandwidth_schedule {
        config.download.bandwidth_schedule = schedule;
    }
    if let Some(slow) = settings.slow_limit_bytes_per_sec {
        config.download.slow_limit_bytes_per_sec = slow;
    }
    config.download.max_retries = settings.max_retries;
    config.download.retry_initial_delay_secs = settings.retry_initial_delay_secs;
    config.download.delete_empty_folders = settings.delete_empty_folders;
    if let Some(verify) = settings.verify {
        config.download.verify = verify;
    }
    if let Some(max) = settings.verify_max_redownloads {
        config.download.verify_max_redownloads = max;
    }
    if let Some(limit) = settings.verify_read_limit_mb_per_sec {
        config.download.verify_read_limit_mb_per_sec = limit;
    }
    if let Some(follow) = settings.verify_follow_bandwidth_pause {
        config.download.verify_follow_bandwidth_pause = follow;
    }
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
    // The Logs page buffer is resized at once, whatever the poller does
    putio_sync_core::log_capture::set_capacity(state.config.read().await.logging.buffer_lines);

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
    if let Some(lines) = settings.buffer_lines {
        config.logging.buffer_lines = lines;
    }
    // The poller would refuse a bad level later; refuse it before it is saved
    putio_sync_core::logging::validate_filters(&config.logging).map_err(invalid)
}

/// Logging fields applied without a restart, and when. The config reload poller
/// (feature 003) applies level and module_filters from the saved file, unless RUST_LOG is
/// set; it runs only if the file existed at startup.
pub(crate) fn live_logging(state: &AppState) -> (&'static [&'static str], &'static str) {
    let live: &'static [&'static str] =
        if !state.logging_reload || std::env::var_os("RUST_LOG").is_some() {
            &["buffer_lines"]
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
    let mut view = notification_view(&*state.config.read().await);
    view.smtp_status = Some(state.email.status());
    Ok(Json(view))
}

/// Update notification settings
///
/// PUT /settings/notifications
async fn update_notification_settings(
    State(state): State<AppState>,
    settings: Result<Json<NotificationSettings>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    let before = state.config.read().await.notifications.smtp.clone();
    // Email applies now; the legacy backend is set up at startup
    let mut changed = update(&state, notification_view, |config| {
        apply_notifications(settings, config)
    })
    .await?;
    let smtp = state.config.read().await.notifications.smtp.clone();
    // The view masks the password, so a new password alone shows no change there
    if smtp != before && !changed.iter().any(|f| f == "smtp") {
        changed.push("smtp".to_string());
    }
    state.email.configure(smtp);

    Ok(Json(SuccessResponse {
        message: {
            let (live, when) = live_notifications();
            saved_message("Notification", &changed, live, when, &[])
        },
    }))
}

/// Send a test email with the settings in the body, saved or not; a masked password is
/// the stored one
///
/// POST /settings/notifications/test
async fn test_email(
    State(state): State<AppState>,
    smtp: Result<Json<SmtpConfig>, JsonRejection>,
) -> Result<Json<EmailTestResult>, ApiError> {
    let smtp = body(smtp)?.unmasked(&state.config.read().await.notifications.smtp);
    let smtp = SmtpConfig {
        enabled: true,
        ..smtp
    };
    smtp.validate().map_err(invalid)?;
    let result = state.email.test(&smtp).await;
    Ok(Json(EmailTestResult {
        ok: result.is_ok(),
        error: result.err(),
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
    if let Some(smtp) = settings.smtp {
        config.notifications.smtp = smtp.unmasked(&config.notifications.smtp);
    }
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
        keep_patterns: settings
            .keep_patterns
            .into_iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect(),
    };
    Ok(())
}

/// Cleanup fields applied without a restart, and when
pub(crate) fn live_cleanup(state: &AppState) -> (&'static [&'static str], &'static str) {
    let live: &'static [&'static str] = match state.engine {
        Engine::Daemon => &[
            "enabled",
            "run_at",
            "min_age_hours",
            "nested",
            "keep_patterns",
        ],
        Engine::Server => &["min_age_hours", "nested", "keep_patterns"],
    };
    (live, "Applies from the next run")
}

/// Get the event hooks
///
/// GET /settings/hooks
async fn get_hooks_settings(State(state): State<AppState>) -> Json<HooksSettings> {
    Json(hooks_view(&*state.config.read().await))
}

/// Replace the event hooks
///
/// PUT /settings/hooks. Applies now: the running service takes the new list, keeping
/// each remaining hook's queue and runs.
async fn update_hooks_settings(
    State(state): State<AppState>,
    settings: Result<Json<HooksSettings>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    let before = state.config.read().await.hooks.clone();
    let mut changed = update(&state, hooks_view, |config| apply_hooks(settings, config)).await?;
    let hooks = state.config.read().await.hooks.clone();
    // The view masks values, so a changed header or env value alone shows no change there
    if changed.is_empty() && hooks != before {
        changed.push("hooks".to_string());
    }
    state.hooks.configure(hooks);
    configure_pipeline(&state).await;
    let (live, when) = live_hooks();
    Ok(Json(SuccessResponse {
        message: saved_message("Hooks", &changed, live, when, &[]),
    }))
}

/// Apply a hooks body to `config` (shared by PUT and preview): masked values come back
/// from the stored hook of the same name
fn apply_hooks(settings: HooksSettings, config: &mut Config) -> Result<(), ApiError> {
    config.hooks =
        putio_sync_core::hooks::unmask(&settings.hooks, &config.hooks).map_err(invalid)?;
    Ok(())
}

/// Email applies now; the legacy backend is set up at startup
pub(crate) fn live_notifications() -> (&'static [&'static str], &'static str) {
    (&["smtp"], "Applies now")
}

/// Hooks apply without a restart under both engines
pub(crate) fn live_hooks() -> (&'static [&'static str], &'static str) {
    (&["hooks"], "Applies now")
}

fn hooks_view(config: &Config) -> HooksSettings {
    HooksSettings {
        hooks: putio_sync_core::hooks::mask(&config.hooks),
    }
}

/// Give the processing pipeline the saved config: its hooks, concurrency and staging
async fn configure_pipeline(state: &AppState) {
    if let Some(pipeline) = &state.pipeline {
        pipeline.configure(&*state.config.read().await);
    }
}

/// Get the processing pipeline settings (`[pipeline]`, feature 038)
///
/// GET /settings/pipeline
async fn get_pipeline_settings(State(state): State<AppState>) -> Json<PipelineConfig> {
    Json(pipeline_view(&*state.config.read().await))
}

/// Replace the processing pipeline settings
///
/// PUT /settings/pipeline. Applies now: a new concurrency or staging mode is used by the
/// next step.
async fn update_pipeline_settings(
    State(state): State<AppState>,
    settings: Result<Json<PipelineConfig>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    let changed = update(&state, pipeline_view, |config| {
        // Staging needs a rename inside one filesystem (R5)
        if settings.staging != putio_sync_core::config::StagingMode::Off {
            putio_sync_pipeline::staging::check_same_device(&config.download.target_directory)
                .map_err(|e| ApiError::BadRequest(format!("staging: {e}")))?;
        }
        config.pipeline = settings;
        Ok(())
    })
    .await?;
    configure_pipeline(&state).await;
    let (live, when) = live_pipeline();
    Ok(Json(SuccessResponse {
        message: saved_message("Pipeline", &changed, live, when, &[]),
    }))
}

/// POST /settings/pipeline/preview
async fn preview_pipeline(
    State(state): State<AppState>,
    settings: Result<Json<PipelineConfig>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let settings = body(settings)?;
    Ok(preview(&state, pipeline_view, live_pipeline(), |c| {
        c.pipeline = settings;
        Ok(())
    })
    .await)
}

/// Pipeline settings apply without a restart under both engines
pub(crate) fn live_pipeline() -> (&'static [&'static str], &'static str) {
    (
        &["concurrency", "optional_retries", "staging", "history_days"],
        "Applies now",
    )
}

fn pipeline_view(config: &Config) -> PipelineConfig {
    config.pipeline.clone()
}

/// Get the plugins folder and plugin settings
///
/// GET /settings/plugins
async fn get_plugins_settings(State(state): State<AppState>) -> Json<PluginsSettings> {
    Json(plugins_view(&*state.config.read().await))
}

/// Replace the plugins folder and plugin settings
///
/// PUT /settings/plugins. Applies now: running plugins take the new settings, and a
/// changed folder is read again.
async fn update_plugins_settings(
    State(state): State<AppState>,
    settings: Result<Json<PluginsSettings>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    let before = state.config.read().await.plugins.clone();
    let apply = apply_plugins(&state, settings);
    let mut changed = update(&state, plugins_view, apply).await?;
    let config = state.config.read().await.clone();
    // The view masks secrets, so a changed secret alone shows no change there
    if !changed.iter().any(|c| c == "plugins") && config.plugins != before {
        changed.push("plugins".to_string());
    }
    state.plugins.configure(config.plugins.clone());
    let dir = plugins_dir(&state, &config);
    if dir != state.plugins.dir() {
        state.plugins.set_dir(dir);
    }
    let (live, when) = live_plugins();
    Ok(Json(SuccessResponse {
        message: saved_message("Plugins", &changed, live, when, &[]),
    }))
}

/// Save one plugin's settings (a plugin action saved values, 031): replaces the entry of
/// the same name or adds it, checks every plugin as a Settings save does, then applies
pub(crate) async fn save_plugin(
    state: &AppState,
    settings: putio_sync_core::plugins::PluginSettings,
) -> Result<(), ApiError> {
    update(state, plugins_view, |config| {
        let mut plugins = config.plugins.clone();
        match plugins.iter_mut().find(|p| p.name == settings.name) {
            Some(entry) => {
                entry.config = settings.config;
                entry.secrets = settings.secrets;
            }
            None => plugins.push(settings),
        }
        let dir = plugins_dir(state, config);
        config.plugins = state
            .plugins
            .check_at(dir.as_deref(), &plugins)
            .map_err(ApiError::BadRequest)?;
        Ok(())
    })
    .await?;
    let config = state.config.read().await;
    state.plugins.configure(config.plugins.clone());
    Ok(())
}

/// The folder a config's plugins come from
fn plugins_dir(state: &AppState, config: &Config) -> Option<PathBuf> {
    putio_sync_core::plugins::dir_for(
        config.plugins_dir.as_deref(),
        state.config_path.as_deref().map(PathBuf::as_path),
    )
}

/// Apply a plugins body (shared by PUT and preview): masked secrets come back from the
/// stored plugin of the same name, values are checked against the manifests in the
/// folder being saved, and each enabled plugin's approved hosts become the hosts it needs
fn apply_plugins(
    state: &AppState,
    settings: PluginsSettings,
) -> impl FnOnce(&mut Config) -> Result<(), ApiError> + '_ {
    move |config| {
        let plugins = putio_sync_core::plugins::unmask(&settings.plugins, &config.plugins)
            .map_err(invalid)?;
        config.plugins_dir = settings.plugins_dir;
        let dir = plugins_dir(state, config);
        config.plugins = state
            .plugins
            .check_at(dir.as_deref(), &plugins)
            .map_err(ApiError::BadRequest)?;
        Ok(())
    }
}

/// Plugins apply without a restart under both engines
pub(crate) fn live_plugins() -> (&'static [&'static str], &'static str) {
    (&["plugins_dir", "plugins"], "Applies now")
}

fn plugins_view(config: &Config) -> PluginsSettings {
    PluginsSettings {
        plugins_dir: config.plugins_dir.clone(),
        plugins: putio_sync_core::plugins::mask(&config.plugins),
    }
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

/// Get the update check settings
///
/// GET /settings/updates
async fn get_updates_settings(
    State(state): State<AppState>,
) -> Result<Json<UpdatesSettings>, ApiError> {
    Ok(Json(updates_view(&*state.config.read().await)))
}

/// Update the update check settings
///
/// PUT /settings/updates
async fn update_updates_settings(
    State(state): State<AppState>,
    settings: Result<Json<UpdatesSettings>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    let changed = update(&state, updates_view, |config| {
        apply_updates(settings, config)
    })
    .await?;

    // The checker reads its settings at every tick
    if let Some(ref updates) = state.updates {
        updates.configure(state.config.read().await.updates.clone());
    }
    let (live, when) = live_updates();

    Ok(Json(SuccessResponse {
        message: saved_message("Updates", &changed, live, when, &[]),
    }))
}

/// Apply an updates body to `config` (shared by PUT and preview)
fn apply_updates(settings: UpdatesSettings, config: &mut Config) -> Result<(), ApiError> {
    config.updates = UpdatesConfig {
        check: settings.check,
        auto_install: settings.auto_install,
    };
    Ok(())
}

/// Update fields applied without a restart, and when
fn live_updates() -> (&'static [&'static str], &'static str) {
    (&["check", "auto_install"], "Applies at the next check")
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

fn putio_view(config: &Config) -> PutioSettings {
    PutioSettings {
        app_id: config.putio.app_id.clone(),
    }
}

/// Get the Put.io app ID and the one in use
///
/// GET /settings/putio
async fn get_putio_settings(State(state): State<AppState>) -> Json<PutioView> {
    let config = state.config.read().await;
    let in_use = putio_sync_core::setup::app_id_with_source(&config);
    Json(PutioView {
        settings: putio_view(&config),
        status: AppIdStatus {
            source: in_use.as_ref().map(|(_, source)| *source),
            in_use: in_use.map(|(id, _)| id),
            env_override: app_id_env_set(),
            register_url: putio_sync_core::setup::APP_REGISTER_URL,
        },
    })
}

fn app_id_env_set() -> bool {
    std::env::var(putio_sync_core::setup::APP_ID_VAR).is_ok_and(|v| !v.trim().is_empty())
}

/// Update the Put.io app ID
///
/// PUT /settings/putio
///
/// While `PUTIO_SYNC_APP_ID` is set, a save that changes the app ID is refused: the
/// environment would still win.
async fn update_putio_settings(
    State(state): State<AppState>,
    settings: Result<Json<PutioSettings>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    let changed = update(&state, putio_view, |config| {
        let before = config.putio.app_id.clone();
        apply_putio(settings, config)?;
        if app_id_env_set() && config.putio.app_id != before {
            return Err(ApiError::BadRequest(format!(
                "{} sets the Put.io app ID; change it there.",
                putio_sync_core::setup::APP_ID_VAR
            )));
        }
        Ok(())
    })
    .await?;
    let (live, when) = live_putio();
    Ok(Json(SuccessResponse {
        message: saved_message("Put.io", &changed, live, when, &[]),
    }))
}

/// Apply a Put.io body to `config` (shared by PUT and preview); empty means none
fn apply_putio(settings: PutioSettings, config: &mut Config) -> Result<(), ApiError> {
    config.putio.app_id = settings
        .app_id
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty());
    Ok(())
}

/// The app ID is read each time a link code is asked for
pub(crate) fn live_putio() -> (&'static [&'static str], &'static str) {
    (&["app_id"], "Applies now")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_creation() {
        let _router = router();
    }
}
