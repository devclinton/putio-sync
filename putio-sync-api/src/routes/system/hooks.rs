//! Event hook routes (feature 027)
//!
//! - GET /hooks - Each hook with its target (redacted), queue and last 20 runs
//! - POST /hooks/test - Send one `hook.test` event to a hook now
//! - GET /events/schema - The event JSON Schema

use crate::error::ApiError;
use axum::{
    extract::{rejection::JsonRejection, State},
    http::header,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use putio_sync_hooks::{HookRun, HookStatus, TestError};
use serde::{Deserialize, Serialize};

pub use crate::AppState;

/// Create the hooks router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/test", post(test))
}

/// Create the events router
pub fn events_router() -> Router<AppState> {
    Router::new().route("/schema", get(schema))
}

#[derive(Serialize)]
struct HookList {
    hooks: Vec<HookStatus>,
}

#[derive(Deserialize)]
struct TestRequest {
    name: String,
}

/// GET /hooks
async fn list(State(state): State<AppState>) -> Json<HookList> {
    Json(HookList {
        hooks: state.hooks.status(),
    })
}

/// POST /hooks/test `{"name": "..."}`. 200 with the run even when the hook failed.
async fn test(
    State(state): State<AppState>,
    request: Result<Json<TestRequest>, JsonRejection>,
) -> Result<Json<HookRun>, ApiError> {
    let Json(request) = request.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    state
        .hooks
        .test(&request.name)
        .await
        .map(Json)
        .map_err(|e| match e {
            TestError::Unknown => ApiError::NotFound(format!("no hook named {:?}", request.name)),
            TestError::Busy => ApiError::Conflict(format!(
                "a test of hook {:?} is still running",
                request.name
            )),
        })
}

/// GET /events/schema
async fn schema() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "application/schema+json")],
        putio_sync_core::events::SCHEMA,
    )
}
