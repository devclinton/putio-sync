//! "Fetch subtitles" on Local files (feature 025, contracts/api.md):
//! - GET  /local/files/{root}/subtitles?path= - whether Put.io can still help, whether
//!   transcription can, the wanted languages and the subtitle files already there
//! - POST /local/files/{root}/subtitles       - `{path}`: start a `manual:<n>` pipeline item
//!
//! Paths are guarded like the other 044 routes: `local_files::resolve` keeps them inside
//! the root.

use super::files::{resolve, AppState};
use crate::error::ApiError;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use putio_sync_core::subtitles::SubtitleMode;
use putio_sync_pipeline::subtitles::{is_video, ManualError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Deserialize)]
pub struct PathQuery {
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct StartRequest {
    pub path: String,
}

#[derive(Serialize)]
pub struct Existing {
    pub language: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct Transcription {
    pub enabled: bool,
    pub ffmpeg: bool,
}

#[derive(Serialize)]
pub struct Preflight {
    pub eligible: bool,
    pub reason: Option<String>,
    /// `available`, `gone` or `unknown`
    pub putio: &'static str,
    /// What the Subtitles settings do with a video outside any category (feature 050)
    pub mode: &'static str,
    pub transcription: Transcription,
    pub languages: Vec<String>,
    pub existing: Vec<Existing>,
}

/// The file on disk: it must exist and be a file
fn require_file(path: &std::path::Path, rel: &str) -> Result<(), ApiError> {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() => Ok(()),
        Ok(_) => Err(ApiError::BadRequest(format!(
            "{rel} is a folder, not a video"
        ))),
        Err(_) => Err(ApiError::NotFound(format!("No file at {rel}"))),
    }
}

/// The path the downloader recorded for this file: the root as configured plus the relative
/// path (the guard's canonical form can differ when the root has symlinks)
fn recorded_path(root: &super::files::Root, rel: &str) -> PathBuf {
    root.path.join(rel)
}

/// Whether Put.io still has the file this one was downloaded from
async fn putio_state(state: &AppState, path: &std::path::Path) -> &'static str {
    let Some(db) = state.database.as_deref() else {
        return "unknown";
    };
    let Ok(Some(download)) = db
        .downloads()
        .find_by_local_path(&path.to_string_lossy())
        .await
    else {
        return "unknown";
    };
    match state.client.get_file_details(download.file_id).await {
        Ok(_) => "available",
        Err(e) if e.is_not_found() => "gone",
        Err(_) => "unknown",
    }
}

/// GET /local/files/{root}/subtitles
pub async fn preflight(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Query(q): Query<PathQuery>,
) -> Result<Json<Preflight>, ApiError> {
    let (root, target) = resolve(&state, &key, &q.path).await?;
    require_file(&target.path, &target.rel)?;
    let config = state.config.read().await.subtitles.clone();
    let languages = config.wanted();
    let mode = config.category_mode(None);
    let ffmpeg =
        putio_sync_subtitles::audio::find_ffmpeg(&config.transcription.ffmpeg_path).is_some();
    let transcription = Transcription {
        enabled: config.transcription.enabled && !config.transcription.url.trim().is_empty(),
        ffmpeg,
    };
    let recorded = recorded_path(&root, &target.rel);
    let name = target
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let existing = putio_sync_subtitles::naming::existing_sidecars(&target.path)
        .into_iter()
        .map(|e| Existing {
            language: e.language,
            name: e
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        })
        .collect();

    let (putio, reason) = if !is_video(&name) {
        ("unknown", Some("This is not a video file".to_string()))
    } else if languages.is_empty() {
        (
            "unknown",
            Some(
                "No subtitle languages are set: add some in Settings, Downloads, Subtitles"
                    .to_string(),
            ),
        )
    } else {
        let can_transcribe = transcription.enabled && transcription.ffmpeg;
        if mode == SubtitleMode::WhisperOnly {
            // Put.io is not asked in this mode, so only transcription has to work
            let reason =
                (!can_transcribe).then(|| "Transcription is off or ffmpeg is missing".to_string());
            ("unknown", reason)
        } else {
            let putio = putio_state(&state, &recorded).await;
            let needs_putio = !can_transcribe || mode == SubtitleMode::DownloadOnly;
            let reason = (putio != "available" && needs_putio).then(|| {
                let why = if putio == "gone" {
                    "Put.io no longer has this file"
                } else {
                    "Put.io cannot be asked about this file (it was not downloaded by putio-sync)"
                };
                if mode == SubtitleMode::DownloadOnly {
                    why.to_string()
                } else {
                    format!("{why}, and transcription is off or ffmpeg is missing")
                }
            });
            (putio, reason)
        }
    };
    Ok(Json(Preflight {
        eligible: reason.is_none(),
        reason,
        putio,
        mode: mode.as_str(),
        transcription,
        languages,
        existing,
    }))
}

/// POST /local/files/{root}/subtitles
pub async fn start(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(body): Json<StartRequest>,
) -> Result<Response, ApiError> {
    let (root, target) = resolve(&state, &key, &body.path).await?;
    require_file(&target.path, &target.rel)?;
    let name = target
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !is_video(&name) {
        return Err(ApiError::BadRequest(format!("{name} is not a video file")));
    }
    if state.config.read().await.subtitles.wanted().is_empty() {
        return Err(ApiError::BadRequest(
            "No subtitle languages are set: add some in Settings, Downloads, Subtitles".into(),
        ));
    }
    let Some(pipeline) = &state.pipeline else {
        return Err(ApiError::ServiceUnavailable(
            "The processing pipeline is not running".into(),
        ));
    };
    let video = recorded_path(&root, &target.rel);
    match pipeline.start_manual_subtitles(&video).await {
        Ok(key) => {
            info!(key = %key, path = %target.rel, "Manual subtitles started");
            Ok((StatusCode::ACCEPTED, Json(serde_json::json!({ "key": key }))).into_response())
        }
        Err(ManualError::Duplicate(key)) => Ok((
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("Subtitles are already being fetched for {name} (see {key} on the Active page)"),
                "key": key,
            })),
        )
            .into_response()),
        Err(ManualError::NotAVideo) => Err(ApiError::BadRequest(format!("{name} is not a video file"))),
        Err(ManualError::Failed(why)) => Err(ApiError::InternalError(why)),
    }
}
