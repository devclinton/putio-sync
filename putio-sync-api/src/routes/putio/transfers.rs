//! Put.io transfer proxy routes
//!
//! Provides REST API endpoints for Put.io transfer operations:
//! - GET  /putio/transfers        - List all transfers
//! - POST /putio/transfers        - Add transfer (multipart for .torrent, JSON for magnet)
//! - GET  /putio/transfers/:id    - Get specific transfer stats
//! - DELETE /putio/transfers/:id  - Remove transfer
//! - POST /putio/transfers/:id/start - Start/resume transfer
//! - POST /putio/transfers/:id/stop  - Stop transfer

use crate::error::ApiError;
use crate::handlers::transfers;
use crate::models::transfers::{AddTransferRequest, TransferResponse};
use crate::models::{ListResponse, SuccessResponse};
use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};

// Re-export AppState from parent
pub use crate::AppState;

/// Create the Put.io transfers router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_transfers))
        .route("/", post(add_transfer))
        .route("/:id", get(get_transfer))
        .route("/:id", delete(remove_transfer))
        .route("/:id/start", post(start_transfer))
        .route("/:id/stop", post(stop_transfer))
}

/// List all transfers
///
/// GET /putio/transfers
async fn list_transfers(
    State(state): State<AppState>,
) -> Result<Json<ListResponse<TransferResponse>>, ApiError> {
    let config = state.config.read().await;
    transfers::list_transfers(&config).await.map(Json)
}

/// Add a new transfer
///
/// POST /putio/transfers
async fn add_transfer(
    State(state): State<AppState>,
    Json(request): Json<AddTransferRequest>,
) -> Result<Json<TransferResponse>, ApiError> {
    let config = state.config.read().await;
    transfers::add_transfer(&config, request).await.map(Json)
}

/// Get transfer details
///
/// GET /putio/transfers/:id
async fn get_transfer(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<TransferResponse>, ApiError> {
    let config = state.config.read().await;
    transfers::get_transfer(&config, transfer_id)
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
    let config = state.config.read().await;
    transfers::remove_transfer(&config, transfer_id)
        .await
        .map(Json)
}

/// Start/resume a transfer
///
/// POST /putio/transfers/:id/start
async fn start_transfer(
    State(_state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<SuccessResponse>, ApiError> {
    // Put.io doesn't have explicit start/stop API - transfers start automatically
    // This endpoint is provided for API consistency but is a no-op
    Ok(Json(SuccessResponse {
        message: format!(
            "Transfer {} is managed automatically by Put.io",
            transfer_id
        ),
    }))
}

/// Stop a transfer
///
/// POST /putio/transfers/:id/stop
async fn stop_transfer(
    State(_state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<SuccessResponse>, ApiError> {
    // Put.io doesn't have explicit start/stop API
    // Users should delete the transfer to stop it
    Ok(Json(SuccessResponse {
        message: format!(
            "To stop transfer {}, use DELETE /putio/transfers/{}",
            transfer_id, transfer_id
        ),
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
