//! Local file management routes

use crate::handlers::local::{self, LocalFileResponse};
use crate::models::{ListResponse, SuccessResponse};
use crate::routes::files::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::{delete, get},
    Json, Router,
};
use serde::Deserialize;

/// Query parameters for listing local files
#[derive(Debug, Deserialize)]
pub struct ListLocalFilesQuery {
    /// Subdirectory path (relative to download directory)
    pub path: Option<String>,
}

/// Query parameters for deleting local files
#[derive(Debug, Deserialize)]
pub struct DeleteLocalFileQuery {
    /// File path (relative to download directory)
    pub path: String,
}

/// Create the local files router
///
/// # Returns
///
/// Router with all local file endpoints configured
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_local_files_handler))
        .route("/", delete(delete_local_file_handler))
}

/// Handler for listing local files
///
/// GET /api/local?path=/Movies
async fn list_local_files_handler(
    State(state): State<AppState>,
    Query(query): Query<ListLocalFilesQuery>,
) -> Result<Json<ListResponse<LocalFileResponse>>, (StatusCode, String)> {
    local::list_local_files(&state.config, query.path)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Handler for deleting a local file
///
/// DELETE /api/local?path=file.txt
async fn delete_local_file_handler(
    State(state): State<AppState>,
    Query(query): Query<DeleteLocalFileQuery>,
) -> Result<Json<SuccessResponse>, (StatusCode, String)> {
    local::delete_local_file(&state.config, query.path)
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

    #[test]
    fn test_list_local_files_query() {
        let json = r#"{"path": "/Movies"}"#;
        let query: ListLocalFilesQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.path, Some("/Movies".to_string()));
    }

    #[test]
    fn test_delete_local_file_query() {
        let json = r#"{"path": "file.txt"}"#;
        let query: DeleteLocalFileQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.path, "file.txt");
    }
}
