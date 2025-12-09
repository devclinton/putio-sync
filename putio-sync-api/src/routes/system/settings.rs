//! Settings management routes
//!
//! Provides REST API endpoints for system settings:
//! - GET  /settings - Get current settings
//! - PUT  /settings - Update settings

use crate::error::ApiError;
use axum::{
    extract::State,
    routing::{get, put},
    Json, Router,
};
use putio_sync_core::config::Config;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Application state
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<Config>>,
}

/// Settings response (simplified view of Config)
#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsResponse {
    pub workers: usize,
    pub buffer_size_mb: usize,
    pub delete_after_download: bool,
    pub skip_existing: bool,
    pub target_directory: String,
}

/// Success response
#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub success: bool,
    pub message: String,
}

/// Create the settings router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_settings))
        .route("/", put(update_settings))
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
        success: true,
        message: "Settings updated successfully".to_string(),
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
