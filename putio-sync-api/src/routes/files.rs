//! File management routes

use crate::handlers::files;
use crate::models::files::{DeleteFilesRequest, FileResponse, ListFilesRequest};
use crate::models::{ListResponse, SuccessResponse};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get},
    Json, Router,
};
use putio_sync_core::config::Config;
use std::sync::Arc;

/// Application state
#[derive(Clone)]
pub struct AppState {
    /// Application configuration
    pub config: Arc<Config>,
}

/// Create the files router
///
/// # Returns
///
/// Router with all files endpoints configured
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_files_handler))
        .route("/:id", get(get_file_handler))
        .route("/:id", delete(delete_file_handler))
        .route("/bulk", delete(delete_files_bulk_handler))
}

/// Handler for listing files
///
/// GET /api/files?parent_id=0&recursive=false&path=/Movies
async fn list_files_handler(
    State(state): State<AppState>,
    Query(request): Query<ListFilesRequest>,
) -> Result<Json<ListResponse<FileResponse>>, (StatusCode, String)> {
    files::list_files(&state.config, request)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for getting a single file
///
/// GET /api/files/:id
async fn get_file_handler(
    State(state): State<AppState>,
    Path(file_id): Path<i64>,
) -> Result<Json<FileResponse>, (StatusCode, String)> {
    files::get_file(&state.config, file_id)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for deleting a single file
///
/// DELETE /api/files/:id
async fn delete_file_handler(
    State(state): State<AppState>,
    Path(file_id): Path<i64>,
) -> Result<Json<SuccessResponse>, (StatusCode, String)> {
    let request = DeleteFilesRequest {
        file_ids: vec![file_id],
    };

    files::delete_files(&state.config, request)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for bulk deleting files
///
/// DELETE /api/files/bulk
async fn delete_files_bulk_handler(
    State(state): State<AppState>,
    Json(request): Json<DeleteFilesRequest>,
) -> Result<Json<SuccessResponse>, (StatusCode, String)> {
    files::delete_files(&state.config, request)
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
