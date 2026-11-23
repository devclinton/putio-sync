//! Anonymous usage stats routes (feature 058, part 2)
//!
//! - GET /usage-stats - `{enabled, endpoint_set, last_sent, payload}`: the exact payload this copy
//!   would send now, shown before anything is turned on
//! - PUT /usage-stats - `{enabled}`: turn the daily send on or off (saved like any setting)
//! - POST /usage-stats/reset-id - Make a new random install id
//!
//! Nothing is sent unless it is on and `[usage_stats] endpoint` names a receiver. The send runs
//! from [`spawn`], once a day, never from a request.

use super::settings::{body, update};
use crate::error::ApiError;
use crate::AppState;
use axum::{
    extract::{rejection::JsonRejection, State},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use putio_sync_core::config::Config;
use putio_sync_core::usage_stats::{self as stats, Inputs, Schedule};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Create the usage stats router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(show).put(set))
        .route("/reset-id", post(reset_id))
}

/// What GET answers
#[derive(Serialize)]
struct View {
    enabled: bool,
    /// Whether a receiver is configured; without one nothing is sent even when on
    endpoint_set: bool,
    /// When the receiver last accepted a send (RFC 3339), or null
    last_sent: Option<String>,
    /// The exact payload
    payload: stats::Payload,
}

#[derive(Deserialize)]
struct Settings {
    enabled: bool,
}

#[derive(Serialize)]
struct Saved {
    message: String,
    #[serde(flatten)]
    view: View,
}

fn view_of(config: &Config) -> (bool, bool) {
    (
        config.usage_stats.enabled,
        !config.usage_stats.endpoint.trim().is_empty(),
    )
}

fn settings_view(config: &Config) -> serde_json::Value {
    let (enabled, endpoint_set) = view_of(config);
    serde_json::json!({ "enabled": enabled, "endpoint_set": endpoint_set })
}

fn file(state: &AppState) -> Result<PathBuf, ApiError> {
    stats::state_path(state.config_path.as_deref().map(|p| p.as_path()))
        .ok_or_else(|| ApiError::NotFound("no config folder".into()))
}

/// The counts the payload needs, read from the database and the crash folder
pub async fn inputs(state: &AppState) -> Inputs {
    let build = match &state.updates {
        Some(updates) => updates.snapshot().channel.kind(),
        None => putio_sync_update::channel::Channel::detect().kind(),
    }
    .to_string();
    let (mut downloads_7d, mut library_bytes) = (0, 0);
    if let Some(db) = &state.database {
        let since = (Utc::now() - chrono::Duration::days(7))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let repo = db.stats();
        downloads_7d = repo
            .get_for_range(Some(&since), None)
            .await
            .map_or(0, |(n, _, _)| n.max(0) as u64);
        library_bytes = repo.library_bytes().await.map_or(0, |b| b.max(0) as u64);
    }
    let dir = putio_sync_core::crash::dir_for(state.config_path.as_deref().map(|p| p.as_path()));
    let crash_reports = tokio::task::spawn_blocking(move || {
        dir.is_some_and(|d| !putio_sync_core::crash::list(&d).is_empty())
    })
    .await
    .unwrap_or(false);
    Inputs {
        build,
        downloads_7d,
        library_bytes,
        crash_reports,
    }
}

async fn current(state: &AppState) -> Result<View, ApiError> {
    let path = file(state)?;
    let config = state.config.read().await.clone();
    let inputs = inputs(state).await;
    let saved = tokio::task::spawn_blocking(move || stats::load(&path))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let (enabled, endpoint_set) = view_of(&config);
    Ok(View {
        enabled,
        endpoint_set,
        last_sent: saved.last_sent.clone(),
        payload: stats::payload(&config, &inputs, &saved.install_id),
    })
}

async fn show(State(state): State<AppState>) -> Result<Json<View>, ApiError> {
    Ok(Json(current(&state).await?))
}

async fn set(
    State(state): State<AppState>,
    settings: Result<Json<Settings>, JsonRejection>,
) -> Result<Json<Saved>, ApiError> {
    let settings = body(settings)?;
    let changed = update(&state, settings_view, |config| {
        config.usage_stats.enabled = settings.enabled;
        Ok(())
    })
    .await?;
    let message = super::settings::saved_message(
        "Usage stats",
        &changed,
        &["enabled"],
        "Applies within a minute",
        &[],
    );
    Ok(Json(Saved {
        message,
        view: current(&state).await?,
    }))
}

async fn reset_id(State(state): State<AppState>) -> Result<Json<Saved>, ApiError> {
    let path = file(&state)?;
    tokio::task::spawn_blocking(move || stats::reset_id(&path))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(|e| ApiError::Internal(format!("Failed to save the new id: {e}")))?;
    Ok(Json(Saved {
        message: "New install id made.".into(),
        view: current(&state).await?,
    }))
}

/// Check once a minute whether the daily send is due, and send it. A no-op while usage stats
/// are off or have no endpoint.
pub fn spawn(state: &AppState) {
    let state = state.clone();
    tokio::spawn(async move {
        let mut schedule = Schedule::new(stats::random_jitter());
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let config = state.config.read().await.clone();
            let Ok(path) = file(&state) else {
                continue;
            };
            let last = if config.usage_stats.active() {
                let path = path.clone();
                tokio::task::spawn_blocking(move || stats::load(&path).attempted_at())
                    .await
                    .ok()
                    .flatten()
            } else {
                None
            };
            let now = Utc::now();
            if !schedule.due(&config.usage_stats, last, now) {
                continue;
            }
            let inputs = inputs(&state).await;
            // Whatever happens, the next try is tomorrow plus a new random delay
            let _ = stats::send_now(&path, &config, &inputs, now, stats::default_reach()).await;
            schedule = Schedule::new(stats::random_jitter());
            // `due` arms on the next tick; `last_attempt` (saved first) holds the day
        }
    });
}
