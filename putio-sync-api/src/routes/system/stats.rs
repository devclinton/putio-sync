//! Statistics routes
//!
//! Provides REST API endpoints for system statistics:
//! - GET  /stats/downloads - Statistics about past downloads
//! - GET  /stats/transfers - Statistics about current transfers
//! - GET  /stats/disk     - Disk space information

use crate::error::ApiError;
use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::sync::Arc;

/// Application state (will include Database in Task 4)
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<putio_sync_core::config::Config>,
}

/// Download statistics
#[derive(Debug, Serialize)]
pub struct DownloadStats {
    pub total_downloads: u64,
    pub total_bytes: u64,
    pub average_speed_bps: f64,
    pub total_time_seconds: u64,
}

/// Transfer statistics
#[derive(Debug, Serialize)]
pub struct TransferStats {
    pub active_transfers: u64,
    pub queued_transfers: u64,
    pub completed_today: u64,
}

/// Disk space statistics
#[derive(Debug, Serialize)]
pub struct DiskStats {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub usage_percent: f64,
}

/// Create the stats router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/downloads", get(download_stats))
        .route("/transfers", get(transfer_stats))
        .route("/disk", get(disk_stats))
}

/// Get download statistics
///
/// GET /stats/downloads
async fn download_stats(
    State(_state): State<AppState>,
) -> Result<Json<DownloadStats>, ApiError> {
    // TODO: Implement with Database in Task 4
    Ok(Json(DownloadStats {
        total_downloads: 0,
        total_bytes: 0,
        average_speed_bps: 0.0,
        total_time_seconds: 0,
    }))
}

/// Get transfer statistics
///
/// GET /stats/transfers
async fn transfer_stats(
    State(_state): State<AppState>,
) -> Result<Json<TransferStats>, ApiError> {
    // TODO: Implement with Database in Task 4
    Ok(Json(TransferStats {
        active_transfers: 0,
        queued_transfers: 0,
        completed_today: 0,
    }))
}

/// Get disk space statistics
///
/// GET /stats/disk
async fn disk_stats(
    State(state): State<AppState>,
) -> Result<Json<DiskStats>, ApiError> {
    let download_dir = &state.config.download.target_directory;

    // Try to get disk space info (platform-specific)
    #[cfg(unix)]
    {
        
        if let Ok(metadata) = std::fs::metadata(download_dir) {
            // This is a simplified version - real implementation would use statvfs
            Ok(Json(DiskStats {
                total_bytes: metadata.len(),
                used_bytes: 0,
                available_bytes: 0,
                usage_percent: 0.0,
            }))
        } else {
            Err(ApiError::InternalError("Failed to get disk stats".to_string()))
        }
    }

    #[cfg(not(unix))]
    {
        // Platform-specific disk space queries would go here
        Ok(Json(DiskStats {
            total_bytes: 0,
            used_bytes: 0,
            available_bytes: 0,
            usage_percent: 0.0,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_creation() {
        let _router = router();
    }
}
