//! Download queue management routes
//!
//! Provides REST API endpoints for download queue operations:
//! - GET  /local/queue               - Get current download queue
//! - POST /local/queue               - Queue a Put.io file or folder (`{file_id}`)
//! - PUT  /local/queue               - Update entire queue
//! - POST /local/queue/<file_id>/move_up    - Move item up in queue
//! - POST /local/queue/<file_id>/move_down  - Move item down in queue
//! - POST /local/queue/<file_id>/move_first - Move item to first position
//! - POST /local/queue/<file_id>/move_last  - Move item to last position
//! - POST /local/queue/<file_id>/skip       - Skip item in queue
//! - POST /local/queue/<file_id>/pause      - Pause a file; a running one stops and keeps its partial
//! - POST /local/queue/<file_id>/resume     - Put a paused file back in the queue
//! - POST /local/queue/<file_id>/retry      - Put a failed file back in the queue
//!
//! `GET /local/queue` takes `?transfer=<id>`, `?folder=<path>` or `?file=<id>` to list one
//! Active page row's files.

use crate::error::ApiError;
use crate::models::SuccessResponse;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post, put},
    Json, Router,
};
use putio_sync_db::models::QueueItem;
use putio_sync_download::DownloadError;
use putio_sync_telemetry::PipelineAction;
use serde::{Deserialize, Serialize};
use tracing::info;

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
    /// The Pause local downloads switch is on
    pub nas_paused: bool,
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

/// Body of `POST /local/queue`
#[derive(Debug, Deserialize)]
pub struct EnqueueRequest {
    /// Put.io file or folder id
    pub file_id: i64,
}

/// Counts from queueing a file or folder, named as the sync outcome names them
#[derive(Debug, Serialize)]
pub struct EnqueueResponse {
    pub found: usize,
    pub queued: usize,
    pub skipped: usize,
    /// Blocked or skipped by a folder rule
    pub blocked: usize,
    pub failed: usize,
    /// Blocked files deleted from Put.io by `delete_blocked` (feature 009)
    pub deleted: usize,
}

/// Create the queue router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_queue))
        .route("/", post(enqueue))
        .route("/", put(update_queue))
        .route("/stats", get(get_stats))
        .route("/clear", post(clear_completed))
        .route("/pause", post(pause_all))
        .route("/resume", post(resume_all))
        .route("/:file_id/move_up", post(move_up))
        .route("/:file_id/move_down", post(move_down))
        .route("/:file_id/move_first", post(move_first))
        .route("/:file_id/move_last", post(move_last))
        .route("/:file_id/skip", post(skip_item))
        .route("/:file_id/pause", post(pause_item))
        .route("/:file_id/resume", post(resume_item))
        .route("/:file_id/retry", post(retry_item))
}

/// Which rows `GET /local/queue` lists; none set lists the whole queue
#[derive(Debug, Default, Deserialize)]
pub struct QueueFilter {
    /// Files of this Put.io transfer
    pub transfer: Option<i64>,
    /// Files written into this folder that belong to no transfer
    pub folder: Option<String>,
    /// This one file
    pub file: Option<i64>,
}

/// Get current download queue
///
/// GET /local/queue
async fn get_queue(
    State(state): State<AppState>,
    Query(filter): Query<QueueFilter>,
) -> Result<Json<QueueResponse>, ApiError> {
    let db = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?;

    let queue_repo = db.queue();
    let items = match (filter.transfer, filter.folder.as_deref(), filter.file) {
        (Some(transfer_id), _, _) => queue_repo.get_for_transfer(transfer_id).await,
        (None, Some(folder), _) => queue_repo.get_for_folder(folder).await,
        (None, None, Some(file_id)) => queue_repo
            .get_by_file_id(file_id)
            .await
            .map(|item| item.into_iter().collect()),
        (None, None, None) => queue_repo.get_all().await,
    }
    .map_err(|e| ApiError::InternalError(format!("Failed to get queue: {}", e)))?;

    let response_items: Vec<QueueItemResponse> = items
        .into_iter()
        .map(|item| QueueItemResponse {
            id: item.id,
            file_id: item.file_id,
            filename: item.filename,
            file_size: item.file_size,
            relative_path: item.relative_path,
            priority: item.priority,
            status: item.status,
            created_at: item.created_at.to_string(),
            updated_at: item.updated_at.to_string(),
        })
        .collect();

    let nas_paused = state.download_manager.nas_paused().await;

    Ok(Json(QueueResponse {
        total: response_items.len(),
        items: response_items,
        nas_paused,
    }))
}

