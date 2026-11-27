//! Put.io storage routes (feature 033)
//!
//! - GET /putio/storage - Storage use and the biggest top-level items
//! - POST /putio/storage/plan - Preview what a free-up would delete (deletes nothing)
//! - POST /putio/storage/free - Delete the chosen files of a preview
//! - GET /putio/storage/runs - Latest free-up runs with the files they deleted
//! - POST /putio/storage/budget/preview - What the storage budget would delete now
//!   (feature 064; deletes nothing)

use crate::error::ApiError;
use axum::{
    extract::{rejection::JsonRejection, State},
    routing::{get, post},
    Json, Router,
};
use putio_sync_db::StorageRun;
use putio_sync_storage::{FreeResult, Plan, StorageError, Summary, Trigger, RECENT_RUNS};
use serde::Deserialize;

pub use crate::AppState;

/// Create the Put.io storage router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(summary))
        .route("/plan", post(plan))
        .route("/free", post(free))
        .route("/runs", get(runs))
        .route("/budget/preview", post(budget_preview))
}

fn api_error(e: StorageError) -> ApiError {
    match e {
        StorageError::AlreadyRunning | StorageError::PlanExpired => {
            ApiError::Conflict(e.to_string())
        }
        StorageError::NoDatabase => ApiError::ServiceUnavailable(e.to_string()),
        StorageError::NothingChosen | StorageError::BadBudget { .. } => {
            ApiError::BadRequest(e.to_string())
        }
        StorageError::Client(e) => ApiError::Client(e),
        StorageError::Db(e) => ApiError::Internal(e.to_string()),
    }
}

/// GET /putio/storage
async fn summary(State(state): State<AppState>) -> Result<Json<Summary>, ApiError> {
    state.storage.summary().await.map(Json).map_err(api_error)
}

/// POST /putio/storage/plan. 409 while a free-up is going; 503 without a database.
async fn plan(State(state): State<AppState>) -> Result<Json<Plan>, ApiError> {
    state.storage.plan().await.map(Json).map_err(api_error)
}

/// Body of POST /putio/storage/free
#[derive(Debug, Deserialize)]
pub struct FreeRequest {
    /// The preview's id
    pub plan_id: String,
    /// Files of that preview to delete
    pub file_ids: Vec<i64>,
}

/// POST /putio/storage/free `{"plan_id": "...", "file_ids": [1, 2]}`
///
/// Runs to the end before answering. 409 when the preview is unknown or expired, or a
/// run is going.
async fn free(
    State(state): State<AppState>,
    request: Result<Json<FreeRequest>, JsonRejection>,
) -> Result<Json<FreeResult>, ApiError> {
    let Json(request) = request.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    state
        .storage
        .free(Trigger::Manual, &request.plan_id, &request.file_ids)
        .await
        .map(Json)
        .map_err(api_error)
}

/// GET /putio/storage/runs: the latest 20, newest first; `[]` without a database
async fn runs(State(state): State<AppState>) -> Result<Json<Vec<StorageRun>>, ApiError> {
    state
        .storage
        .runs(RECENT_RUNS)
        .await
        .map(Json)
        .map_err(api_error)
}

/// Body of POST /putio/storage/budget/preview; may be missing or `{}`
#[derive(Debug, Default, Deserialize)]
pub struct BudgetPreviewRequest {
    /// The budget to try, 10–99; the saved `storage.budget_used_percent` when missing
    #[serde(default)]
    pub used_percent: Option<u8>,
}

/// POST /putio/storage/budget/preview `{"used_percent": 80}`
///
/// The files the storage budget would delete now to bring the account under that share:
/// the oldest that are downloaded, under the rules of a free-up. It deletes nothing,
/// works with the budget off, and its `id` can't be passed to `/free`. 400 for a percent
/// outside 10–99; 409 while a free-up is going; 503 without a database.
async fn budget_preview(
    State(state): State<AppState>,
    request: Option<Json<BudgetPreviewRequest>>,
) -> Result<Json<Plan>, ApiError> {
    let percent = request.and_then(|Json(r)| r.used_percent);
    state
        .storage
        .budget_preview(percent)
        .await
        .map(Json)
        .map_err(api_error)
}
