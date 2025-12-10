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
use putio_sync_db::models::HistoryAction;
use serde::Serialize;
use tracing::error;

// Re-export AppState from parent
pub use crate::AppState;

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
    State(state): State<AppState>,
) -> Result<Json<DownloadStats>, ApiError> {
    let db = state.database.as_ref().ok_or_else(|| {
        ApiError::InternalError("Database not available".to_string())
    })?;

    let history_repo = db.history();

    // Get total download count
    let total_downloads = history_repo
        .count(Some(HistoryAction::Downloaded))
        .await
        .map_err(|e| {
            error!("Failed to count downloads: {}", e);
            ApiError::InternalError(format!("Failed to count downloads: {}", e))
        })? as u64;

    // Get all downloaded entries to calculate totals
    let downloads = history_repo
        .query(Some(HistoryAction::Downloaded), 10000, 0)
        .await
        .map_err(|e| {
            error!("Failed to query downloads: {}", e);
            ApiError::InternalError(format!("Failed to query downloads: {}", e))
        })?;

    let total_bytes: i64 = downloads.iter().map(|d| d.file_size).sum();
    let total_time_seconds: i64 = downloads
        .iter()
        .filter_map(|d| d.duration_seconds)
        .sum();

    let total_speed: i64 = downloads
        .iter()
        .filter_map(|d| d.average_speed)
        .sum();

    let speed_count = downloads
        .iter()
        .filter(|d| d.average_speed.is_some())
        .count();

    let average_speed_bps = if speed_count > 0 {
        total_speed as f64 / speed_count as f64
    } else {
        0.0
    };

    Ok(Json(DownloadStats {
        total_downloads,
        total_bytes: total_bytes as u64,
        average_speed_bps,
        total_time_seconds: total_time_seconds as u64,
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
    let config = state.config.read().await;
    let download_dir = &config.download.target_directory;

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
