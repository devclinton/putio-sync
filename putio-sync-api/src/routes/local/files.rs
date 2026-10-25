//! Local file management routes
//!
//! Provides REST API endpoints for local file operations:
//! - GET  /local/files          - List files in download folder
//! - GET  /local/files/*path    - Get info on specific file
//! - DELETE /local/files/*path  - Delete local file

use crate::error::ApiError;
use axum::{
    extract::{Path, State},
    routing::{delete, get},
    Json, Router,
};
use serde::{Deserialize, Serialize};

// Re-export AppState from parent
pub use crate::AppState;

/// Local file information
#[derive(Debug, Serialize, Deserialize)]
pub struct LocalFileInfo {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub modified: Option<String>,
}

/// List response
#[derive(Debug, Serialize)]
pub struct ListFilesResponse {
    pub files: Vec<LocalFileInfo>,
    pub total: usize,
}

/// Success response
#[derive(Debug, Serialize)]
pub struct SuccessResponse {
    pub success: bool,
    pub message: String,
}

/// Create the local files router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_files))
        .route("/*path", get(get_file_info))
        .route("/*path", delete(delete_file))
}

/// List files in the download directory
///
/// GET /local/files
async fn list_files(State(state): State<AppState>) -> Result<Json<ListFilesResponse>, ApiError> {
    let config = state.config.read().await;
    let download_dir = &config.download.target_directory;

    let mut files = Vec::new();

    if let Ok(entries) = std::fs::read_dir(download_dir) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                let modified = metadata.modified().ok().map(|time| format!("{:?}", time));

                files.push(LocalFileInfo {
                    path: path.to_string_lossy().to_string(),
                    name,
                    size: metadata.len(),
                    is_dir: metadata.is_dir(),
                    modified,
                });
            }
        }
    }

    let total = files.len();

    Ok(Json(ListFilesResponse { files, total }))
}

/// Get information about a specific file
///
/// GET /local/files/*path
async fn get_file_info(
    State(state): State<AppState>,
    Path(file_path): Path<String>,
) -> Result<Json<LocalFileInfo>, ApiError> {
    let config = state.config.read().await;
    let download_dir = &config.download.target_directory;
    let full_path = download_dir.join(&file_path);

    // Security check: ensure path is within download directory
    if !full_path.starts_with(download_dir) {
        return Err(ApiError::BadRequest("Invalid path".to_string()));
    }

    let metadata = std::fs::metadata(&full_path)
        .map_err(|e| ApiError::NotFound(format!("File not found: {}", e)))?;

    let name = full_path
        .file_name()
        .ok_or_else(|| ApiError::BadRequest("Invalid file name".to_string()))?
        .to_string_lossy()
        .to_string();

    let modified = metadata.modified().ok().map(|time| format!("{:?}", time));

    Ok(Json(LocalFileInfo {
        path: full_path.to_string_lossy().to_string(),
        name,
        size: metadata.len(),
        is_dir: metadata.is_dir(),
        modified,
    }))
}

/// Delete a local file
///
/// DELETE /local/files/*path
async fn delete_file(
    State(state): State<AppState>,
    Path(file_path): Path<String>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let config = state.config.read().await;
    let download_dir = &config.download.target_directory;
    let full_path = download_dir.join(&file_path);

    // Security check: ensure path is within download directory
    if !full_path.starts_with(download_dir) {
        return Err(ApiError::BadRequest("Invalid path".to_string()));
    }

    if full_path.is_dir() {
        std::fs::remove_dir_all(&full_path)
    } else {
        std::fs::remove_file(&full_path)
    }
    .map_err(|e| ApiError::InternalError(format!("Failed to delete: {}", e)))?;

    Ok(Json(SuccessResponse {
        success: true,
        message: format!("Deleted: {}", file_path),
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