/// Queue a Put.io file, or every file under a Put.io folder, through the folder sync's
/// filters and dedupe (R12)
///
/// POST /local/queue
async fn enqueue(
    State(state): State<AppState>,
    Json(request): Json<EnqueueRequest>,
) -> Result<(StatusCode, Json<EnqueueResponse>), ApiError> {
    if state.database.is_none() {
        return Err(ApiError::ServiceUnavailable(
            "Database unavailable".to_string(),
        ));
    }
    let stats = state
        .download_manager
        .enqueue_tree(request.file_id)
        .await
        .map_err(|e| match e {
            DownloadError::ClientError(ref c) if c.is_not_found() => {
                ApiError::NotFound("File not found".to_string())
            }
            other => ApiError::InternalError(format!("Failed to queue file: {}", other)),
        })?;

    Ok((
        StatusCode::ACCEPTED,
        Json(EnqueueResponse {
            found: stats.total_files,
            queued: stats.files_downloaded,
            skipped: stats.files_skipped,
            blocked: stats.files_blocked,
            failed: stats.files_failed,
            deleted: stats.files_deleted,
        }),
    ))
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
    let db = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?;

    // The first item in the list gets the highest priority, as `GET` lists them
    let order: Vec<i64> = items.iter().map(|item| item.file_id).collect();
    db.queue()
        .set_order(&order)
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to update priority: {}", e)))?;

    Ok(Json(SuccessResponse {
        message: format!("Updated priorities for {} items", items.len()),
    }))
}

/// Where a move sends a queue item
#[derive(Clone, Copy)]
enum Move {
    Up,
    Down,
    First,
    Last,
}

/// Move queue item up
///
/// POST /local/queue/:file_id/move_up
async fn move_up(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    move_item(&state, &file_id, Move::Up).await
}

/// Move queue item down
///
/// POST /local/queue/:file_id/move_down
async fn move_down(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    move_item(&state, &file_id, Move::Down).await
}

/// Move queue item to first position
///
/// POST /local/queue/:file_id/move_first
async fn move_first(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    move_item(&state, &file_id, Move::First).await
}

/// Move queue item to last position
///
/// POST /local/queue/:file_id/move_last
async fn move_last(
    State(state): State<AppState>,
    Path(file_id): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    move_item(&state, &file_id, Move::Last).await
}

/// Move an item within the order `GET /local/queue` lists, which is the order the
/// workers claim pending rows in (highest priority first, then oldest).
///
/// The whole queue is renumbered, because the folder sync queues every file with the
/// same priority and swapping equal priorities would change nothing.
async fn move_item(
    state: &AppState,
    file_id_str: &str,
    to: Move,
) -> Result<Json<SuccessResponse>, ApiError> {
    let file_id: i64 = file_id_str
        .parse()
        .map_err(|_| ApiError::BadRequest("Invalid file ID".to_string()))?;

    let db = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?;

    let queue_repo = db.queue();
    let mut order: Vec<i64> = queue_repo
        .get_all()
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to get queue: {}", e)))?
        .into_iter()
        .map(|item| item.file_id)
        .collect();

    let pos = order
        .iter()
        .position(|id| *id == file_id)
        .ok_or_else(|| ApiError::NotFound("Item not found in queue".to_string()))?;
    let last = order.len() - 1;

    let message = match to {
        Move::Up | Move::First if pos == 0 => {
            return Err(ApiError::BadRequest("Item is already first".to_string()))
        }
        Move::Down | Move::Last if pos == last => {
            return Err(ApiError::BadRequest("Item is already last".to_string()))
        }
        Move::Up => {
            order.swap(pos, pos - 1);
            "Item moved up in queue"
        }
        Move::Down => {
            order.swap(pos, pos + 1);
            "Item moved down in queue"
        }
        Move::First => {
            let id = order.remove(pos);
            order.insert(0, id);
            "Item moved to first position in queue"
        }
        Move::Last => {
            let id = order.remove(pos);
            order.push(id);
            "Item moved to last position in queue"
        }
    };

    queue_repo
        .set_order(&order)
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to update priority: {}", e)))?;

    Ok(Json(SuccessResponse {
        message: message.to_string(),
    }))
}

/// Skip queue item
///
/// POST /local/queue/:file_id/skip
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

    let existing = queue_repo
        .get_by_file_id(file_id)
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to get queue item: {}", e)))?;
    if existing.is_none() {
        return Err(ApiError::NotFound("Item not found in queue".to_string()));
    }

    // Remove the item from queue
    queue_repo
        .remove(file_id)
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to skip item: {}", e)))?;

    Ok(Json(SuccessResponse {
        message: format!("Item {} skipped and removed from queue", file_id),
    }))
}

