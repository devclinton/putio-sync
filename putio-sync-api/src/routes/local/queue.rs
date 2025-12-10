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

/// Queue statistics response
#[derive(Debug, Serialize)]
pub struct QueueStatsResponse {
    pub total: usize,
    pub pending: usize,
    pub downloading: usize,
    pub completed: usize,
    pub failed: usize,
    pub paused: usize,
}

/// Create the queue router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_queue))
        .route("/", put(update_queue))
        .route("/stats", get(get_stats))
        .route("/clear", post(clear_completed))
        .route("/pause", post(pause_all))
        .route("/resume", post(resume_all))
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
    State(state): State<AppState>,
    Path(file_id_str): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let file_id: i64 = file_id_str
        .parse()
        .map_err(|_| ApiError::BadRequest("Invalid file ID".to_string()))?;

    let db = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?;

    let queue_repo = db.queue();

    // Get all items sorted by priority
    let mut items = queue_repo.get_all().await.map_err(|e| {
        ApiError::InternalError(format!("Failed to get queue: {}", e))
    })?;

    // Sort by priority (lower number = higher priority)
    items.sort_by_key(|item| item.priority);

    // Find the item and swap with previous
    if let Some(pos) = items.iter().position(|item| item.file_id == file_id) {
        if pos == 0 {
            return Err(ApiError::BadRequest("Item is already first".to_string()));
        }

        let current_priority = items[pos].priority;
        let prev_priority = items[pos - 1].priority;

        // Swap priorities
        queue_repo
            .update_priority(file_id, prev_priority)
            .await
            .map_err(|e| ApiError::InternalError(format!("Failed to update priority: {}", e)))?;

        queue_repo
            .update_priority(items[pos - 1].file_id, current_priority)
            .await
            .map_err(|e| ApiError::InternalError(format!("Failed to update priority: {}", e)))?;

        Ok(Json(SuccessResponse {
            message: "Item moved up in queue".to_string(),
        }))
    } else {
        Err(ApiError::NotFound("Item not found in queue".to_string()))
    }
}

/// Move queue item down
///
/// POST /local/queue/*path/move_down
async fn move_down(
    State(state): State<AppState>,
    Path(file_id_str): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let file_id: i64 = file_id_str
        .parse()
        .map_err(|_| ApiError::BadRequest("Invalid file ID".to_string()))?;

    let db = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?;

    let queue_repo = db.queue();

    // Get all items sorted by priority
    let mut items = queue_repo.get_all().await.map_err(|e| {
        ApiError::InternalError(format!("Failed to get queue: {}", e))
    })?;

    // Sort by priority (lower number = higher priority)
    items.sort_by_key(|item| item.priority);

    // Find the item and swap with next
    if let Some(pos) = items.iter().position(|item| item.file_id == file_id) {
        if pos >= items.len() - 1 {
            return Err(ApiError::BadRequest("Item is already last".to_string()));
        }

        let current_priority = items[pos].priority;
        let next_priority = items[pos + 1].priority;

        // Swap priorities
        queue_repo
            .update_priority(file_id, next_priority)
            .await
            .map_err(|e| ApiError::InternalError(format!("Failed to update priority: {}", e)))?;

        queue_repo
            .update_priority(items[pos + 1].file_id, current_priority)
            .await
            .map_err(|e| ApiError::InternalError(format!("Failed to update priority: {}", e)))?;

        Ok(Json(SuccessResponse {
            message: "Item moved down in queue".to_string(),
        }))
    } else {
        Err(ApiError::NotFound("Item not found in queue".to_string()))
    }
}

/// Move queue item to first position
///
/// POST /local/queue/*path/move_first
async fn move_first(
    State(state): State<AppState>,
    Path(file_id_str): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let file_id: i64 = file_id_str
        .parse()
        .map_err(|_| ApiError::BadRequest("Invalid file ID".to_string()))?;

    let db = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?;

    let queue_repo = db.queue();

    // Get all items sorted by priority
    let mut items = queue_repo.get_all().await.map_err(|e| {
        ApiError::InternalError(format!("Failed to get queue: {}", e))
    })?;

    // Sort by priority (lower number = higher priority)
    items.sort_by_key(|item| item.priority);

    // Find the item
    if let Some(pos) = items.iter().position(|item| item.file_id == file_id) {
        if pos == 0 {
            return Err(ApiError::BadRequest("Item is already first".to_string()));
        }

        // Get the minimum priority (first item's priority - 1)
        let new_priority = items[0].priority - 1;

        // Move item to first by giving it lowest priority number
        queue_repo
            .update_priority(file_id, new_priority)
            .await
            .map_err(|e| ApiError::InternalError(format!("Failed to update priority: {}", e)))?;

        Ok(Json(SuccessResponse {
            message: "Item moved to first position in queue".to_string(),
        }))
    } else {
        Err(ApiError::NotFound("Item not found in queue".to_string()))
    }
}

