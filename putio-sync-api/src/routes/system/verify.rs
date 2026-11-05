//! Download verification routes (feature 034)
//!
//! - GET /verify - Whether new downloads are checked, the run going, the last run and
//!   files that did not verify
//! - POST /verify - Start a "verify everything" run `{"paths"?, "report_only"}`
//! - POST /verify/pause, /verify/resume, /verify/stop - Steer the run going

use crate::error::ApiError;
use axum::{
    extract::{rejection::JsonRejection, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use putio_sync_db::{VerifyProblem, VerifyRun};
use putio_sync_download::{VerifyControlError, VerifyRequest, VerifyStartError, VerifyStatus};
use serde::{Deserialize, Serialize};

pub use crate::AppState;

/// Problems listed at most
const PROBLEMS: i64 = 200;

/// Create the verify router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_verify).post(start))
        .route("/pause", post(pause))
        .route("/resume", post(resume))
        .route("/stop", post(stop))
}

#[derive(Serialize)]
struct VerifyResponse {
    enabled: bool,
    status: VerifyStatus,
    last_run: Option<VerifyRun>,
    problems: Vec<VerifyProblem>,
}

#[derive(Deserialize, Default)]
struct StartRequest {
    #[serde(default)]
    paths: Option<Vec<String>>,
    #[serde(default)]
    report_only: bool,
}

fn control_error(e: VerifyControlError) -> ApiError {
    ApiError::Conflict(e.to_string())
}

/// GET /verify
async fn get_verify(State(state): State<AppState>) -> Result<Json<VerifyResponse>, ApiError> {
    let enabled = state.config.read().await.download.verify;
    let status = state.download_manager.verify().status();
    let (last_run, problems) = match &state.database {
        Some(db) => {
            let last_run = db
                .verify()
                .latest_run()
                .await
                .map_err(|e| ApiError::Internal(e.to_string()))?;
            let last_id = last_run.as_ref().map(|run| run.id);
            let problems = db
                .verify()
                .open_problems(PROBLEMS)
                .await
                .map_err(|e| ApiError::Internal(e.to_string()))?
                .into_iter()
                // A run's problems only while it is the latest; downloads' always
                .filter(|p| p.source == "download" || p.run_id == last_id)
                .collect();
            (last_run, problems)
        }
        None => (None, Vec::new()),
    };
    Ok(Json(VerifyResponse {
        enabled,
        status,
        last_run,
        problems,
    }))
}

/// POST /verify; 202 with the run's status
async fn start(
    State(state): State<AppState>,
    request: Result<Json<StartRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<VerifyStatus>), ApiError> {
    // An empty body starts a run over everything
    let request = match request {
        Ok(Json(request)) => request,
        Err(JsonRejection::MissingJsonContentType(_)) => StartRequest::default(),
        Err(e) => return Err(ApiError::BadRequest(e.body_text())),
    };
    let status = state
        .download_manager
        .verify()
        .start(VerifyRequest {
            paths: request.paths,
            report_only: request.report_only,
        })
        .await
        .map_err(|e| match e {
            VerifyStartError::Running => ApiError::Conflict(e.to_string()),
            VerifyStartError::NoFiles => ApiError::BadRequest(e.to_string()),
            VerifyStartError::NoDatabase => ApiError::ServiceUnavailable(e.to_string()),
            VerifyStartError::Database(msg) => ApiError::Internal(msg),
        })?;
    Ok((StatusCode::ACCEPTED, Json(status)))
}

/// POST /verify/pause
async fn pause(State(state): State<AppState>) -> Result<Json<VerifyStatus>, ApiError> {
    state
        .download_manager
        .verify()
        .pause()
        .map(Json)
        .map_err(control_error)
}

/// POST /verify/resume
async fn resume(State(state): State<AppState>) -> Result<Json<VerifyStatus>, ApiError> {
    state
        .download_manager
        .verify()
        .resume()
        .map(Json)
        .map_err(control_error)
}

/// POST /verify/stop
async fn stop(State(state): State<AppState>) -> Result<Json<VerifyStatus>, ApiError> {
    state
        .download_manager
        .verify()
        .stop()
        .map(Json)
        .map_err(control_error)
}
