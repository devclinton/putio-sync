//! Organising files stream mode keeps on Put.io (feature 023)
//!
//! - GET  /settings/organise - The saved `[organise]` settings
//! - PUT  /settings/organise - Save them; the next sync pass uses them
//! - POST /settings/organise/preview - What a PUT would do
//! - GET  /organise - Whether the organiser runs here, and its moves
//! - POST /organise/apply, /organise/undo, /organise/dismiss - `{file_ids}`, one result each
//! - POST /organise/scan - Plan moves for files streamed before organising was on

use super::settings::{body, preview, saved_message, update};
use crate::error::ApiError;
use crate::models::SuccessResponse;
use crate::AppState;
use axum::{
    extract::{rejection::JsonRejection, State},
    routing::{get, post},
    Json, Router,
};
use putio_sync_core::config::Config;
use putio_sync_core::organise::OrganiseConfig;
use putio_sync_db::OrganiseMove;
use putio_sync_download::{MoveOutcome, Organiser};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// GET /organise
#[derive(Debug, Serialize)]
pub struct OrganiseBody {
    /// False when the daemon is not the engine; the moves are then empty
    pub available: bool,
    pub moves: Vec<MoveView>,
}

/// One move as the Put.io page shows it
#[derive(Debug, Serialize)]
pub struct MoveView {
    pub file_id: i64,
    pub kind: String,
    pub from_folder: String,
    pub from_name: String,
    pub to_folder: String,
    pub to_name: String,
    pub by_plugin: bool,
    pub status: &'static str,
    pub error: Option<String>,
    pub updated_at: String,
}

impl From<OrganiseMove> for MoveView {
    fn from(m: OrganiseMove) -> Self {
        Self {
            file_id: m.file_id,
            kind: m.kind,
            from_folder: m.from_folder,
            from_name: m.from_name,
            to_folder: m.to_folder,
            to_name: m.to_name,
            by_plugin: m.by_plugin,
            status: m.status.as_str(),
            error: m.error,
            updated_at: m.updated_at,
        }
    }
}

/// POST /organise/apply|undo|dismiss
#[derive(Debug, Deserialize)]
pub struct FileIds {
    pub file_ids: Vec<i64>,
}

/// The reply: what happened to each id
#[derive(Debug, Serialize)]
pub struct OutcomesBody {
    pub results: Vec<MoveOutcome>,
}

/// POST /organise/scan
#[derive(Debug, Serialize)]
pub struct ScanBody {
    pub planned: usize,
}

/// The settings routes, merged into `/settings`
pub(crate) fn settings_router() -> Router<AppState> {
    Router::new()
        .route("/organise", get(get_settings).put(update_settings))
        .route("/organise/preview", post(preview_settings))
}

/// The moves routes, nested at `/organise`
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/apply", post(apply))
        .route("/undo", post(undo))
        .route("/dismiss", post(dismiss))
        .route("/scan", post(scan))
}

pub(crate) fn organise_view(config: &Config) -> OrganiseConfig {
    config.organise.clone()
}

/// Every field applies at the next sync pass
pub(crate) fn live_organise() -> (&'static [&'static str], &'static str) {
    (
        &[
            "tv",
            "tv_folder",
            "movies",
            "movies_folder",
            "apply",
            "use_plugins",
        ],
        "Applies at the next sync",
    )
}

fn organiser(state: &AppState) -> Result<&Arc<Organiser>, ApiError> {
    state.organiser.as_ref().ok_or_else(|| {
        ApiError::ServiceUnavailable(
            "Organising on Put.io runs under `daemon`; this engine only saves the settings."
                .to_string(),
        )
    })
}

fn ids(body: Result<Json<FileIds>, JsonRejection>) -> Result<Vec<i64>, ApiError> {
    let FileIds { file_ids } = super::settings::body(body)?;
    if file_ids.is_empty() || file_ids.len() > putio_sync_download::organise::MAX_BATCH {
        return Err(ApiError::BadRequest(format!(
            "file_ids must hold 1 to {} ids",
            putio_sync_download::organise::MAX_BATCH
        )));
    }
    Ok(file_ids)
}

/// GET /settings/organise
async fn get_settings(State(state): State<AppState>) -> Json<OrganiseConfig> {
    Json(organise_view(&*state.config.read().await))
}

/// PUT /settings/organise
async fn update_settings(
    State(state): State<AppState>,
    settings: Result<Json<OrganiseConfig>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    let changed = update(&state, organise_view, |config| {
        config.organise = settings;
        Ok(())
    })
    .await?;
    let mut notes = Vec::new();
    match &state.organiser {
        Some(o) => o.configure(state.config.read().await.organise.clone()),
        None => notes.push(
            "Organising on Put.io runs under `daemon`; this engine only saves the settings."
                .to_string(),
        ),
    }
    let (fields, when) = live_organise();
    Ok(Json(SuccessResponse {
        message: saved_message("Organise", &changed, fields, when, &notes),
    }))
}

/// POST /settings/organise/preview
async fn preview_settings(
    State(state): State<AppState>,
    settings: Result<Json<OrganiseConfig>, JsonRejection>,
) -> Result<Json<super::assistant::Preview>, ApiError> {
    let settings = body(settings)?;
    Ok(preview(&state, organise_view, live_organise(), |c| {
        c.organise = settings;
        Ok(())
    })
    .await)
}

/// GET /organise
async fn list(State(state): State<AppState>) -> Result<Json<OrganiseBody>, ApiError> {
    let Some(o) = &state.organiser else {
        return Ok(Json(OrganiseBody {
            available: false,
            moves: Vec::new(),
        }));
    };
    let moves = o
        .list()
        .await
        .map_err(|e| ApiError::InternalError(e.to_string()))?;
    Ok(Json(OrganiseBody {
        available: true,
        moves: moves.into_iter().map(MoveView::from).collect(),
    }))
}

/// POST /organise/apply
async fn apply(
    State(state): State<AppState>,
    body: Result<Json<FileIds>, JsonRejection>,
) -> Result<Json<OutcomesBody>, ApiError> {
    let ids = ids(body)?;
    let results = organiser(&state)?.apply(&ids).await;
    Ok(Json(OutcomesBody { results }))
}

/// POST /organise/undo
async fn undo(
    State(state): State<AppState>,
    body: Result<Json<FileIds>, JsonRejection>,
) -> Result<Json<OutcomesBody>, ApiError> {
    let ids = ids(body)?;
    let results = organiser(&state)?.undo(&ids).await;
    Ok(Json(OutcomesBody { results }))
}

/// POST /organise/dismiss
async fn dismiss(
    State(state): State<AppState>,
    body: Result<Json<FileIds>, JsonRejection>,
) -> Result<Json<OutcomesBody>, ApiError> {
    let ids = ids(body)?;
    let results = organiser(&state)?.dismiss(&ids).await;
    Ok(Json(OutcomesBody { results }))
}

/// POST /organise/scan
async fn scan(State(state): State<AppState>) -> Result<Json<ScanBody>, ApiError> {
    let planned = organiser(&state)?
        .scan()
        .await
        .map_err(|e| ApiError::InternalError(e.to_string()))?;
    Ok(Json(ScanBody { planned }))
}
