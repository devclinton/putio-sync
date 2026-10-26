//! Put.io file proxy routes
//!
//! Provides REST API endpoints for Put.io file operations:
//! - GET  /putio/files           - List files for folder (supports sorting)
//! - GET  /putio/files/:id       - Get file details
//! - DELETE /putio/files/:id     - Delete file/folder (recursive optional)
//! - GET  /putio/files/:id/download - Get download URL

use crate::error::ApiError;
use crate::handlers::files;
use crate::models::files::{DeleteFilesRequest, FileResponse, ListFilesRequest};
use crate::models::{ListResponse, SuccessResponse};
use axum::{
    extract::{Path, Query, State},
    routing::{delete, get},
    Json, Router,
};

// Re-export AppState from parent
pub use crate::AppState;

/// Create the Put.io files router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_files))
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
async fn delete_file(
    State(state): State<AppState>,
    Path(file_id): Path<i64>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let request = DeleteFilesRequest {
        file_ids: vec![file_id],
    };

    files::delete_files(&state.client, request).await.map(Json)
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
