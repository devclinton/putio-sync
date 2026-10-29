//! Active transfers pipeline route (feature 039, contracts/api.md)
//!
//! - GET /active - every transfer on its way to the NAS, by stage and state, with counts
//!
//! Put.io figures come from the shared transfers snapshot, at most [`MAX_AGE`] old; NAS
//! figures are read on each request.

use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::{extract::State, Json};
use putio_sync_db::pipeline::NAS_DOWNLOADS_PAUSED;
use putio_sync_download::pipeline::Pipeline;
use putio_sync_download::DownloadError;
use serde::Serialize;
use std::time::Duration;

/// How old the Put.io transfer list may be for a page (R3)
pub const MAX_AGE: Duration = Duration::from_secs(10);

/// Response of `GET /active`
#[derive(Debug, Serialize)]
pub struct ActiveResponse {
    #[serde(flatten)]
    pub pipeline: Pipeline,
    /// The Pause NAS downloads switch is on
    pub nas_paused: bool,
    /// Items that left the list within `[transfers] finished_retention_hours`
    pub recently_finished: Vec<serde_json::Value>,
}

/// GET /active
pub async fn get_active(State(state): State<AppState>) -> ApiResult<Json<ActiveResponse>> {
    let db = state.database.as_ref().ok_or_else(|| {
        ApiError::ServiceUnavailable("The Active page needs the database".to_string())
    })?;
    let retention = state.config.read().await.transfers.finished_retention_hours;
    let pipeline = state
        .download_manager
        .pipeline(MAX_AGE, &state.stall_tracker, retention)
        .await
        .map_err(|e| match e {
            DownloadError::ClientError(e) => {
                ApiError::ServiceUnavailable(format!("Put.io is unreachable: {e}"))
            }
            e => ApiError::InternalError(e.to_string()),
        })?;
    let nas_paused = db
        .flags()
        .get_bool(NAS_DOWNLOADS_PAUSED)
        .await
        .map_err(|e| ApiError::InternalError(e.to_string()))?;
    Ok(Json(ActiveResponse {
        pipeline,
        nas_paused,
        recently_finished: Vec::new(),
    }))
}
