//! Download queue management routes
//!
//! Provides REST API endpoints for download queue operations:
//! - GET  /local/queue               - Get current download queue
//! - PUT  /local/queue               - Update entire queue
//! - POST /local/queue/*path/move_up      - Move item up in queue
//! - POST /local/queue/*path/move_down    - Move item down in queue
//! - POST /local/queue/*path/move_first   - Move item to first position
//! - POST /local/queue/*path/move_last    - Move item to last position
//! - POST /local/queue/*path/skip         - Skip item in queue

use crate::error::ApiError;
use crate::models::SuccessResponse;
use axum::{
    extract::{Path, State},
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};

// Re-export AppState from parent
pub use crate::AppState;

/// Queue item representation
#[derive(Debug, Serialize, Deserialize)]
pub struct QueueItemResponse {
    pub id: i64,
    pub file_id: i64,
    pub filename: String,
    pub file_size: i64,
    pub relative_path: String,
    pub priority: i64,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Queue response
#[derive(Debug, Serialize)]
pub struct QueueResponse {
    pub items: Vec<QueueItemResponse>,
    pub total: usize,
}

/// Create the queue router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_queue))
        .route("/", put(update_queue))
        .route("/*path/move_up", post(move_up))
        .route("/*path/move_down", post(move_down))
        .route("/*path/move_first", post(move_first))
        .route("/*path/move_last", post(move_last))
        .route("/*path/skip", post(skip_item))
}

/// Get current download queue
///
/// GET /local/queue
async fn get_queue(
    State(state): State<AppState>,
) -> Result<Json<QueueResponse>, ApiError> {
    let db = state.database.as_ref().ok_or_else(|| {
        ApiError::InternalError("Database not available".to_string())
    })?;

    let queue_repo = db.queue();
    let items = queue_repo.get_all().await.map_err(|e| {
        ApiError::InternalError(format!("Failed to get queue: {}", e))
    })?;

    let response_items: Vec<QueueItemResponse> = items
        .into_iter()
        .map(|item| QueueItemResponse {
            id: item.id,
            file_id: item.file_id,
            filename: item.filename,
            file_size: item.file_size,
            relative_path: item.relative_path,
            priority: item.priority,
            status: format!("{:?}", item.status),
            created_at: item.created_at.to_string(),
            updated_at: item.updated_at.to_string(),
        })
        .collect();

    Ok(Json(QueueResponse {
        total: response_items.len(),
        items: response_items,
    }))
}

/// Update entire queue
///
/// PUT /local/queue
///
/// Updates priorities for all items in the queue based on their position in the list.
async fn update_queue(
    State(state): State<AppState>,
    Json(items): Json<Vec<QueueItemResponse>>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let db = state.database.as_ref().ok_or_else(|| {
        ApiError::InternalError("Database not available".to_string())
    })?;

    let queue_repo = db.queue();

    // Update each item's priority based on its position in the list
    for (index, item) in items.iter().enumerate() {
        queue_repo
            .update_priority(item.file_id, index as i64)
            .await
            .map_err(|e| {
                ApiError::InternalError(format!("Failed to update priority: {}", e))
            })?;
    }

    Ok(Json(SuccessResponse {
        message: format!("Updated priorities for {} items", items.len()),
    }))
}

/// Move queue item up
///
/// POST /local/queue/*path/move_up
async fn move_up(
    State(_state): State<AppState>,
    Path(_item_path): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    // TODO: Implement with Database in Task 4
    Ok(Json(SuccessResponse {
        message: "Item moved up (not implemented yet)".to_string(),
    }))
}

/// Move queue item down
///
/// POST /local/queue/*path/move_down
async fn move_down(
    State(_state): State<AppState>,
    Path(_item_path): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    // TODO: Implement with Database in Task 4
    Ok(Json(SuccessResponse {
        message: "Item moved down (not implemented yet)".to_string(),
    }))
}

/// Move queue item to first position
///
/// POST /local/queue/*path/move_first
async fn move_first(
    State(_state): State<AppState>,
    Path(_item_path): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    // TODO: Implement with Database in Task 4
    Ok(Json(SuccessResponse {
        message: "Item moved to first (not implemented yet)".to_string(),
    }))
}

/// Move queue item to last position
///
/// POST /local/queue/*path/move_last
async fn move_last(
    State(_state): State<AppState>,
    Path(_item_path): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    // TODO: Implement with Database in Task 4
    Ok(Json(SuccessResponse {
        message: "Item moved to last (not implemented yet)".to_string(),
    }))
}

/// Skip queue item
///
/// POST /local/queue/*path/skip
async fn skip_item(
    State(_state): State<AppState>,
    Path(_item_path): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    // TODO: Implement with Database in Task 4
    Ok(Json(SuccessResponse {
        message: "Item skipped (not implemented yet)".to_string(),
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
