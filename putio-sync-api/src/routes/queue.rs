//! Queue management routes

use crate::handlers::queue;
use crate::models::queue::{
    AddToQueueRequest, ChangePriorityRequest, QueueItemResponse, QueueStatsResponse,
    RemoveFromQueueRequest, ReorderQueueRequest,
};
use crate::models::{ListResponse, SuccessResponse};
use crate::routes::files::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};

/// Create the queue router
///
/// # Returns
///
/// Router with all queue endpoints configured
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_queue_handler))
        .route("/", post(add_to_queue_handler))
        .route("/", delete(clear_queue_handler))
        .route("/remove", post(remove_from_queue_handler))
        .route("/reorder", post(reorder_queue_handler))
        .route("/:id/priority", post(change_priority_handler))
        .route("/stats", get(get_queue_stats_handler))
}

/// Handler for listing queue items
///
/// GET /api/queue
async fn list_queue_handler(
    State(state): State<AppState>,
) -> Result<Json<ListResponse<QueueItemResponse>>, (StatusCode, String)> {
    queue::list_queue(&state.config)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for adding items to queue
///
/// POST /api/queue
async fn add_to_queue_handler(
    State(state): State<AppState>,
    Json(request): Json<AddToQueueRequest>,
) -> Result<Json<SuccessResponse>, (StatusCode, String)> {
    queue::add_to_queue(&state.config, request)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for removing items from queue
///
/// POST /api/queue/remove
async fn remove_from_queue_handler(
    State(state): State<AppState>,
    Json(request): Json<RemoveFromQueueRequest>,
) -> Result<Json<SuccessResponse>, (StatusCode, String)> {
    queue::remove_from_queue(&state.config, request)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for reordering queue
///
/// POST /api/queue/reorder
async fn reorder_queue_handler(
    State(state): State<AppState>,
    Json(request): Json<ReorderQueueRequest>,
) -> Result<Json<SuccessResponse>, (StatusCode, String)> {
    queue::reorder_queue(&state.config, request)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for changing priority
///
/// POST /api/queue/:id/priority
async fn change_priority_handler(
    State(state): State<AppState>,
    Path(file_id): Path<i64>,
    Json(request): Json<ChangePriorityRequest>,
) -> Result<Json<SuccessResponse>, (StatusCode, String)> {
    queue::change_priority(&state.config, file_id, request)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for getting queue statistics
///
/// GET /api/queue/stats
async fn get_queue_stats_handler(
    State(state): State<AppState>,
) -> Result<Json<QueueStatsResponse>, (StatusCode, String)> {
    queue::get_queue_stats(&state.config)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for clearing the queue
///
/// DELETE /api/queue
async fn clear_queue_handler(
    State(state): State<AppState>,
) -> Result<Json<SuccessResponse>, (StatusCode, String)> {
    queue::clear_queue(&state.config)
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
