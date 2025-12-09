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

/// Settings response (simplified view of Config)
#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsResponse {
    pub workers: usize,
    pub buffer_size_mb: usize,
    pub delete_after_download: bool,
    pub skip_existing: bool,
    pub target_directory: String,
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

/// Create the settings router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_settings))
        .route("/", put(update_settings))
        .route("/watcher", get(get_watcher_config))
        .route("/watcher", put(update_watcher_config))
}

/// Get current settings
///
/// GET /settings
async fn get_settings(
    State(state): State<AppState>,
) -> Result<Json<SettingsResponse>, ApiError> {
    let config = state.config.read().await;

    Ok(Json(SettingsResponse {
        workers: config.download.workers,
        buffer_size_mb: config.download.buffer_size_mb,
        delete_after_download: config.download.delete_after_download,
        skip_existing: config.download.skip_existing,
        target_directory: config.download.target_directory.to_string_lossy().to_string(),
    }))
}

/// Update settings
///
/// PUT /settings
async fn update_settings(
    State(state): State<AppState>,
    Json(settings): Json<SettingsResponse>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let mut config = state.config.write().await;

    // Update configuration
    config.download.workers = settings.workers;
    config.download.buffer_size_mb = settings.buffer_size_mb;
    config.download.delete_after_download = settings.delete_after_download;
    config.download.skip_existing = settings.skip_existing;
    config.download.target_directory = settings.target_directory.into();

    // TODO: Persist to config file

    Ok(Json(SuccessResponse {
        message: "Settings updated successfully".to_string(),
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
    let mut config = state.config.write().await;

    // Update file watcher configuration
    config.watch.enabled = watcher.enabled;
    config.watch.paths = watcher.paths.into_iter().map(PathBuf::from).collect();
    config.watch.refresh_cycle = watcher.refresh_cycle;
    config.watch.parent_folder_id = watcher.parent_folder_id;
    config.watch.archive_directory = watcher.archive_directory.map(PathBuf::from);
    config.watch.delete_after_upload = watcher.delete_after_upload;

    // TODO: Persist to config file
    // TODO: Notify file watcher to reload configuration

    Ok(Json(SuccessResponse {
        message: "File watcher configuration updated successfully".to_string(),
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
