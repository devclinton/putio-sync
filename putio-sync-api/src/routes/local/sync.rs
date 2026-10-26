//! "Sync now" route (feature 004, FR-013)
//!
//! - POST /local/sync - Start a folder sync now; refused while one runs

use crate::error::ApiError;
use crate::routes::system::status::{sync_json, SyncSection};
use axum::{extract::State, http::StatusCode, Json};
use putio_sync_download::SyncRequestError;

// Re-export AppState from parent
pub use crate::AppState;

/// Start a folder sync now
///
/// POST /local/sync
///
/// 202 with the `sync` object of `/status` (`in_progress: true`); 409 while a sync
/// runs; 503 without a database, since the sync has nowhere to queue files.
pub async fn sync_now(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<SyncSection>), ApiError> {
    if state.database.is_none() {
        return Err(ApiError::ServiceUnavailable(
            "Database unavailable".to_string(),
        ));
    }

    match state.download_manager.request_sync().await {
        Ok(()) => {
            let status = state.download_manager.sync_status().await;
            Ok((StatusCode::ACCEPTED, Json(sync_json(&status))))
        }
        Err(SyncRequestError::InProgress) => Err(ApiError::Conflict(
            "A sync is already in progress".to_string(),
        )),
    }
}