/// Pause one file: a running download stops within seconds and keeps its partial
///
/// POST /local/queue/:file_id/pause
async fn pause_item(
    State(state): State<AppState>,
    Path(file_id_str): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let file_id: i64 = file_id_str
        .parse()
        .map_err(|_| ApiError::BadRequest("Invalid file ID".to_string()))?;
    let item = queued_item(&state, file_id).await?;
    if item.status != "pending" && item.status != "downloading" {
        return Err(ApiError::Conflict(format!(
            "Item is {}, not pending or downloading",
            item.status
        )));
    }
    let changed = state
        .download_manager
        .pause_files(&[file_id])
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to pause item: {}", e)))?;
    if changed.is_empty() {
        return Err(ApiError::Conflict("Item is no longer running".to_string()));
    }
    info!(file_id, name = %item.filename, "Paused file");
    putio_sync_telemetry::pipeline::action(PipelineAction::Pause);
    Ok(Json(SuccessResponse {
        message: format!("Item {} paused", file_id),
    }))
}

/// Put a paused file back in the queue, in its old place
///
/// POST /local/queue/:file_id/resume
async fn resume_item(
    State(state): State<AppState>,
    Path(file_id_str): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let file_id: i64 = file_id_str
        .parse()
        .map_err(|_| ApiError::BadRequest("Invalid file ID".to_string()))?;
    let item = queued_item(&state, file_id).await?;
    let changed = state
        .download_manager
        .resume_files(&[file_id])
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to resume item: {}", e)))?;
    if changed.is_empty() {
        return Err(ApiError::Conflict(format!(
            "Item is {}, not paused",
            item.status
        )));
    }
    info!(file_id, name = %item.filename, "Resumed file");
    putio_sync_telemetry::pipeline::action(PipelineAction::Resume);
    Ok(Json(SuccessResponse {
        message: format!("Item {} resumed", file_id),
    }))
}

/// Put a failed file back to `pending`, in its old place (R10)
///
/// POST /local/queue/:file_id/retry
async fn retry_item(
    State(state): State<AppState>,
    Path(file_id_str): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let file_id: i64 = file_id_str
        .parse()
        .map_err(|_| ApiError::BadRequest("Invalid file ID".to_string()))?;
    let item = queued_item(&state, file_id).await?;
    let changed = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?
        .queue()
        .retry_files(&[file_id])
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to retry item: {}", e)))?;
    if changed.is_empty() {
        return Err(ApiError::Conflict(format!(
            "Item is {}, not failed",
            item.status
        )));
    }
    info!(file_id, name = %item.filename, "Retrying file");
    Ok(Json(SuccessResponse {
        message: format!("Item {} queued again", file_id),
    }))
}

/// The queue row of `file_id`, or 404
async fn queued_item(state: &AppState, file_id: i64) -> Result<QueueItem, ApiError> {
    let db = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?;
    db.queue()
        .get_by_file_id(file_id)
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to get queue item: {}", e)))?
        .ok_or_else(|| ApiError::NotFound("Item not found in queue".to_string()))
}

/// Get queue statistics
///
/// GET /local/queue/stats
async fn get_stats(State(state): State<AppState>) -> Result<Json<QueueStatsResponse>, ApiError> {
    let db = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?;

    let queue_repo = db.queue();
    let stats = queue_repo
        .get_stats()
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to get queue stats: {}", e)))?;

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
async fn clear_completed(State(state): State<AppState>) -> Result<Json<SuccessResponse>, ApiError> {
    let db = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?;

    let queue_repo = db.queue();
    let count = queue_repo
        .clear_completed()
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to clear completed items: {}", e)))?;

    Ok(Json(SuccessResponse {
        message: format!("Cleared {} completed items from queue", count),
    }))
}

/// Response of the Pause local downloads switch
#[derive(Debug, Serialize)]
pub struct SwitchResponse {
    pub nas_paused: bool,
}

/// Turn the Pause local downloads switch on: running downloads stop and keep their partial
/// files, nothing new starts, and no row changes (feature 039, FR-020)
///
/// POST /local/queue/pause
async fn pause_all(State(state): State<AppState>) -> Result<Json<SwitchResponse>, ApiError> {
    set_switch(&state, true).await
}

/// Turn the switch off: queued files start again in their old order
///
/// POST /local/queue/resume
async fn resume_all(State(state): State<AppState>) -> Result<Json<SwitchResponse>, ApiError> {
    set_switch(&state, false).await
}

async fn set_switch(state: &AppState, on: bool) -> Result<Json<SwitchResponse>, ApiError> {
    state
        .download_manager
        .set_nas_paused(on)
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to change the switch: {}", e)))?;
    info!(nas_paused = on, "Pause local downloads switch changed");
    Ok(Json(SwitchResponse { nas_paused: on }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_creation() {
        let _router = router();
    }
}
