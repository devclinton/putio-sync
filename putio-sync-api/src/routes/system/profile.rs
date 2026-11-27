//! Settings › Usage profile (feature 056): run the setup wizard's profile again.
//!
//! The same plan as setup mode ([`putio_sync_core::profile`]) against the running config.
//! Settings changed by hand since the profile set them are kept and listed. The save goes
//! through the normal settings save, so it applies live (the release check's sizes wait
//! for a restart, as any `[precheck]` change).

use crate::error::ApiError;
use crate::routes::setup::{normalized, review, with_profile, ProfileBody};
use crate::routes::system::settings::{body, update};
use crate::setup::ProfileDraft;
use crate::AppState;
use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use putio_sync_core::config::Config;
use putio_sync_core::profile::{self, ProfileConfig};
use putio_sync_core::setup::{check_folder, preset_choices, suggest_watch_dir};
use serde_json::{json, Value};

/// The routes, nested under `/settings`
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/profile", get(get_profile).put(put_profile))
        .route("/profile/preview", post(preview_profile))
}

/// The top-level Put.io folders, or the ones saved with the profile when Put.io can't be
/// reached
async fn folders(state: &AppState, saved: Option<&ProfileConfig>) -> Vec<String> {
    match state.client.list_files(0).await {
        Ok(listing) => listing
            .files
            .into_iter()
            .filter(|f| f.file_type == "FOLDER")
            .map(|f| f.name)
            .collect(),
        Err(_) => saved
            .map(|p| p.folders.values().flatten().cloned().collect())
            .unwrap_or_default(),
    }
}

fn view(config: &Config) -> Option<ProfileConfig> {
    config.profile.clone()
}

/// The draft for a body: answers checked, folders detected again
async fn draft(state: &AppState, body: ProfileBody) -> Result<ProfileDraft, ApiError> {
    let answers = body
        .answers
        .map(normalized)
        .ok_or_else(|| ApiError::BadRequest("Answer the usage questions".into()))?;
    answers
        .validate()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let saved = state.config.read().await.profile.clone();
    let found = profile::detect(&folders(state, saved.as_ref()).await);
    Ok(ProfileDraft {
        answers,
        overrides: body.overrides,
        found,
    })
}

/// GET /settings/profile: the saved answers, and what Put.io's folders suggest now
async fn get_profile(State(state): State<AppState>) -> Json<Value> {
    let config = state.config.read().await.clone();
    let folders = folders(&state, config.profile.as_ref()).await;
    Json(json!({
        "answers": config.profile.as_ref().map(ProfileConfig::answers),
        "found": profile::detect(&folders),
        "folders": folders,
        "watch_dir": suggest_watch_dir(Some(&config)),
        "tailscale": crate::routes::setup::tailscale_found(&config),
        "presets": preset_choices(&config.download)
            .into_iter()
            .map(|(name, label)| json!({ "name": name, "label": label }))
            .collect::<Vec<_>>(),
    }))
}

/// POST /settings/profile/preview: the review, with what changes and what is kept
async fn preview_profile(
    State(state): State<AppState>,
    payload: Result<Json<ProfileBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let draft = draft(&state, body(payload)?).await?;
    let config = state.config.read().await.clone();
    let (_, review) = review(&config, &draft)?;
    Ok(Json(review))
}

/// PUT /settings/profile: save the profile's settings, keeping hand-made changes
async fn put_profile(
    State(state): State<AppState>,
    payload: Result<Json<ProfileBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let draft = draft(&state, body(payload)?).await?;
    if let Some(watch) = &draft.answers.watch {
        // Made now, as setup makes it at finish
        let check = check_folder(watch, None, true);
        if !check.usable() {
            return Err(ApiError::BadRequest(format!(
                "The watch folder {} can't be used: {}",
                watch.display(),
                check.error.unwrap_or_else(|| "not writable".into())
            )));
        }
    }
    let mut diff = None;
    update(&state, view, |config| {
        let (next, _, d) = with_profile(config, &draft)?;
        *config = next;
        diff = Some(d);
        Ok(())
    })
    .await?;
    let diff = diff.unwrap_or_default();
    Ok(Json(json!({ "changes": diff.changes, "kept": diff.kept })))
}
