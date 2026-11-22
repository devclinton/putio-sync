//! Crash report routes (feature 058)
//!
//! - GET /crash-reports - The reports kept next to the config file, newest first, as summaries
//! - GET /crash-reports/{id} - One report with its full text, the prefilled GitHub new-issue
//!   link and the link that searches existing issues
//! - POST /crash-reports/{id}/dismiss - Hide the report from the notice and the doctor
//!
//! Nothing is sent from here: the person opens the issue link from their own browser.

use crate::error::ApiError;
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use putio_sync_core::crash::{self, CrashReport, Issue};
use serde::Serialize;
use std::path::PathBuf;

pub use crate::AppState;

/// Create the crash report router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/:id", get(show))
        .route("/:id/dismiss", post(dismiss))
}

fn dir(state: &AppState) -> Result<PathBuf, ApiError> {
    crash::dir_for(state.config_path.as_deref().map(|p| p.as_path()))
        .ok_or_else(|| ApiError::NotFound("no config folder".into()))
}

#[derive(Serialize)]
struct Summary {
    id: String,
    kind: crash::Kind,
    time: String,
    version: String,
    signature: String,
    message: String,
    dismissed: bool,
}

#[derive(Serialize)]
struct List {
    /// Reports not dismissed, within the last 7 days: what the notice shows
    pending: usize,
    reports: Vec<Summary>,
}

#[derive(Serialize)]
struct Detail {
    #[serde(flatten)]
    report: CrashReport,
    signature: String,
    /// The whole report as text
    text: String,
    #[serde(flatten)]
    issue: Issue,
}

fn first_line(message: &str) -> String {
    message
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(200)
        .collect()
}

async fn list(State(state): State<AppState>) -> Result<Json<List>, ApiError> {
    let dir = dir(&state)?;
    let (pending, reports) = tokio::task::spawn_blocking(move || {
        (
            crash::recent_undismissed(&dir).len(),
            crash::list(&dir)
                .into_iter()
                .map(|r| Summary {
                    signature: r.signature(),
                    message: first_line(&r.message),
                    id: r.id,
                    kind: r.kind,
                    time: r.time,
                    version: r.version,
                    dismissed: r.dismissed,
                })
                .collect(),
        )
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(List { pending, reports }))
}

async fn show(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Detail>, ApiError> {
    let dir = dir(&state)?;
    let report = tokio::task::spawn_blocking(move || crash::get(&dir, &id))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or_else(|| ApiError::NotFound("no such crash report".into()))?;
    Ok(Json(Detail {
        signature: report.signature(),
        text: report.to_text(),
        issue: report.issue(),
        report,
    }))
}

async fn dismiss(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let dir = dir(&state)?;
    let done = tokio::task::spawn_blocking(move || crash::dismiss(&dir, &id))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    if done {
        Ok(Json(serde_json::json!({ "dismissed": true })))
    } else {
        Err(ApiError::NotFound("no such crash report".into()))
    }
}
