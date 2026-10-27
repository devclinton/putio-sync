//! Put.io doctor routes (feature 010)
//!
//! - GET /doctor - The latest report (404 before the first run)
//! - POST /doctor/run - Run every check now
//! - POST /doctor/fixes/{fix}/preview - What a fix would act on
//! - POST /doctor/fixes/{fix}/apply - Apply a fix to the listed items

use crate::error::ApiError;
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    routing::{get, post},
    Json, Router,
};
use putio_sync_doctor::{FixError, FixId, FixOutcome, FixPreview, FixRequest, Report};

pub use crate::AppState;

/// Create the doctor router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(latest))
        .route("/run", post(run))
        .route("/fixes/:fix/preview", post(preview))
        .route("/fixes/:fix/apply", post(apply))
}

fn fix_id(id: &str) -> Result<FixId, ApiError> {
    id.parse()
        .map_err(|_| ApiError::NotFound(format!("unknown fix {id:?}")))
}

/// GET /doctor
async fn latest(State(state): State<AppState>) -> Result<Json<Report>, ApiError> {
    state
        .doctor
        .latest()
        .map(Json)
        .ok_or_else(|| ApiError::NotFound("no doctor run yet".into()))
}

/// POST /doctor/run. A request made during a run waits for it and returns its report.
async fn run(State(state): State<AppState>) -> Json<Report> {
    Json(state.doctor.run().await)
}

/// POST /doctor/fixes/{fix}/preview
async fn preview(
    State(state): State<AppState>,
    Path(fix): Path<String>,
) -> Result<Json<FixPreview>, ApiError> {
    Ok(Json(state.doctor.preview(fix_id(&fix)?).await))
}

/// POST /doctor/fixes/{fix}/apply `{"items": [...], "confirm": true}`
async fn apply(
    State(state): State<AppState>,
    Path(fix): Path<String>,
    request: Result<Json<FixRequest>, JsonRejection>,
) -> Result<Json<FixOutcome>, ApiError> {
    let fix = fix_id(&fix)?;
    let Json(request) = request.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    state
        .doctor
        .apply(fix, &request)
        .await
        .map(Json)
        .map_err(|e| match e {
            FixError::UnknownFix(_) => ApiError::NotFound(e.to_string()),
            FixError::Invalid(msg) => ApiError::BadRequest(msg),
            FixError::Conflict(msg) => ApiError::Conflict(msg),
        })
}
