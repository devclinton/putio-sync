//! Put.io file proxy routes
//!
//! Provides REST API endpoints for Put.io file operations:
//! - GET  /putio/files           - List files for folder (supports sorting)
//! - GET  /putio/files/:id       - Get file details
//! - DELETE /putio/files/:id     - Delete file/folder (recursive optional)
//! - POST /putio/files/delete    - Delete several files/folders with one Put.io call
//! - GET  /putio/files/:id/download - Get download URL

use crate::error::ApiError;
use crate::handlers::{files, unique_ids};
use crate::models::files::{DeleteFilesRequest, FileResponse, ListFilesRequest};
use crate::models::{ListResponse, SuccessResponse};
use axum::{
    extract::{rejection::JsonRejection, Path, Query, State},
    routing::{delete, get, post},
    Json, Router,
};

// Re-export AppState from parent
pub use crate::AppState;

/// Queue states that still need the file on Put.io
const QUEUED: [&str; 3] = ["pending", "downloading", "paused"];

/// Create the Put.io files router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_files))
        .route("/delete", post(delete_many))
        .route("/:id", get(get_file))
        .route("/:id", delete(delete_file))
        .route("/:id/download", get(get_download_url))
}

/// List files in a folder
///
/// GET /putio/files?parent_id=0&recursive=false&sort=name
async fn list_files(
    State(state): State<AppState>,
    Query(request): Query<ListFilesRequest>,
) -> Result<Json<ListResponse<FileResponse>>, ApiError> {
    files::list_files(&state.client, request).await.map(Json)
}

/// Get file details
///
/// GET /putio/files/:id
async fn get_file(
    State(state): State<AppState>,
    Path(file_id): Path<i64>,
) -> Result<Json<FileResponse>, ApiError> {
    files::get_file(&state.client, file_id).await.map(Json)
}

/// Delete a file or folder
///
/// DELETE /putio/files/:id?recursive=true
///
/// Refuses (409) a file the queue still needs: pending, downloading or paused (R11).
/// Without a database (dev mode) there is no queue to check.
async fn delete_file(
    State(state): State<AppState>,
    Path(file_id): Path<i64>,
) -> Result<Json<SuccessResponse>, ApiError> {
    if !queued_names(&state, &[file_id]).await?.is_empty() {
        return Err(
            ApiError::Conflict("File is queued or downloading".to_string())
                .with_details("Skip or cancel it first"),
        );
    }

    let request = DeleteFilesRequest {
        file_ids: vec![file_id],
    };

    files::delete_files(&state.client, request).await.map(Json)
}

/// Delete several files or folders with one Put.io call (feature 008)
///
/// POST /putio/files/delete `{"file_ids": [1, 2]}`
///
/// Refuses (409) the whole request, deleting nothing, when any file is one the queue still
/// needs, naming them in `details` (research R6).
async fn delete_many(
    State(state): State<AppState>,
    request: Result<Json<DeleteFilesRequest>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let Json(request) = request.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    let file_ids = unique_ids(request.file_ids)?;

    let kept = queued_names(&state, &file_ids).await?;
    if !kept.is_empty() {
        return Err(ApiError::Conflict(format!(
            "{} of {} items are queued or downloading",
            kept.len(),
            file_ids.len()
        ))
        .with_details(format!("Skip or cancel them first: {}", kept.join(", "))));
    }

    let count = file_ids.len();
    let response = files::delete_files(&state.client, DeleteFilesRequest { file_ids }).await?;
    tracing::info!(count, "Deleted Put.io files");
    Ok(Json(response))
}

/// Names of the files among `file_ids` the queue still needs: pending, downloading or
/// paused (R11). Without a database (dev mode) there is no queue to check.
async fn queued_names(state: &AppState, file_ids: &[i64]) -> Result<Vec<String>, ApiError> {
    let Some(db) = &state.database else {
        return Ok(Vec::new());
    };
    let mut names = Vec::new();
    for &file_id in file_ids {
        let row =
            db.queue().get_by_file_id(file_id).await.map_err(|e| {
                ApiError::InternalError(format!("Failed to check the queue: {}", e))
            })?;
        if let Some(row) = row.filter(|r| QUEUED.contains(&r.status.as_str())) {
            names.push(row.filename);
        }
    }
    Ok(names)
}

/// Get download URL for a file
///
/// GET /putio/files/:id/download
async fn get_download_url(
    State(state): State<AppState>,
    Path(file_id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Return the Put.io download URL (from the shared client, so it follows its base URL)
    let url = state.client.get_download_url(file_id);
    Ok(Json(serde_json::json!({
        "download_url": url,
        "file_id": file_id
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_creation() {
        let _router = router();
    }
}
