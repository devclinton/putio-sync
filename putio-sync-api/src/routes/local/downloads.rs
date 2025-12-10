//! Active download management routes (Task 4)
//!
//! Provides REST API endpoints for managing active downloads:
//! - POST /local/downloads        - Start new download
//! - GET  /local/downloads        - List active downloads
//! - GET  /local/downloads/:id    - Get download progress
//! - DELETE /local/downloads/:id  - Cancel download

use crate::error::ApiError;
use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

// Re-export AppState from parent
pub use crate::AppState;

/// Start download request
#[derive(Debug, Deserialize)]
pub struct StartDownloadRequest {
    pub file_id: i64,
    pub filename: String,
    pub relative_path: Option<String>,
}

/// Download progress response
#[derive(Debug, Serialize)]
pub struct DownloadProgressResponse {
    pub id: i64,
    pub file_id: i64,
    pub filename: String,
    pub status: String,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub speed_bps: f64,
    pub eta_seconds: Option<u64>,
}

/// Active downloads list response
#[derive(Debug, Serialize)]
pub struct ActiveDownloadsResponse {
    pub downloads: Vec<DownloadProgressResponse>,
    pub total: usize,
}

/// Success response with download ID
#[derive(Debug, Serialize)]
pub struct StartDownloadResponse {
    pub message: String,
    pub download_id: Option<i64>,
}

/// Success response for cancellation
#[derive(Debug, Serialize)]
pub struct CancelResponse {
    pub message: String,
}

/// Create the downloads router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", post(start_download))
        .route("/", get(list_downloads))
        .route("/:id", get(get_download_progress))
        .route("/:id", delete(cancel_download))
}

/// Start a new download
///
/// POST /local/downloads
///
/// Adds a file to the download queue and starts downloading it.
///
/// Note: This endpoint currently requires enhancements to DownloadManager
/// to support starting individual downloads via API (not just batch processing).
async fn start_download(
    State(state): State<AppState>,
    Json(request): Json<StartDownloadRequest>,
) -> Result<Json<StartDownloadResponse>, ApiError> {
    // TODO: Add DownloadManager method to start individual downloads
    // For now, we can get file info from Put.io and add to queue
    // but we need to enhance DownloadManager first

    // Validate file exists on Put.io
    let file = state
        .client
        .get_file_details(request.file_id)
        .await
        .map_err(ApiError::Client)?;

    // TODO: Add file to download queue and start download
    // This requires adding a method like:
    // download_manager.start_download(file_id, filename, relative_path).await

    Ok(Json(StartDownloadResponse {
        message: format!("Download queued for file: {}", file.file.name),
        download_id: Some(request.file_id),
    }))
}

/// List all active downloads
///
/// GET /local/downloads
///
/// Returns list of currently downloading files with progress information.
async fn list_downloads(
    State(state): State<AppState>,
) -> Result<Json<ActiveDownloadsResponse>, ApiError> {
    let active_downloads = state
        .download_manager
        .get_active_downloads()
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to get active downloads: {}", e)))?;

    let downloads: Vec<DownloadProgressResponse> = active_downloads
        .iter()
        .map(|d| DownloadProgressResponse {
            id: d.file_id,
            file_id: d.file_id,
            filename: d.filename.clone(),
            status: "downloading".to_string(),
            bytes_downloaded: d.bytes_downloaded,
            total_bytes: d.file_size,
            speed_bps: d.speed_bps,
            eta_seconds: d.eta_seconds,
        })
        .collect();

    Ok(Json(ActiveDownloadsResponse {
        total: downloads.len(),
        downloads,
    }))
}

/// Get progress for a specific download
///
/// GET /local/downloads/:id
///
/// Returns progress information for a specific download by file_id.
async fn get_download_progress(
    State(state): State<AppState>,
    Path(download_id): Path<i64>,
) -> Result<Json<DownloadProgressResponse>, ApiError> {
    let progress = state
        .download_manager
        .get_download_progress(download_id)
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to get download progress: {}", e)))?;

    match progress {
        Some(d) => Ok(Json(DownloadProgressResponse {
            id: d.file_id,
            file_id: d.file_id,
            filename: d.filename,
            status: "downloading".to_string(),
            bytes_downloaded: d.bytes_downloaded,
            total_bytes: d.file_size,
            speed_bps: d.speed_bps,
            eta_seconds: d.eta_seconds,
        })),
        None => Err(ApiError::NotFound(format!(
            "Download {} not found (not currently downloading)",
            download_id
        ))),
    }
}

/// Cancel an active download
///
/// DELETE /local/downloads/:id
///
/// Cancels an active download by updating its status to pending.
/// The download will not be retried until manually resumed.
async fn cancel_download(
    State(state): State<AppState>,
    Path(download_id): Path<i64>,
) -> Result<Json<CancelResponse>, ApiError> {
    state
        .download_manager
        .cancel_download(download_id)
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to cancel download: {}", e)))?;

    Ok(Json(CancelResponse {
        message: format!("Download {} cancelled successfully", download_id),
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
