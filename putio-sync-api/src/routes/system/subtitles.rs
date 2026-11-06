//! Subtitles settings (feature 025)
//!
//! - GET  /settings/subtitles - the wanted languages, excluded categories and transcription
//!   server, the API key masked
//! - PUT  /settings/subtitles - replace them; they apply now. A masked key keeps the stored one
//! - POST /settings/subtitles/preview - what a PUT would do, and the file names a video would get
//! - POST /settings/subtitles/test - send a short tone to the transcription server and look
//!   for ffmpeg

use super::assistant::Preview;
use super::settings::{body, preview, saved_message, update, AppState};
use crate::error::ApiError;
use axum::{
    body::Bytes,
    extract::{rejection::JsonRejection, State},
    routing::{get, post},
    Json, Router,
};
use putio_sync_core::config::Config;
use putio_sync_core::subtitles::{SubtitlesConfig, MASK};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// The fields that apply to the next step without a restart
const LIVE: &[&str] = &[
    "languages",
    "excluded_categories",
    "transcription",
    "mode",
    "category_modes",
    "language_modes",
    "min_confidence",
    "opensubtitles",
];
const WHEN: &str = "Applies now";
const APPLIES_NOW: &str =
    "New items and waiting steps use these settings; running steps finish as they are.";

/// The settings routes of this group
pub(crate) fn settings_router() -> Router<AppState> {
    Router::new()
        .route("/subtitles", get(get_subtitles).put(put_subtitles))
        .route("/subtitles/preview", post(preview_subtitles))
        .route("/subtitles/test", post(test_subtitles))
}

pub(crate) fn live_subtitles() -> (&'static [&'static str], &'static str) {
    (LIVE, WHEN)
}

/// The config with a stored key shown as the mask
pub(crate) fn subtitles_view(config: &Config) -> SubtitlesConfig {
    config.subtitles.masked()
}

/// GET body
#[derive(Serialize)]
struct Full {
    #[serde(flatten)]
    settings: SubtitlesConfig,
    ffmpeg: Ffmpeg,
    /// The mode that applies when nothing narrower is set, and the match threshold, spelled
    /// out because a stored default is left out of `settings`
    effective_mode: &'static str,
    effective_min_confidence: u8,
    /// Whether an OpenSubtitles key is set (here or in the environment)
    opensubtitles_on: bool,
    known_languages: Vec<Language>,
    live: &'static [&'static str],
    applies_now: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

