//! Active transfers pipeline route (feature 039, contracts/api.md)
//!
//! - GET  /active        - every transfer on its way to the NAS, by stage and state, with counts
//! - POST /active/pause  - pause items by key (`{"keys": [...]}`): a stop on the NAS, a hold on Put.io
//! - POST /active/resume - resume items by key
//! - GET  /active/:key/timeline - the stages and steps an item went through (feature 038)
//! - POST /active/:key/steps/:id/retry|skip - the operator's answer to a failed step
//!
//! Put.io figures come from the shared transfers snapshot, at most [`MAX_AGE`] old; NAS
//! figures are read on each request.

use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
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
    /// The download view, each item with its processing `step` and, after them, the
    /// items that are downloaded but still being processed (feature 038)
    #[serde(flatten)]
    pub pipeline: serde_json::Value,
    /// The Pause local downloads switch is on
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
    /// The item finished its processing steps with warnings (feature 038); its timeline
    /// has the detail
    pub warnings: bool,
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
        warnings: false,
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
    let mut recently_finished: Vec<FinishedItem> = db
        .finished()
        .list()
        .await
        .map_err(|e| ApiError::InternalError(e.to_string()))?
        .into_iter()
        .map(finished_item)
        .collect();
    if let Some(pipeline) = &state.pipeline {
        for item in &mut recently_finished {
            item.warnings = pipeline.has_warnings(&item.key).await;
        }
    }
    let pipeline = with_processing(&state, pipeline).await;
    Ok(Json(ActiveResponse {
        pipeline,
        nas_paused,
        recently_finished,
    }))
}

/// The download view as JSON, with a `step` on every item (feature 038: the step an item
/// is at, `null` while it is downloading or has none) and a row for each item that is
/// downloaded but still in processing. Their `stage` is `processing`; `counts.processing`
/// counts them by state.
async fn with_processing(state: &AppState, view: Pipeline) -> serde_json::Value {
    use serde_json::{json, Value};
    let mut value = serde_json::to_value(&view).unwrap_or(Value::Null);
    let Some(pipeline) = &state.pipeline else {
        return value;
    };
    let summaries = pipeline.summaries().await;
    let processing = pipeline.processing_items().await;
    let step = |key: &str| {
        summaries
            .get(key)
            .and_then(|s| serde_json::to_value(s).ok())
            .unwrap_or(Value::Null)
    };
    if let Some(items) = value.get_mut("items").and_then(Value::as_array_mut) {
        for item in items.iter_mut() {
            let key = item.get("key").and_then(Value::as_str).unwrap_or_default();
            let summary = step(key);
            item["step"] = summary;
        }
        let listed: std::collections::HashSet<String> = items
            .iter()
            .filter_map(|i| i.get("key").and_then(Value::as_str).map(str::to_string))
            .collect();
        for row in processing.iter().filter(|r| !listed.contains(&r.key)) {
            items.push(json!({
                "key": row.key,
                "name": row.name,
                "transfer_id": row.transfer_id,
                "stage": "processing",
                "state": row.state.as_str(),
                "held": row.state == putio_sync_pipeline::ItemState::Held,
                "paused": false,
                "size": 0,
                "done_bytes": 0,
                "percent": 100.0,
                "speed_bps": null,
                "eta_seconds": null,
                "files": null,
                "putio_status": null,
                "seeding": false,
                "stall": null,
                "origin": null,
                "reason": null,
                "next_sync_at": null,
                "note": null,
                "phase": null,
                "links": {},
                "step": step(&row.key),
            }));
        }
    }
    let mut counts = json!({"running": 0, "held": 0, "waiting_for_operator": 0});
    for row in &processing {
        let n = counts[row.state.as_str()].as_u64().unwrap_or(0) + 1;
        counts[row.state.as_str()] = json!(n);
    }
    if let Some(all) = value.get_mut("counts") {
        all["processing"] = counts;
    }
    value
}

/// GET /active/:key/timeline
pub async fn timeline(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> ApiResult<Json<putio_sync_pipeline::Timeline>> {
    let timeline = match &state.pipeline {
        Some(pipeline) => pipeline.timeline(&key).await,
        None => None,
    };
    timeline
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("No pipeline record for {key}")))
}

fn action_error(e: putio_sync_pipeline::ActionError) -> ApiError {
    match e {
        putio_sync_pipeline::ActionError::NotFound => {
            ApiError::NotFound("No such item or step".to_string())
        }
        putio_sync_pipeline::ActionError::Conflict(why) => ApiError::Conflict(why),
    }
}

/// POST /active/:key/steps/:id/retry
pub async fn retry_step(
    State(state): State<AppState>,
    Path((key, id)): Path<(String, i64)>,
) -> ApiResult<StatusCode> {
    let pipeline = state
        .pipeline
        .as_ref()
        .ok_or_else(|| ApiError::NotFound("No such item or step".to_string()))?;
    pipeline.retry(&key, id).await.map_err(action_error)?;
    Ok(StatusCode::ACCEPTED)
}

/// POST /active/:key/steps/:id/skip
pub async fn skip_step(
    State(state): State<AppState>,
    Path((key, id)): Path<(String, i64)>,
) -> ApiResult<StatusCode> {
    let pipeline = state
        .pipeline
        .as_ref()
        .ok_or_else(|| ApiError::NotFound("No such item or step".to_string()))?;
    pipeline.skip(&key, id).await.map_err(action_error)?;
    Ok(StatusCode::ACCEPTED)
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
