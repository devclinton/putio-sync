//! Download history routes
//!
//! Provides REST API endpoints for download history:
//! - GET  /history - Download history with filtering and pagination
//! - GET  /history/stats - History statistics

use crate::error::ApiError;
use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use putio_sync_db::models::HistoryAction;
use serde::{Deserialize, Serialize};
use tracing::error;

// Re-export AppState from parent
pub use crate::AppState;

/// History query parameters
#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub action: Option<String>, // "downloaded" or "deleted"
}

/// History entry
#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub file_id: i64,
    pub filename: String,
    pub action: String,
    pub file_size: u64,
    pub duration_seconds: Option<u64>,
    pub speed_bps: Option<f64>,
    pub timestamp: String,
}

/// History response
#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub entries: Vec<HistoryEntry>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

/// History statistics response
#[derive(Debug, Serialize)]
pub struct HistoryStatsResponse {
    pub total_downloads: i64,
    pub total_deletes: i64,
    pub total_bytes_downloaded: i64,
    pub average_speed_bps: f64,
}

/// Create the history router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_history))
        .route("/stats", get(get_history_stats))
}

/// Get download history
///
/// GET /history?limit=50&offset=0&action=downloaded
async fn get_history(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50).min(500) as i64; // Cap at 500
    let offset = query.offset.unwrap_or(0) as i64;

    let db = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?;

    let history_repo = db.history();

    // Parse action filter
    let action = query
        .action
        .as_ref()
        .and_then(|a| match a.to_lowercase().as_str() {
            "downloaded" => Some(HistoryAction::Downloaded),
            "deleted" => Some(HistoryAction::Deleted),
            _ => None,
        });

    // Query history
    let entries_db = history_repo
        .query(action, limit, offset)
        .await
        .map_err(|e| {
            error!("Failed to query history: {}", e);
            ApiError::InternalError(format!("Failed to query history: {}", e))
        })?;

    // Get total count
    let total = history_repo.count(action).await.map_err(|e| {
        error!("Failed to count history: {}", e);
        ApiError::InternalError(format!("Failed to count history: {}", e))
    })? as usize;

    // Convert to response format
    let entries: Vec<HistoryEntry> = entries_db
        .into_iter()
        .map(|entry| HistoryEntry {
            id: entry.id,
            file_id: entry.file_id,
            filename: entry.filename,
            action: entry.action,
            file_size: entry.file_size as u64,
            duration_seconds: entry.duration_seconds.map(|d| d as u64),
            speed_bps: entry.average_speed.map(|s| s as f64),
            timestamp: entry.timestamp,
        })
        .collect();

    Ok(Json(HistoryResponse {
        total,
        limit: limit as usize,
        offset: offset as usize,
        entries,
    }))
}

/// Get history statistics
///
/// GET /history/stats
async fn get_history_stats(
    State(state): State<AppState>,
) -> Result<Json<HistoryStatsResponse>, ApiError> {
    let db = state
        .database
        .as_ref()
        .ok_or_else(|| ApiError::InternalError("Database not available".to_string()))?;

    let history_repo = db.history();

    let total_downloads = history_repo
        .count(Some(HistoryAction::Downloaded))
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to count downloads: {}", e)))?;

    let total_deletes = history_repo
        .count(Some(HistoryAction::Deleted))
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to count deletes: {}", e)))?;

    // Get all downloaded entries to calculate totals
    let downloads = history_repo
        .query(Some(HistoryAction::Downloaded), 10000, 0)
        .await
        .map_err(|e| ApiError::InternalError(format!("Failed to query downloads: {}", e)))?;

    let total_bytes_downloaded: i64 = downloads.iter().map(|d| d.file_size).sum();

    let total_speed: i64 = downloads.iter().filter_map(|d| d.average_speed).sum();

    let speed_count = downloads
        .iter()
        .filter(|d| d.average_speed.is_some())
        .count();

    let average_speed_bps = if speed_count > 0 {
        total_speed as f64 / speed_count as f64
    } else {
        0.0
    };

    Ok(Json(HistoryStatsResponse {
        total_downloads,
        total_deletes,
        total_bytes_downloaded,
        average_speed_bps,
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