/// Whether ffmpeg can be found with the configured path
#[derive(Serialize)]
struct Ffmpeg {
    found: bool,
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

fn ffmpeg_info(config: &putio_sync_core::subtitles::TranscriptionConfig) -> Ffmpeg {
    let path = putio_sync_subtitles::audio::find_ffmpeg(&config.ffmpeg_path);
    Ffmpeg {
        found: path.is_some(),
        path: path.map(|p| p.to_string_lossy().into_owned()),
        version: None,
    }
}

#[derive(Serialize)]
struct Language {
    code: String,
    name: String,
}

fn full(config: &Config, warnings: Vec<String>) -> Full {
    Full {
        settings: subtitles_view(config),
        ffmpeg: ffmpeg_info(&config.subtitles.transcription),
        effective_mode: config.subtitles.category_mode(None).as_str(),
        effective_min_confidence: config.subtitles.min_confidence,
        opensubtitles_on: config.subtitles.opensubtitles.is_active(),
        known_languages: putio_sync_core::lang::known()
            .into_iter()
            .map(|(code, name)| Language { code, name })
            .collect(),
        live: LIVE,
        applies_now: APPLIES_NOW,
        warnings,
    }
}

/// The body of `full`, with the fields a default config leaves out spelled out, so the page
/// and the settings help can rely on them being there
fn body_of(config: &Config, warnings: Vec<String>) -> Value {
    let mut out = serde_json::to_value(full(config, warnings)).unwrap_or_default();
    let s = &config.subtitles.masked();
    for (name, value) in [
        ("mode", serde_json::json!(s.mode)),
        ("category_modes", serde_json::json!(s.category_modes)),
        ("language_modes", serde_json::json!(s.language_modes)),
        ("min_confidence", serde_json::json!(s.min_confidence)),
        ("opensubtitles", serde_json::json!(s.opensubtitles)),
    ] {
        out[name] = value;
    }
    out
}

async fn get_subtitles(State(state): State<AppState>) -> Json<Value> {
    Json(body_of(&*state.config.read().await, Vec::new()))
}

/// Keep the stored key when the body has the mask
fn with_stored_key(mut settings: SubtitlesConfig, stored: &SubtitlesConfig) -> SubtitlesConfig {
    settings = settings.unmasked(stored);
    // Canonical codes, once each; a name validation will refuse stays as typed
    let mut languages: Vec<String> = Vec::new();
    for l in &settings.languages {
        let code = putio_sync_core::lang::lookup(l).unwrap_or_else(|| l.trim().to_string());
        if !languages.contains(&code) {
            languages.push(code);
        }
    }
    settings.languages = languages;
    settings
}

async fn put_subtitles(
    State(state): State<AppState>,
    settings: Result<Json<SubtitlesConfig>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let settings = body(settings)?;
    let before = state.config.read().await.subtitles.clone();
    let mut changed = update(&state, subtitles_view, |config| {
        config.subtitles = with_stored_key(settings, &config.subtitles);
        // Canonical keys for the mode maps are kept, not just checked
        config
            .subtitles
            .normalize()
            .map_err(super::settings::invalid)
    })
    .await?;
    let config = state.config.read().await.clone();
    // The view shows a key as the mask, so a new key is not a visible change
    if config.subtitles.transcription.api_key != before.transcription.api_key
        && !changed.iter().any(|f| f == "transcription")
    {
        changed.push("transcription".to_string());
    }
    if (config.subtitles.opensubtitles.api_key != before.opensubtitles.api_key
        || config.subtitles.opensubtitles.password != before.opensubtitles.password)
        && !changed.iter().any(|f| f == "opensubtitles")
    {
        changed.push("opensubtitles".to_string());
    }
    if let Some(pipeline) = &state.pipeline {
        pipeline.configure(&config);
    }
    let mut warnings = Vec::new();
    transcription_warnings(&config, &mut warnings);
    let (live, when) = live_subtitles();
    let mut out = body_of(&config, warnings);
    out["message"] = Value::String(saved_message("Subtitles", &changed, live, when, &[]));
    Ok(Json(out))
}

/// Notes the save still goes through with
fn transcription_warnings(config: &Config, warnings: &mut Vec<String>) {
    let t = &config.subtitles.transcription;
    if t.enabled && !config.subtitles.languages.is_empty() && !ffmpeg_info(t).found {
        warnings
            .push("ffmpeg not found: transcription will fail until it is installed".to_string());
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PreviewBody {
    video: Option<String>,
    settings: Option<SubtitlesConfig>,
}

async fn preview_subtitles(
    State(state): State<AppState>,
    request: Result<Json<Value>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let raw = body(request)?;
    // `{video, settings}`, or just the settings like every other group's preview
    let (video, settings) = if raw.get("video").is_some() || raw.get("settings").is_some() {
        let parsed: PreviewBody = serde_json::from_value(raw)
            .map_err(|e| ApiError::BadRequest(format!("invalid preview body: {e}")))?;
        (parsed.video, parsed.settings)
    } else {
        let parsed: SubtitlesConfig = serde_json::from_value(raw)
            .map_err(|e| ApiError::BadRequest(format!("invalid preview body: {e}")))?;
        (None, Some(parsed))
    };
    let stored = state.config.read().await.subtitles.clone();
    let candidate = settings.map(|s| with_stored_key(s, &stored));
    let wanted = candidate.clone().unwrap_or(stored);
    let Json(Preview {
        valid,
        error,
        changes,
        when,
        risks,
        ..
    }) = preview(&state, subtitles_view, live_subtitles(), |c| {
        if let Some(s) = candidate {
            c.subtitles = s;
        }
        Ok(())
    })
    .await;
    let mut out = serde_json::json!({
        "valid": valid,
        "changes": changes,
        "when": when,
        "risks": risks,
    });
    if let Some(error) = error {
        out["error"] = Value::String(error);
    }
    if let Some(video) = video {
        out["files"] = serde_json::json!(file_names(&video, &wanted));
        out["forced_sdh_note"] = Value::String(
            "Forced and SDH files are saved with .forced or .sdh before .srt when a source marks them; \
             they never replace the main file."
                .to_string(),
        );
    }
    Ok(Json(out))
}

#[derive(Serialize)]
struct TestAnswer {
    ok: bool,
    ffmpeg: Ffmpeg,
    server: putio_sync_subtitles::transcribe::ServerTest,
}

/// POST /settings/subtitles/test
///
/// The body is optional: `{"transcription": {...}}` tests unsaved settings, and a masked key
/// stands for the stored one. A 3 second tone is made in process, so this works without ffmpeg.
/// A completed test is always 200; a missing or invalid URL is 400.
async fn test_subtitles(
    State(state): State<AppState>,
    raw: Bytes,
) -> Result<Json<TestAnswer>, ApiError> {
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct TestBody {
        transcription: Option<putio_sync_core::subtitles::TranscriptionConfig>,
    }
    let parsed: TestBody = if raw.iter().all(u8::is_ascii_whitespace) {
        TestBody::default()
    } else {
        serde_json::from_slice(&raw)
            .map_err(|e| ApiError::BadRequest(format!("invalid body: {e}")))?
    };
    let stored = state.config.read().await.subtitles.transcription.clone();
    let mut cfg = parsed.transcription.unwrap_or_else(|| stored.clone());
    if cfg.api_key == MASK {
        cfg.api_key = stored.api_key;
    }
    putio_sync_core::subtitles::check_url(&cfg.url).map_err(super::settings::invalid)?;
    let mut ffmpeg = ffmpeg_info(&cfg);
    if let Some(path) = &ffmpeg.path {
        ffmpeg.version = putio_sync_subtitles::audio::version(Path::new(path)).await;
    }
    let server = putio_sync_subtitles::transcribe::Transcriber::new(&cfg)
        .probe(putio_sync_subtitles::audio::test_tone_wav(3))
        .await;
    Ok(Json(TestAnswer {
        ok: server.ok,
        ffmpeg,
        server,
    }))
}

/// The main file name for each wanted language
fn file_names(video: &str, settings: &SubtitlesConfig) -> Vec<String> {
    use putio_sync_subtitles::naming::{sidecar_name, Slot};
    let path = Path::new(video);
    settings
        .wanted()
        .iter()
        .map(|l| sidecar_name(path, &Slot::main(l)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_video() {
        let s = SubtitlesConfig {
            languages: vec!["en".into(), "es".into()],
            ..Default::default()
        };
        assert_eq!(
            file_names("Movie.2024.1080p.mkv", &s),
            ["Movie.2024.1080p.en.srt", "Movie.2024.1080p.es.srt"]
        );
    }
}
