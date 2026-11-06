//! Translation settings (feature 026)
//!
//! - GET  /settings/translation - the translator, the chat server and the callback service;
//!   the API key and the callback header values are masked
//! - PUT  /settings/translation - replace them; they apply now. Masked values keep the
//!   stored ones
//! - POST /settings/translation/preview - what a PUT would do
//! - POST /settings/translation/test - translate two sample lines through the chat server,
//!   or send a sample job to the callback service

use super::assistant::Preview;
use super::settings::{body, invalid, preview, saved_message, update, AppState};
use crate::error::ApiError;
use axum::{
    body::Bytes,
    extract::{rejection::JsonRejection, State},
    routing::{get, post},
    Json, Router,
};
use putio_sync_core::config::Config;
use putio_sync_core::translation::{TranslationConfig, TranslatorKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The fields that apply to the next step without a restart
const LIVE: &[&str] = &[
    "kind",
    "excluded_categories",
    "max_in_flight",
    "timeout_secs",
    "chunk_cues",
    "instruction",
    "chat",
    "callback",
];
const WHEN: &str = "Applies now";
const APPLIES_NOW: &str =
    "New items and waiting steps use these settings; running steps finish as they are.";

/// The settings routes of this group
pub(crate) fn settings_router() -> Router<AppState> {
    Router::new()
        .route("/translation", get(get_translation).put(put_translation))
        .route("/translation/preview", post(preview_translation))
        .route("/translation/test", post(test_translation))
}

pub(crate) fn live_translation() -> (&'static [&'static str], &'static str) {
    (LIVE, WHEN)
}

/// The config with the key and header values shown as the mask
pub(crate) fn translation_view(config: &Config) -> TranslationConfig {
    config.translation.masked()
}

/// GET body
#[derive(Serialize)]
struct Full {
    #[serde(flatten)]
    settings: TranslationConfig,
    /// The wanted languages of the Subtitles settings: what gets translated into
    target_languages: Vec<String>,
    /// The URL chat requests go to; only for `kind = chat`
    resolved_endpoint: Option<String>,
    /// What the service is told to call; only when `callback_base_url` is set
    callback_url_form: Option<String>,
    live: &'static [&'static str],
    applies_now: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

fn full(config: &Config, warnings: Vec<String>) -> Full {
    let t = &config.translation;
    let base = &t.callback.callback_base_url;
    Full {
        settings: translation_view(config),
        target_languages: config.subtitles.wanted(),
        resolved_endpoint: (t.kind == TranslatorKind::Chat && !t.chat.base_url.trim().is_empty())
            .then(|| putio_sync_translation::chat::resolve_endpoint(&t.chat.base_url)),
        callback_url_form: (!base.is_empty())
            .then(|| putio_sync_translation::callback::callback_url(base, "<token>")),
        live: LIVE,
        applies_now: APPLIES_NOW,
        warnings,
    }
}

/// Notes the save still goes through with
fn warnings_for(config: &Config) -> Vec<String> {
    let mut warnings = Vec::new();
    if config.translation.is_configured() && config.subtitles.wanted().is_empty() {
        warnings.push(
            "no wanted languages in Subtitles settings: nothing will be translated".to_string(),
        );
    }
    warnings
}

async fn get_translation(State(state): State<AppState>) -> Json<Full> {
    let config = state.config.read().await;
    Json(full(&config, warnings_for(&config)))
}

fn keys_changed(before: &TranslationConfig, after: &TranslationConfig) -> bool {
    before.chat.api_key != after.chat.api_key || before.callback.headers != after.callback.headers
}

async fn put_translation(
    State(state): State<AppState>,
    settings: Result<Json<TranslationConfig>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let settings = body(settings)?;
    let before = state.config.read().await.translation.clone();
    let mut changed = update(&state, translation_view, |config| {
        config.translation = settings.unmasked(&config.translation);
        config.translation.normalize().map_err(invalid)?;
        Ok(())
    })
    .await?;
    let config = state.config.read().await.clone();
    // The view shows secrets as the mask, so a new key is not a visible change
    if keys_changed(&before, &config.translation)
        && !changed.iter().any(|f| f == "chat" || f == "callback")
    {
        changed.push(if before.chat.api_key != config.translation.chat.api_key {
            "chat".to_string()
        } else {
            "callback".to_string()
        });
    }
    if let Some(pipeline) = &state.pipeline {
        pipeline.configure(&config);
    }
    let (live, when) = live_translation();
    let mut out = serde_json::to_value(full(&config, warnings_for(&config))).unwrap_or_default();
    out["message"] = Value::String(saved_message("Translation", &changed, live, when, &[]));
    Ok(Json(out))
}

async fn preview_translation(
    State(state): State<AppState>,
    settings: Result<Json<TranslationConfig>, JsonRejection>,
) -> Result<Json<Preview>, ApiError> {
    let settings = body(settings)?;
    Ok(preview(&state, translation_view, live_translation(), |c| {
        c.translation = settings.unmasked(&c.translation);
        c.translation.normalize().map_err(invalid)?;
        Ok(())
    })
    .await)
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct TestBody {
    translation: Option<TranslationConfig>,
    sample_target: Option<String>,
}

/// POST /settings/translation/test
///
/// The body is optional: `{"translation": {...}}` tests unsaved settings, and masked values
/// stand for the stored ones. A completed test is always 200; settings that are invalid for
/// their kind are 400.
async fn test_translation(
    State(state): State<AppState>,
    raw: Bytes,
) -> Result<Json<Value>, ApiError> {
    let parsed: TestBody = if raw.iter().all(u8::is_ascii_whitespace) {
        TestBody::default()
    } else {
        serde_json::from_slice(&raw)
            .map_err(|e| ApiError::BadRequest(format!("invalid body: {e}")))?
    };
    let (stored, wanted) = {
        let config = state.config.read().await;
        (config.translation.clone(), config.subtitles.wanted())
    };
    let mut cfg = match parsed.translation {
        Some(t) => t.unmasked(&stored),
        None => stored,
    };
    cfg.normalize().map_err(invalid)?;
    let target = parsed
        .sample_target
        .as_deref()
        .and_then(putio_sync_core::lang::lookup)
        .or_else(|| wanted.first().cloned())
        .unwrap_or_else(|| "es".to_string());
    match cfg.kind {
        TranslatorKind::None => Err(ApiError::BadRequest(
            "no translator is selected: choose a chat model or a callback service".to_string(),
        )),
        TranslatorKind::Chat => {
            let key = cfg.chat.effective_api_key();
            let probe = putio_sync_translation::probe::chat(
                &cfg.chat.base_url,
                &cfg.chat.model,
                &key,
                &target,
                &cfg.instruction,
            )
            .await;
            let mut out = serde_json::to_value(&probe).unwrap_or_default();
            out["kind"] = Value::String("chat".to_string());
            Ok(Json(out))
        }
        TranslatorKind::Callback => Err(ApiError::BadRequest(
            "testing a callback service is not available yet".to_string(),
        )),
    }
}
