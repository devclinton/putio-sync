//! "Translate subtitles" on Local files (feature 026, contracts/api.md):
//! - GET  /local/files/{root}/translate?path= - whether a translator is set up, the
//!   subtitles already next to the video, and the languages that can be asked for
//! - POST /local/files/{root}/translate       - `{path, targets}`: start a `manual:<n>`
//!   pipeline item with one translation per language
//!
//! Paths are guarded like the other 044 routes.

use super::files::{resolve, AppState};
use super::subtitles::{Existing, PathQuery};
use crate::error::ApiError;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use putio_sync_core::translation::TranslatorKind;
use putio_sync_pipeline::subtitles::{is_video, ManualError};
use putio_sync_pipeline::translate::MANUAL_MAX_TARGETS;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Deserialize)]
pub struct StartRequest {
    pub path: String,
    #[serde(default)]
    pub targets: Vec<String>,
}

#[derive(Serialize)]
pub struct Preflight {
    pub eligible: bool,
    pub reason: Option<String>,
    /// `none`, `chat` or `callback`
    pub translator: &'static str,
    /// The wanted languages (Subtitles settings), the usual choices
    pub languages: Vec<String>,
    /// Wanted languages with no subtitle next to the video yet
    pub missing: Vec<String>,
    pub existing: Vec<Existing>,
    pub max_targets: usize,
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// GET /local/files/{root}/translate
pub async fn preflight(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Query(q): Query<PathQuery>,
) -> Result<Json<Preflight>, ApiError> {
    let (root, target) = resolve(&state, &key, &q.path).await?;
    let _ = root;
    match std::fs::metadata(&target.path) {
        Ok(m) if m.is_file() => {}
        Ok(_) => {
            return Err(ApiError::BadRequest(format!(
                "{} is a folder, not a video",
                target.rel
            )))
        }
        Err(_) => return Err(ApiError::NotFound(format!("No file at {}", target.rel))),
    }
    let (translator, languages) = {
        let config = state.config.read().await;
        (config.translation.kind, config.subtitles.wanted())
    };
    let existing: Vec<Existing> = putio_sync_subtitles::naming::existing_sidecars(&target.path)
        .into_iter()
        .map(|e| Existing {
            language: e.language,
            name: file_name(&e.path),
        })
        .collect();
    let missing = languages
        .iter()
        .filter(|l| !existing.iter().any(|e| &e.language == *l))
        .cloned()
        .collect();
    let reason = if !is_video(&file_name(&target.path)) {
        Some("This is not a video file".to_string())
    } else if translator == TranslatorKind::None {
        Some("No translator is set up: choose one in Settings, Downloads, Translation".to_string())
    } else if existing.is_empty() {
        Some("There is no subtitle next to this video to translate from".to_string())
    } else {
        None
    };
    Ok(Json(Preflight {
        eligible: reason.is_none(),
        reason,
        translator: translator.as_str(),
        languages,
        missing,
        existing,
        max_targets: MANUAL_MAX_TARGETS,
    }))
}

/// POST /local/files/{root}/translate
pub async fn start(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(body): Json<StartRequest>,
) -> Result<Response, ApiError> {
    let (root, target) = resolve(&state, &key, &body.path).await?;
    match std::fs::metadata(&target.path) {
        Ok(m) if m.is_file() => {}
        Ok(_) => {
            return Err(ApiError::BadRequest(format!(
                "{} is a folder, not a video",
                target.rel
            )))
        }
        Err(_) => return Err(ApiError::NotFound(format!("No file at {}", target.rel))),
    }
    let name = file_name(&target.path);
    if !is_video(&name) {
        return Err(ApiError::BadRequest(format!("{name} is not a video file")));
    }
    if state.config.read().await.translation.kind == TranslatorKind::None {
        return Err(ApiError::BadRequest(
            "No translator is set up: choose one in Settings, Downloads, Translation".into(),
        ));
    }
    if body.targets.is_empty() || body.targets.len() > MANUAL_MAX_TARGETS {
        return Err(ApiError::BadRequest(format!(
            "Choose from 1 to {MANUAL_MAX_TARGETS} languages"
        )));
    }
    let mut targets = Vec::new();
    for t in &body.targets {
        match putio_sync_core::lang::lookup(t) {
            Some(code) => targets.push(code),
            None => {
                return Err(ApiError::BadRequest(format!(
                    "\"{t}\" is not a known language"
                )))
            }
        }
    }
    let Some(pipeline) = &state.pipeline else {
        return Err(ApiError::ServiceUnavailable(
            "The processing pipeline is not running".into(),
        ));
    };
    let video = root.path.join(&target.rel);
    match pipeline.start_manual_translation(&video, &targets).await {
        Ok(key) => {
            info!(key = %key, path = %target.rel, languages = ?targets, "Manual translation started");
            Ok((StatusCode::ACCEPTED, Json(serde_json::json!({ "key": key }))).into_response())
        }
        Err(ManualError::Duplicate(key)) => Ok((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("{name} is already being translated (see {key} on the Active page)"),
                "key": key,
            })),
        )
            .into_response()),
        Err(ManualError::NotAVideo) => Err(ApiError::BadRequest(format!("{name} is not a video file"))),
        Err(ManualError::Failed(why)) => Err(ApiError::BadRequest(why)),
    }
}