/// Move queue item to last position
///
/// POST /local/queue/*path/move_last
async fn move_last(
    State(state): State<AppState>,
    Path(file_id_str): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let file_id: i64 = file_id_str
        .parse()
        .map_err(|_| ApiError::BadRequest("Invalid file ID".to_string()))?;

    let db = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?;

    let queue_repo = db.queue();

    // Get all items sorted by priority
    let mut items = queue_repo.get_all().await.map_err(|e| {
        ApiError::InternalError(format!("Failed to get queue: {}", e))
    })?;

    // Sort by priority (lower number = higher priority)
    items.sort_by_key(|item| item.priority);

    // Find the item
    if let Some(pos) = items.iter().position(|item| item.file_id == file_id) {
        if pos >= items.len() - 1 {
            return Err(ApiError::BadRequest("Item is already last".to_string()));
        }

        // Get the maximum priority (last item's priority + 1)
        let new_priority = items.last().unwrap().priority + 1;

        // Move item to last by giving it highest priority number
        queue_repo
            .update_priority(file_id, new_priority)
            .await
            .map_err(|e| ApiError::InternalError(format!("Failed to update priority: {}", e)))?;

        Ok(Json(SuccessResponse {
            message: "Item moved to last position in queue".to_string(),
        }))
    } else {
        Err(ApiError::NotFound("Item not found in queue".to_string()))
    }
}

/// Skip queue item
///
/// POST /local/queue/*path/skip
async fn skip_item(
    State(state): State<AppState>,
    Path(file_id_str): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let file_id: i64 = file_id_str
        .parse()
        .map_err(|_| ApiError::BadRequest("Invalid file ID".to_string()))?;

    let db = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?;

    let queue_repo = db.queue();

    // Remove the item from queue
    queue_repo
        .remove(file_id)
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to skip item: {}", e)))?;

    Ok(Json(SuccessResponse {
        message: format!("Item {} skipped and removed from queue", file_id),
    }))
}

/// Get queue statistics
///
/// GET /local/queue/stats
async fn get_stats(
    State(state): State<AppState>,
) -> Result<Json<QueueStatsResponse>, ApiError> {
    let db = state.database.as_ref().ok_or_else(|| {
        ApiError::InternalError("Database not available".to_string())
    })?;

    let queue_repo = db.queue();
    let stats = queue_repo.get_stats().await.map_err(|e| {
        ApiError::InternalError(format!("Failed to get queue stats: {}", e))
    })?;

    Ok(Json(QueueStatsResponse {
        total: stats.total as usize,
        pending: stats.pending as usize,
        downloading: stats.downloading as usize,
        completed: stats.completed as usize,
        failed: stats.failed as usize,
        paused: stats.paused as usize,
    }))
}

/// Clear completed items from queue
///
/// POST /local/queue/clear
async fn clear_completed(
    State(state): State<AppState>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let db = state.database.as_ref().ok_or_else(|| {
        ApiError::InternalError("Database not available".to_string())
    })?;

    let queue_repo = db.queue();
    let count = queue_repo.clear_completed().await.map_err(|e| {
        ApiError::InternalError(format!("Failed to clear completed items: {}", e))
    })?;

    Ok(Json(SuccessResponse {
        message: format!("Cleared {} completed items from queue", count),
    }))
}

/// Pause all active downloads
///
/// POST /local/queue/pause
async fn pause_all(
    State(state): State<AppState>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let db = state.database.as_ref().ok_or_else(|| {
        ApiError::InternalError("Database not available".to_string())
    })?;

    let queue_repo = db.queue();
    let count = queue_repo.pause_all().await.map_err(|e| {
        ApiError::InternalError(format!("Failed to pause queue: {}", e))
    })?;

    Ok(Json(SuccessResponse {
        message: format!("Paused {} items in queue", count),
    }))
}

/// Resume all paused downloads
///
/// POST /local/queue/resume
async fn resume_all(
    State(state): State<AppState>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let db = state.database.as_ref().ok_or_else(|| {
        ApiError::InternalError("Database not available".to_string())
    })?;

    let queue_repo = db.queue();
    let count = queue_repo.resume_all().await.map_err(|e| {
        ApiError::InternalError(format!("Failed to resume queue: {}", e))
    })?;

    Ok(Json(SuccessResponse {
        message: format!("Resumed {} items in queue", count),
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
