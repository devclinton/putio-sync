//! File watcher control routes
//!
//! Provides REST API endpoints for file watcher control:
//! - GET  /local/watcher/status - Get watcher status
//! - POST /local/watcher/scan   - Manually trigger a scan
//! - POST /local/watcher/start  - Start watching
//! - POST /local/watcher/stop   - Stop watching

use crate::error::ApiError;
use crate::models::SuccessResponse;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

// Re-export AppState from parent
pub use crate::AppState;

/// Watcher status response
#[derive(Debug, Serialize, Deserialize)]
pub struct WatcherStatusResponse {
    pub enabled: bool,
    pub watching: bool,
    pub watch_paths: Vec<String>,
    pub files_found: usize,
    pub files_processed: usize,
}

/// Scan result response
#[derive(Debug, Serialize)]
pub struct ScanResponse {
    pub message: String,
    pub files_found: usize,
    pub files_processed: usize,
}

/// Create the watcher router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/status", get(get_status))
        .route("/scan", post(trigger_scan))
        .route("/start", post(start_watcher))
        .route("/stop", post(stop_watcher))
}

/// Get file watcher status
///
/// GET /local/watcher/status
async fn get_status(
    State(state): State<AppState>,
) -> Result<Json<WatcherStatusResponse>, ApiError> {
    let config = state.config.read().await;

    // Get actual watcher stats if available
    let (files_processed, actual_paths) = if let Some(ref watcher) = state.file_watcher {
        let stats = watcher.get_stats();
        (
            stats.total_processed,
            stats
                .watch_directories
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
        )
    } else {
        // Fallback to config if watcher not available
        (
            0,
            config
                .watch
                .paths
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
        )
    };

    Ok(Json(WatcherStatusResponse {
        enabled: config.watch.enabled,
        watching: state.file_watcher.is_some() && config.watch.enabled,
        watch_paths: actual_paths,
        files_found: 0, // Not tracked by watcher
        files_processed,
    }))
}

/// Manually trigger a scan
///
/// POST /local/watcher/scan
async fn trigger_scan(
    State(state): State<AppState>,
) -> Result<Json<ScanResponse>, ApiError> {
    // Check if watcher is available
    let watcher = state
        .file_watcher
        .as_ref()
        .ok_or_else(|| ApiError::BadRequest("File watcher not available (daemon mode required)".to_string()))?;

    // Get configuration
    let config = state.config.read().await;
    let delete_after_upload = config.watch.delete_after_upload;
    drop(config);

    // Trigger scan
    info!("Manual scan triggered via API");
    match watcher.scan_once(&state.client, delete_after_upload, state.database.as_deref()).await {
        Ok(stats) => {
            info!(
                "Manual scan completed: {} found, {} processed, {} skipped, {} failed",
                stats.files_found, stats.files_processed, stats.files_skipped, stats.files_failed
            );

            Ok(Json(ScanResponse {
                message: format!(
                    "Scan completed successfully. Found {} files, processed {}, skipped {}, failed {}",
                    stats.files_found, stats.files_processed, stats.files_skipped, stats.files_failed
                ),
                files_found: stats.files_found,
                files_processed: stats.files_processed,
            }))
        }
        Err(e) => {
            error!("Manual scan failed: {}", e);
            Err(ApiError::InternalError(format!("Scan failed: {}", e)))
        }
    }
}

/// Start file watcher
///
/// POST /local/watcher/start
async fn start_watcher(
    State(state): State<AppState>,
) -> Result<Json<SuccessResponse>, ApiError> {
    // Update configuration
    {
        let mut config = state.config.write().await;
        config.watch.enabled = true;
    }

    // Persist to config file if path is available (for daemon hot-reload)
    if let Some(ref config_path) = state.config_path {
        let config = state.config.read().await;
        if let Err(e) = config.save(config_path.as_ref()) {
            error!("Failed to persist config change: {}", e);
            return Err(ApiError::InternalError(format!(
                "Failed to save config: {}",
                e
            )));
        }
        info!("File watcher enabled and persisted to config");
    } else {
        info!("File watcher enabled (in-memory only, no config file)");
    }

    Ok(Json(SuccessResponse {
        message: "File watcher started successfully".to_string(),
    }))
}

/// Stop file watcher
///
/// POST /local/watcher/stop
async fn stop_watcher(
    State(state): State<AppState>,
) -> Result<Json<SuccessResponse>, ApiError> {
    // Update configuration
    {
        let mut config = state.config.write().await;
        config.watch.enabled = false;
    }

    // Persist to config file if path is available (for daemon hot-reload)
    if let Some(ref config_path) = state.config_path {
        let config = state.config.read().await;
        if let Err(e) = config.save(config_path.as_ref()) {
            error!("Failed to persist config change: {}", e);
            return Err(ApiError::InternalError(format!(
                "Failed to save config: {}",
                e
            )));
        }
        info!("File watcher disabled and persisted to config");
    } else {
        info!("File watcher disabled (in-memory only, no config file)");
    }

    Ok(Json(SuccessResponse {
        message: "File watcher stopped successfully".to_string(),
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
