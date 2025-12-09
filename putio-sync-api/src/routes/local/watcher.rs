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

    // TODO: Get actual watcher status from daemon/file manager
    // For now, return configuration-based status
    Ok(Json(WatcherStatusResponse {
        enabled: config.watch.enabled,
        watching: config.watch.enabled, // TODO: Get from actual watcher
        watch_paths: config
            .watch
            .paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
        files_found: 0,      // TODO: Get from watcher
        files_processed: 0,  // TODO: Get from watcher
    }))
}

/// Manually trigger a scan
///
/// POST /local/watcher/scan
async fn trigger_scan(
    State(_state): State<AppState>,
) -> Result<Json<ScanResponse>, ApiError> {
    // TODO: Implement actual scan trigger
    // This would send a message to the file watcher to scan now

    Ok(Json(ScanResponse {
        message: "Scan triggered (not yet implemented)".to_string(),
        files_found: 0,
        files_processed: 0,
    }))
}

/// Start file watcher
///
/// POST /local/watcher/start
async fn start_watcher(
    State(state): State<AppState>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let mut config = state.config.write().await;

    // Enable watcher in configuration
    config.watch.enabled = true;

    // TODO: Actually start the file watcher if it's not running
    // This would send a message to the daemon to start watching

    Ok(Json(SuccessResponse {
        message: "File watcher started (configuration updated)".to_string(),
    }))
}

/// Stop file watcher
///
/// POST /local/watcher/stop
async fn stop_watcher(
    State(state): State<AppState>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let mut config = state.config.write().await;

    // Disable watcher in configuration
    config.watch.enabled = false;

    // TODO: Actually stop the file watcher if it's running
    // This would send a message to the daemon to stop watching

    Ok(Json(SuccessResponse {
        message: "File watcher stopped (configuration updated)".to_string(),
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
