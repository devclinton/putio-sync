//! Put.io empty-folder cleanup route
//!
//! - POST /putio/cleanup/empty-folders - Remove empty folders (or preview with `dry_run`)

use crate::error::ApiError;
use axum::{
    extract::{rejection::JsonRejection, State},
    routing::post,
    Json, Router,
};
use putio_sync_client::EmptyFolderReport;
use putio_sync_core::cleanup::{CleanupError, CleanupTrigger};
use serde::{Deserialize, Serialize};
use std::time::Instant;

pub use crate::AppState;

/// Create the Put.io cleanup router
pub fn router() -> Router<AppState> {
    Router::new().route("/empty-folders", post(clean_empty_folders))
}

/// Request body; `dry_run` is required so nothing is deleted by accident
#[derive(Debug, Deserialize)]
pub struct CleanupRequest {
    /// Report what would be removed without deleting
    pub dry_run: bool,
}

/// The run's report, with the count and how long it took
#[derive(Debug, Serialize)]
pub struct CleanupResponse {
    /// Folders removed (or that would be)
    pub removed_count: usize,
    /// How long the run took
    pub duration_ms: u64,
    /// The run's report
    #[serde(flatten)]
    pub report: EmptyFolderReport,
}

/// Remove empty folders from Put.io
///
/// POST /putio/cleanup/empty-folders `{"dry_run": true}`
///
/// Runs to the end before answering. 409 while another run (manual or daily) is going.
async fn clean_empty_folders(
    State(state): State<AppState>,
    request: Result<Json<CleanupRequest>, JsonRejection>,
) -> Result<Json<CleanupResponse>, ApiError> {
    let Json(request) = request.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    let started = Instant::now();
    let report = state
        .cleanup
        .run(CleanupTrigger::Manual, request.dry_run)
        .await
        .map_err(|e| match e {
            CleanupError::AlreadyRunning => ApiError::Conflict(e.to_string()),
            CleanupError::Client(e) => ApiError::Client(e),
        })?;
    Ok(Json(CleanupResponse {
        removed_count: report.removed.len(),
        duration_ms: started.elapsed().as_millis() as u64,
        report,
    }))
}
