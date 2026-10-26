//! Put.io transfer proxy routes
//!
//! Provides REST API endpoints for Put.io transfer operations:
//! - GET  /putio/transfers        - List all transfers
//! - POST /putio/transfers        - Add transfer (multipart for .torrent, JSON for magnet);
//!   records it as added by hand for the Stats page (feature 006)
//! - GET  /putio/transfers/:id    - Get specific transfer stats
//! - DELETE /putio/transfers/:id  - Remove transfer (Put.io cancel)
//! - POST /putio/transfers/:id/retry - Retry a failed transfer
//! - POST /putio/transfers/:id/readd - Cancel a transfer and add its magnet again
//!
//! Put.io has no pause or resume for transfers, so there are no start/stop routes:
//! retry restarts a failed one, remove stops and deletes one.

use crate::error::ApiError;
use crate::handlers::transfers;
use crate::models::transfers::{AddTransferRequest, ReaddResponse, TransferResponse};
use crate::models::{ListResponse, SuccessResponse};
use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use putio_sync_db::models::{Origin, TransferOrigin};
use tracing::warn;

// Re-export AppState from parent
pub use crate::AppState;

/// Create the Put.io transfers router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_transfers))
        .route("/", post(add_transfer))
        .route("/:id", get(get_transfer))
        .route("/:id", delete(remove_transfer))
        .route("/:id/retry", post(retry_transfer))
        .route("/:id/readd", post(readd_transfer))
}

/// List all transfers
///
/// GET /putio/transfers
async fn list_transfers(
    State(state): State<AppState>,
) -> Result<Json<ListResponse<TransferResponse>>, ApiError> {
    transfers::list_transfers(&state.client, &state.stall_tracker)
        .await
        .map(Json)
}

/// Add a new transfer
///
/// POST /putio/transfers
async fn add_transfer(
    State(state): State<AppState>,
    Json(request): Json<AddTransferRequest>,
) -> Result<Json<TransferResponse>, ApiError> {
    let transfer = transfers::add_transfer(&state.client, request).await?;
    if let Some(db) = state.database.as_ref() {
        let origin = TransferOrigin {
            origin: Origin::Manual,
            watch_folder: None,
        };
        if let Err(e) = db
            .sources()
            .record_transfer_origin(transfer.id, &origin)
            .await
        {
            warn!(transfer_id = transfer.id, error = %e, "Failed to record transfer origin");
        }
    }
    Ok(Json(transfer))
}

/// Get transfer details
///
/// GET /putio/transfers/:id
async fn get_transfer(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<TransferResponse>, ApiError> {
    transfers::get_transfer(&state.client, &state.stall_tracker, transfer_id)
        .await
        .map(Json)
}

/// Remove a transfer
///
/// DELETE /putio/transfers/:id
async fn remove_transfer(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<SuccessResponse>, ApiError> {
    transfers::remove_transfer(&state.client, transfer_id)
        .await
        .map(Json)
}

/// Retry a failed transfer; answers with the transfer as Put.io reports it afterwards
///
/// POST /putio/transfers/:id/retry
async fn retry_transfer(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<TransferResponse>, ApiError> {
    transfers::retry_transfer(&state.client, transfer_id)
        .await
        .map(Json)
}

/// Cancel a transfer and add its magnet again to the same folder (feature 007)
///
/// POST /putio/transfers/:id/readd
async fn readd_transfer(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<ReaddResponse>, ApiError> {
    transfers::readd_transfer(&state.client, transfer_id)
        .await
        .map(Json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_creation() {
        let _router = router();
    }
}
