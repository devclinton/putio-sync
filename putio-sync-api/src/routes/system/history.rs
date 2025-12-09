//! Download history routes
//!
//! Provides REST API endpoints for download history:
//! - GET  /history - Download history with status

use crate::error::ApiError;
use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Application state (will include Database in Task 4)
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<putio_sync_core::config::Config>,
}

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

/// Create the history router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_history))
}

/// Get download history
///
/// GET /history?limit=50&offset=0&action=downloaded
async fn get_history(
    State(_state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, ApiError> {
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);

    // TODO: Implement with Database in Task 4
    Ok(Json(HistoryResponse {
        entries: vec![],
        total: 0,
        limit,
        offset,
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
