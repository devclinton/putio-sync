//! Bandwidth routes (feature 032)
//!
//! - GET /bandwidth - The download limit in force, where it comes from and when it changes
//! - PUT /bandwidth/override - Pause or slow downloads for a while `{"mode", "minutes"}`
//! - DELETE /bandwidth/override - End the pause or slow-down and follow the schedule again

use crate::error::ApiError;
use axum::{
    extract::{rejection::JsonRejection, State},
    routing::{get, put},
    Json, Router,
};
use putio_sync_core::bandwidth::{OverrideMode, Status};
use putio_sync_download::DownloadError;
use serde::Deserialize;

pub use crate::AppState;

/// Length of a pause or slow-down when the request names none
const DEFAULT_MINUTES: u32 = 60;

/// Create the bandwidth router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_bandwidth))
        .route("/override", put(set_override).delete(clear_override))
}

#[derive(Deserialize)]
struct OverrideRequest {
    mode: OverrideMode,
    #[serde(default = "default_minutes")]
    minutes: u32,
}

fn default_minutes() -> u32 {
    DEFAULT_MINUTES
}

fn api_error(e: DownloadError) -> ApiError {
    match e {
        DownloadError::InvalidInput(msg) => ApiError::BadRequest(msg),
        e => ApiError::Internal(e.to_string()),
    }
}

/// GET /bandwidth
async fn get_bandwidth(State(state): State<AppState>) -> Json<Status> {
    Json(state.download_manager.bandwidth().status())
}

/// PUT /bandwidth/override; replaces any override in force
async fn set_override(
    State(state): State<AppState>,
    request: Result<Json<OverrideRequest>, JsonRejection>,
) -> Result<Json<Status>, ApiError> {
    let Json(request) = request.map_err(|e| ApiError::BadRequest(e.body_text()))?;
    let status = state
        .download_manager
        .set_bandwidth_override(request.mode, request.minutes)
        .await
        .map_err(api_error)?;
    tracing::info!(mode = ?request.mode, minutes = request.minutes, "Bandwidth override set");
    Ok(Json(status))
}

/// DELETE /bandwidth/override; 200 even when there was none
async fn clear_override(State(state): State<AppState>) -> Result<Json<Status>, ApiError> {
    let status = state
        .download_manager
        .clear_bandwidth_override()
        .await
        .map_err(api_error)?;
    Ok(Json(status))
}
