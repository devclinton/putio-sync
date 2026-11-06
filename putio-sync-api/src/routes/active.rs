//! Active transfers pipeline route (feature 039, contracts/api.md)
//!
//! - GET  /active        - every transfer on its way to the NAS, by stage and state, with counts
//! - POST /active/pause  - pause items by key (`{"keys": [...]}`): a stop on the NAS, a hold on Put.io
//! - POST /active/resume - resume items by key
//!
//! Put.io figures come from the shared transfers snapshot, at most [`MAX_AGE`] old; NAS
//! figures are read on each request.

use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::{extract::State, Json};
use chrono::{NaiveDateTime, SecondsFormat};
use putio_sync_db::pipeline::FinishedRow;
use putio_sync_download::pipeline::{ItemResult, Pipeline};
use putio_sync_download::DownloadError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How old the Put.io transfer list may be for a page (R3)
pub const MAX_AGE: Duration = Duration::from_secs(10);

/// Response of `GET /active`
#[derive(Debug, Serialize)]
pub struct ActiveResponse {
    #[serde(flatten)]
    pub pipeline: Pipeline,
    /// The Pause NAS downloads switch is on
    pub nas_paused: bool,
    /// Items that left the list within `[transfers] finished_retention_hours`
    pub recently_finished: Vec<FinishedItem>,
}

/// One Recently finished item: how it ended and where its time went
#[derive(Debug, Serialize)]
pub struct FinishedItem {
    pub key: String,
    pub name: String,
    /// `done` or `failed`
    pub outcome: &'static str,
    pub reason: Option<String>,
    pub size: i64,
    /// RFC 3339 UTC
    pub done_at: String,
    /// Added to done
    pub total_seconds: Option<i64>,
    /// Added to finished on Put.io
    pub putio_seconds: Option<i64>,
    /// Finished on Put.io to first file queued
    pub waiting_seconds: Option<i64>,
    /// First file queued to done
    pub nas_seconds: Option<i64>,
}

/// A stored time: SQLite's `YYYY-MM-DD HH:MM:SS`, or Put.io's `YYYY-MM-DDTHH:MM:SS`, UTC
fn parse_time(text: &str) -> Option<NaiveDateTime> {
    let text = text.get(..19)?;
    NaiveDateTime::parse_from_str(&text.replace('T', " "), "%Y-%m-%d %H:%M:%S").ok()
}

/// Seconds from `from` to `to`; `None` when either end is missing
fn between(from: &Option<NaiveDateTime>, to: &Option<NaiveDateTime>) -> Option<i64> {
    Some(
        to.as_ref()?
            .signed_duration_since(*from.as_ref()?)
            .num_seconds()
            .max(0),
    )
}

fn finished_item(row: FinishedRow) -> FinishedItem {
    let added = row.added_at.as_deref().and_then(parse_time);
    let putio_done = row.putio_done_at.as_deref().and_then(parse_time);
    let queued = row.queued_at.as_deref().and_then(parse_time);
    let done = parse_time(&row.done_at);
    FinishedItem {
        total_seconds: between(&added, &done),
        putio_seconds: between(&added, &putio_done),
        waiting_seconds: between(&putio_done, &queued),
        nas_seconds: between(&queued, &done),
        done_at: done.map_or(row.done_at, |t| {
            t.and_utc().to_rfc3339_opts(SecondsFormat::Secs, true)
        }),
        key: row.key,
        name: row.name,
        outcome: row.outcome.as_str(),
        reason: row.reason,
        size: row.size,
    }
}

/// GET /active
pub async fn get_active(State(state): State<AppState>) -> ApiResult<Json<ActiveResponse>> {
    let db = state.database.as_ref().ok_or_else(|| {
        ApiError::ServiceUnavailable("The Active page needs the database".to_string())
    })?;
    let retention = state.config.read().await.transfers.finished_retention_hours;
    let pipeline = state
        .download_manager
        .pipeline(MAX_AGE, &state.stall_tracker, retention)
        .await
        .map_err(|e| match e {
            DownloadError::ClientError(e) => {
                ApiError::ServiceUnavailable(format!("Put.io is unreachable: {e}"))
            }
            e => ApiError::InternalError(e.to_string()),
        })?;
    let nas_paused = state.download_manager.nas_paused().await;
    let recently_finished = db
        .finished()
        .list()
        .await
        .map_err(|e| ApiError::InternalError(e.to_string()))?
        .into_iter()
        .map(finished_item)
        .collect();
    Ok(Json(ActiveResponse {
        pipeline,
        nas_paused,
        recently_finished,
    }))
}

/// Most keys one pause or resume may name
pub const MAX_KEYS: usize = 500;

/// Body of `POST /active/pause` and `/active/resume`
#[derive(Debug, Deserialize)]
pub struct KeysRequest {
    pub keys: Vec<String>,
}

/// Response of `POST /active/pause` and `/active/resume`: one result per key, in order
#[derive(Debug, Serialize)]
pub struct ActionResponse {
    pub results: Vec<ItemResult>,
}

/// POST /active/pause
pub async fn pause(
    State(state): State<AppState>,
    Json(request): Json<KeysRequest>,
) -> ApiResult<Json<ActionResponse>> {
    check_keys(&request)?;
    let retention = state.config.read().await.transfers.finished_retention_hours;
    let results = state
        .download_manager
        .pause_items(&request.keys, &state.stall_tracker, retention)
        .await
        .map_err(|e| ApiError::InternalError(e.to_string()))?;
    Ok(Json(ActionResponse { results }))
}

/// POST /active/resume
pub async fn resume(
    State(state): State<AppState>,
    Json(request): Json<KeysRequest>,
) -> ApiResult<Json<ActionResponse>> {
    check_keys(&request)?;
    let retention = state.config.read().await.transfers.finished_retention_hours;
    let results = state
        .download_manager
        .resume_items(&request.keys, &state.stall_tracker, retention)
        .await
        .map_err(|e| ApiError::InternalError(e.to_string()))?;
    Ok(Json(ActionResponse { results }))
}

fn check_keys(request: &KeysRequest) -> ApiResult<()> {
    if request.keys.is_empty() || request.keys.len() > MAX_KEYS {
        return Err(ApiError::BadRequest(format!(
            "keys must have from 1 to {MAX_KEYS} entries"
        )));
    }
    Ok(())
}
