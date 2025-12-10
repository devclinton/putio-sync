//! Worker statistics and management routes
//!
//! Provides REST API endpoints for worker monitoring:
//! - GET /local/workers       - List all workers with stats
//! - GET /local/workers/:id   - Get specific worker stats

use crate::error::ApiError;
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::Serialize;

// Re-export AppState from parent
pub use crate::AppState;

/// Worker statistics response
#[derive(Debug, Serialize)]
pub struct WorkerStatsResponse {
    pub worker_id: usize,
    pub bytes_downloaded: u64,
    pub bandwidth_limit_bps: Option<u64>,
    pub avg_bandwidth_bps: f64,
    pub uptime_seconds: u64,
}

/// Workers list response
#[derive(Debug, Serialize)]
pub struct WorkersResponse {
    pub workers: Vec<WorkerStatsResponse>,
    pub total: usize,
    pub total_bandwidth_limit_bps: Option<u64>,
    pub total_avg_bandwidth_bps: f64,
}

/// Create the workers router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_workers))
        .route("/:id", get(get_worker))
}

/// List all workers with statistics
///
/// GET /local/workers
///
/// Returns list of all active workers with their bandwidth usage and limits.
async fn list_workers(
    State(state): State<AppState>,
) -> Result<Json<WorkersResponse>, ApiError> {
    let worker_stats = state.download_manager.get_worker_stats().await;

    let mut workers = Vec::new();
    let mut total_avg_bps = 0.0;
    let mut total_limit: Option<u64> = None;

    for stat in worker_stats {
        let bytes = *stat.bytes_downloaded.read().await;
        let avg_bps = stat.avg_bandwidth_bps().await;
        let uptime = stat.started_at.elapsed().as_secs();

        total_avg_bps += avg_bps;

        if let Some(limit) = stat.bandwidth_limit {
            total_limit = Some(total_limit.unwrap_or(0) + limit);
        }

        workers.push(WorkerStatsResponse {
            worker_id: stat.worker_id,
            bytes_downloaded: bytes,
            bandwidth_limit_bps: stat.bandwidth_limit,
            avg_bandwidth_bps: avg_bps,
            uptime_seconds: uptime,
        });
    }

    Ok(Json(WorkersResponse {
        total: workers.len(),
        workers,
        total_bandwidth_limit_bps: total_limit,
        total_avg_bandwidth_bps: total_avg_bps,
    }))
}

/// Get statistics for a specific worker
///
/// GET /local/workers/:id
///
/// Returns detailed statistics for one worker by ID.
async fn get_worker(
    State(state): State<AppState>,
    Path(worker_id): Path<usize>,
) -> Result<Json<WorkerStatsResponse>, ApiError> {
    let stat = state
        .download_manager
        .get_worker_stat(worker_id)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("Worker {} not found", worker_id)))?;

    let bytes = *stat.bytes_downloaded.read().await;
    let avg_bps = stat.avg_bandwidth_bps().await;
    let uptime = stat.started_at.elapsed().as_secs();

    Ok(Json(WorkerStatsResponse {
        worker_id: stat.worker_id,
        bytes_downloaded: bytes,
        bandwidth_limit_bps: stat.bandwidth_limit,
        avg_bandwidth_bps: avg_bps,
        uptime_seconds: uptime,
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
