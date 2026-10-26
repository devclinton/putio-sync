//! Settings management routes
//!
//! Provides REST API endpoints for system settings:
//! - GET  /settings - Get current settings
//! - PUT  /settings - Update settings
//! - GET  /settings/watcher - Get file watcher configuration
//! - PUT  /settings/watcher - Update file watcher configuration
//! - GET  /settings/cleanup - Get the Put.io empty-folder cleanup settings
//! - PUT  /settings/cleanup - Update the Put.io empty-folder cleanup settings

use crate::error::ApiError;
use crate::models::SuccessResponse;
use axum::{
    extract::{rejection::JsonRejection, State},
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Re-export AppState from parent
pub use crate::AppState;
use crate::Engine;
use putio_sync_core::config::{
    CleanupConfig, Config, NotificationBackend, ProgressStyle, SortOrder,
};
use putio_sync_core::error::CoreError;
use putio_sync_core::logging::ConsoleFormat;

/// A malformed or mistyped body is a 400 with a JSON `error`, like a failed validation
fn body<T>(body: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    body.map(|Json(value)| value)
        .map_err(|rejection| ApiError::BadRequest(rejection.body_text()))
}

fn invalid(e: CoreError) -> ApiError {
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
async fn update<V: Serialize>(
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
    *config = candidate;
    Ok(changed)
}

/// Top-level fields that differ between two serialised group bodies
fn changed_fields<V: Serialize>(before: &V, after: &V) -> Vec<String> {
    let (Ok(serde_json::Value::Object(before)), Ok(serde_json::Value::Object(after))) =
        (serde_json::to_value(before), serde_json::to_value(after))
    else {
        return Vec::new();
    };
    after
        .iter()
        .filter(|(name, value)| before.get(*name) != Some(*value))
        .map(|(name, _)| name.clone())
        .collect()
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
    pub skip_existing: bool,
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
}

/// Download fields the engine's download manager picks up for the next download
/// (README "Settings from the UI", checked against the daemon code)
const DOWNLOAD_LIVE: &[&str] = &[
    "workers",
    "bandwidth_limit_bytes_per_sec",
    "buffer_size_mb",
    "max_retries",
    "retry_initial_delay_secs",
    "skip_existing",
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
        skip_existing: config.download.skip_existing,
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
    }
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
    // Parse enums before acquiring write lock
    let progress_style = parse_progress_style(&settings.progress_style)?;
    let sort_order = parse_sort_order(&settings.sort_order)?;

    let mut changed = update(&state, download_view, |config| {
        config.download.workers = settings.workers;
        config.download.target_directory = settings.target_directory.into();
        config.download.buffer_size_mb = settings.buffer_size_mb;
        config.download.skip_existing = settings.skip_existing;
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
    let when = match state.engine {
        Engine::Daemon => "Applies from the next download",
        Engine::Server => {
            if !changed.is_empty() || !notes.is_empty() {
                notes.push(
                    "The server --download background thread keeps its startup settings until a restart."
                        .to_string(),
                );
            }
            "Applies to the API's own download manager from the next download"
        }
    };

    Ok(Json(SuccessResponse {
        message: saved_message("Download", &changed, DOWNLOAD_LIVE, when, &notes),
    }))
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
    use putio_sync_file_manager::watcher::WatcherConfig as FileWatcherConfig;
    use std::time::Duration;

    let watch_paths: Vec<PathBuf> = watcher.paths.into_iter().map(PathBuf::from).collect();
    let archive_dir = watcher.archive_directory.map(PathBuf::from);

    let mut changed = update(&state, watcher_view, |config| {
        config.watch.enabled = watcher.enabled;
        config.watch.paths = watch_paths.clone();
        config.watch.refresh_cycle = watcher.refresh_cycle;
        config.watch.parent_folder_id = watcher.parent_folder_id;
        config.watch.archive_directory = archive_dir.clone();
        config.watch.delete_after_upload = watcher.delete_after_upload;
        Ok(())
    })
    .await?;

    // A watcher shared with the daemon (it started with a watch folder) reads paths,
    // parent folder and archive folder at its next scan. `enabled` and
    // `delete_after_upload` are read by the daemon's watch loop only at startup.
    if let Some(ref file_watcher) = state.file_watcher {
        let new_watcher_config = FileWatcherConfig {
            watch_dirs: watch_paths,
            parent_id: watcher.parent_folder_id,
            interval: Duration::from_secs(watcher.refresh_cycle.max(1) as u64),
            archive_dir,
            delete_after_upload: watcher.delete_after_upload,
        };

        file_watcher.update_config(new_watcher_config);
    }
    // parent_folder_id is protected from the empty-folder cleanup
    state.cleanup.configure(&*state.config.read().await);

    let mut notes = Vec::new();
    let live = match (state.engine, state.file_watcher.is_some()) {
        (Engine::Daemon, true) => {
            if take(&mut changed, "refresh_cycle") {
                notes.push(
                    "refresh_cycle is saved, but the daemon scans on --watch-interval.".to_string(),
                );
            }
            WATCHER_LIVE
        }
        (Engine::Daemon, false) => {
            if !changed.is_empty() {
                notes.push(
                    "The daemon started without a watch folder, so the watcher picks up changes only after a restart."
                        .to_string(),
                );
            }
            &[]
        }
        (Engine::Server, _) => &[],
    };

    Ok(Json(SuccessResponse {
        message: saved_message(
            "Watcher",
            &changed,
            live,
            "Applies at the next scan",
            &notes,
        ),
    }))
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
    // Parse enum before acquiring write lock
    let console_format = parse_console_format(&settings.console_format)?;

    let mut changed = update(&state, logging_view, |config| {
        config.logging.level = settings.level;
        config.logging.file_enabled = settings.file_enabled;
        config.logging.file_path = settings.file_path.into();
        config.logging.console_enabled = settings.console_enabled;
        config.logging.console_format = console_format;
        config.logging.module_filters = settings.module_filters;
        // The poller would refuse a bad level later; refuse it before it is saved
        putio_sync_core::logging::validate_filters(&config.logging).map_err(invalid)
    })
    .await?;

    // The config reload poller (feature 003) applies level and module_filters from the
    // saved file, unless RUST_LOG is set; it runs only if the file existed at startup.
    let mut notes = Vec::new();
    let live = if !state.logging_reload {
        &[]
    } else if std::env::var_os("RUST_LOG").is_some() {
        let level = take(&mut changed, "level");
        let filters = take(&mut changed, "module_filters");
        if level || filters {
            notes.push("level and module_filters are ignored while RUST_LOG is set.".to_string());
        }
        &[]
    } else {
        LOGGING_LIVE
    };

    Ok(Json(SuccessResponse {
        message: saved_message(
            "Logging",
            &changed,
            live,
            "Applies within 10 seconds",
            &notes,
        ),
    }))
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
    // Parse enum before acquiring write lock
    let backend = parse_notification_backend(&settings.backend)?;

    // Notifications are set up at startup, so every change needs a restart
    let changed = update(&state, notification_view, |config| {
        config.notifications.enabled = settings.enabled;
        config.notifications.backend = backend;
        config.notifications.webhook_url = settings.webhook_url;
        config.notifications.email = settings.email;
        Ok(())
    })
    .await?;

    Ok(Json(SuccessResponse {
        message: saved_message("Notification", &changed, &[], "", &[]),
    }))
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
        config.cleanup = CleanupConfig {
            enabled: settings.enabled,
            run_at: settings.run_at,
            min_age_hours: settings.min_age_hours,
        };
        Ok(())
    })
    .await?;

    // The cleanup service reads its settings on every run, and the daemon's schedule on
    // every check
    state.cleanup.configure(&*state.config.read().await);

    let mut notes = Vec::new();
    let live: &[&str] = match state.engine {
        Engine::Daemon => &["enabled", "run_at", "min_age_hours"],
        Engine::Server => {
            if !changed.is_empty() {
                notes.push(
                    "The daily run happens only under `daemon`; manual runs use min_age_hours now."
                        .to_string(),
                );
            }
            &["min_age_hours"]
        }
    };

    Ok(Json(SuccessResponse {
        message: saved_message(
            "Cleanup",
            &changed,
            live,
            "Applies from the next run",
            &notes,
        ),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_creation() {
        let _router = router();
    }
}
