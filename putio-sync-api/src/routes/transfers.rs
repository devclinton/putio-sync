//! Transfer management routes

use crate::handlers::transfers;
use crate::models::transfers::{AddTransferRequest, CancelTransfersRequest, TransferResponse};
use crate::models::{ListResponse, SuccessResponse};
use crate::routes::files::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};

/// Create the transfers router
///
/// # Returns
///
/// Router with all transfers endpoints configured
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_transfers_handler))
        .route("/", post(add_transfer_handler))
        .route("/:id", get(get_transfer_handler))
        .route("/:id", delete(cancel_transfer_handler))
        .route("/:id/retry", post(retry_transfer_handler))
        .route("/cancel", post(cancel_transfers_bulk_handler))
}

/// Handler for listing transfers
///
/// GET /api/transfers
async fn list_transfers_handler(
    State(state): State<AppState>,
) -> Result<Json<ListResponse<TransferResponse>>, (StatusCode, String)> {
    transfers::list_transfers(&state.config)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for adding a new transfer
///
/// POST /api/transfers
async fn add_transfer_handler(
    State(state): State<AppState>,
    Json(request): Json<AddTransferRequest>,
) -> Result<Json<TransferResponse>, (StatusCode, String)> {
    transfers::add_transfer(&state.config, request)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for getting a single transfer
///
/// GET /api/transfers/:id
async fn get_transfer_handler(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<TransferResponse>, (StatusCode, String)> {
    transfers::get_transfer(&state.config, transfer_id)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for cancelling a single transfer
///
/// DELETE /api/transfers/:id
async fn cancel_transfer_handler(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<SuccessResponse>, (StatusCode, String)> {
    let request = CancelTransfersRequest {
        transfer_ids: vec![transfer_id],
    };

    transfers::cancel_transfers(&state.config, request)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for retrying a transfer
///
/// POST /api/transfers/:id/retry
async fn retry_transfer_handler(
    State(state): State<AppState>,
    Path(transfer_id): Path<i64>,
) -> Result<Json<TransferResponse>, (StatusCode, String)> {
    transfers::retry_transfer(&state.config, transfer_id)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for bulk cancelling transfers
///
/// POST /api/transfers/cancel
async fn cancel_transfers_bulk_handler(
    State(state): State<AppState>,
    Json(request): Json<CancelTransfersRequest>,
) -> Result<Json<SuccessResponse>, (StatusCode, String)> {
    transfers::cancel_transfers(&state.config, request)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_creation() {
        let _router = router();
        // Just ensure the router can be created without panicking
    }
}
