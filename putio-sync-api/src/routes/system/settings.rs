//! Settings management routes
//!
//! Provides REST API endpoints for system settings:
//! - GET  /settings - Get current settings
//! - PUT  /settings - Update settings
//! - GET  /settings/watcher - Get file watcher configuration
//! - PUT  /settings/watcher - Update file watcher configuration

use crate::error::ApiError;
use crate::models::SuccessResponse;
use axum::{
    extract::State,
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Re-export AppState from parent
pub use crate::AppState;
use putio_sync_core::config::{NotificationBackend, ProgressStyle, SortOrder};
use putio_sync_core::logging::ConsoleFormat;

/// Helper function to persist config to file
async fn persist_config(state: &AppState) -> Result<(), ApiError> {
    if let Some(ref config_path) = state.config_path {
        let config = state.config.read().await;
        config.save(config_path.as_ref()).map_err(|e| {
            ApiError::InternalError(format!("Failed to persist config: {}", e))
        })?;
    }
    Ok(())
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
        "byextension" | "by-extension" | "by_extension" | "extension" => {
            Ok(SortOrder::ByExtension)
        }
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
}

/// Get download settings
///
/// GET /settings/download
async fn get_download_settings(
    State(state): State<AppState>,
) -> Result<Json<DownloadSettings>, ApiError> {
    let config = state.config.read().await;

    Ok(Json(DownloadSettings {
        workers: config.download.workers,
        target_directory: config.download.target_directory.to_string_lossy().to_string(),
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
        bandwidth_limit_bytes_per_sec: config.download.bandwidth_limit_bytes_per_sec,
        max_retries: config.download.max_retries,
        retry_initial_delay_secs: config.download.retry_initial_delay_secs,
        delete_empty_folders: config.download.delete_empty_folders,
    }))
}

/// Update download settings
///
/// PUT /settings/download
async fn update_download_settings(
    State(state): State<AppState>,
    Json(settings): Json<DownloadSettings>,
) -> Result<Json<SuccessResponse>, ApiError> {
    // Parse enums before acquiring write lock
    let progress_style = parse_progress_style(&settings.progress_style)?;
    let sort_order = parse_sort_order(&settings.sort_order)?;

    {
        let mut config = state.config.write().await;

        // Update configuration
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
        config.download.bandwidth_limit_bytes_per_sec = settings.bandwidth_limit_bytes_per_sec;
        config.download.max_retries = settings.max_retries;
        config.download.retry_initial_delay_secs = settings.retry_initial_delay_secs;
        config.download.delete_empty_folders = settings.delete_empty_folders;
    }

    // Hot reload: Update download manager configuration dynamically
    // This updates the shared config without restarting workers or losing queue state.
    // Changes take effect immediately:
    // - Bandwidth limits applied to active and new downloads
    // - Worker count changes (future: will scale workers gracefully)
    // - Buffer sizes applied to new downloads
    {
        let download_config = {
            let cfg = state.config.read().await;
            crate::extract_download_config(&cfg)
        };

        // Update config via hot reload - workers will see changes immediately
        state.download_manager.update_config(download_config).await;
    }

    // Persist config to file
    persist_config(&state).await?;

    Ok(Json(SuccessResponse {
        message: "Download settings updated successfully via hot reload. Changes applied without restarting workers.".to_string(),
    }))
}

/// Get file watcher configuration
///
/// GET /settings/watcher
async fn get_watcher_config(
    State(state): State<AppState>,
) -> Result<Json<WatcherConfig>, ApiError> {
    let config = state.config.read().await;

    Ok(Json(WatcherConfig {
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
    }))
}

/// Update file watcher configuration
///
/// PUT /settings/watcher
async fn update_watcher_config(
    State(state): State<AppState>,
    Json(watcher): Json<WatcherConfig>,
) -> Result<Json<SuccessResponse>, ApiError> {
    use putio_sync_file_manager::watcher::WatcherConfig as FileWatcherConfig;
    use std::time::Duration;

    let watch_paths: Vec<PathBuf> = watcher.paths.into_iter().map(PathBuf::from).collect();
    let archive_dir = watcher.archive_directory.map(PathBuf::from);

    {
        let mut config = state.config.write().await;

        // Update file watcher configuration
        config.watch.enabled = watcher.enabled;
        config.watch.paths = watch_paths.clone();
        config.watch.refresh_cycle = watcher.refresh_cycle;
        config.watch.parent_folder_id = watcher.parent_folder_id;
        config.watch.archive_directory = archive_dir.clone();
        config.watch.delete_after_upload = watcher.delete_after_upload;
    }

    // Hot reload: Update file watcher configuration dynamically
    // This updates the shared config without restarting the watcher daemon.
    // Changes take effect on the next scan cycle.
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

    // Persist config to file
    persist_config(&state).await?;

    let message = if state.file_watcher.is_some() {
        "File watcher configuration updated successfully via hot reload. Changes will take effect on next scan cycle.".to_string()
    } else {
        "File watcher configuration updated successfully. Note: Server restart required for changes to take effect (watcher not available for hot reload).".to_string()
    };

    Ok(Json(SuccessResponse { message }))
}

/// Get logging settings
///
/// GET /settings/logging
async fn get_logging_settings(
    State(state): State<AppState>,
) -> Result<Json<LoggingSettings>, ApiError> {
    let config = state.config.read().await;

    Ok(Json(LoggingSettings {
        level: config.logging.level.clone(),
        file_enabled: config.logging.file_enabled,
        file_path: config.logging.file_path.to_string_lossy().to_string(),
        console_enabled: config.logging.console_enabled,
        console_format: format!("{:?}", config.logging.console_format),
        module_filters: config.logging.module_filters.clone(),
    }))
}

/// Update logging settings
///
/// PUT /settings/logging
async fn update_logging_settings(
    State(state): State<AppState>,
    Json(settings): Json<LoggingSettings>,
) -> Result<Json<SuccessResponse>, ApiError> {
    // Parse enum before acquiring write lock
    let console_format = parse_console_format(&settings.console_format)?;

    {
        let mut config = state.config.write().await;

        // Update logging configuration
        config.logging.level = settings.level;
        config.logging.file_enabled = settings.file_enabled;
        config.logging.file_path = settings.file_path.into();
        config.logging.console_enabled = settings.console_enabled;
        config.logging.console_format = console_format;
        config.logging.module_filters = settings.module_filters.clone();
    }

    // Persist config to file
    persist_config(&state).await?;

    // Note: Logging hot reload is not currently supported due to tracing-subscriber's
    // complex layer type system. Changes will take effect after server restart.
    Ok(Json(SuccessResponse {
        message: "Logging configuration updated successfully. Note: Server restart required for changes to take effect.".to_string(),
    }))
}

/// Get notification settings
///
/// GET /settings/notifications
async fn get_notification_settings(
    State(state): State<AppState>,
) -> Result<Json<NotificationSettings>, ApiError> {
    let config = state.config.read().await;

    Ok(Json(NotificationSettings {
        enabled: config.notifications.enabled,
        backend: format!("{:?}", config.notifications.backend),
        webhook_url: config.notifications.webhook_url.clone(),
        email: config.notifications.email.clone(),
    }))
}

/// Update notification settings
///
/// PUT /settings/notifications
async fn update_notification_settings(
    State(state): State<AppState>,
    Json(settings): Json<NotificationSettings>,
) -> Result<Json<SuccessResponse>, ApiError> {
    // Parse enum before acquiring write lock
    let backend = parse_notification_backend(&settings.backend)?;

    {
        let mut config = state.config.write().await;

        // Update notification configuration
        config.notifications.enabled = settings.enabled;
        config.notifications.backend = backend;
        config.notifications.webhook_url = settings.webhook_url;
        config.notifications.email = settings.email;

        // Note: Notification system hot reload is not currently implemented.
        // The notification system is initialized at startup and changes require a server restart.
        // Future enhancement: Add dynamic notification backend reinitialization.
    }

    // Persist config to file
    persist_config(&state).await?;

    Ok(Json(SuccessResponse {
        message: "Notification configuration updated successfully. Note: Server restart required for changes to take effect.".to_string(),
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
