//! DLNA media server settings and status (feature 022)
//!
//! - GET  /settings/media-server - The saved `[dlna]` settings and the folders that can be shared
//! - PUT  /settings/media-server - Save them and apply them to the running server at once
//! - POST /settings/media-server/preview - What a PUT would do
//! - GET  /media-server/status - State, addresses, streams playing and players seen

use super::settings::{body, preview, saved_message, update};
use crate::error::ApiError;
use crate::models::SuccessResponse;
use crate::AppState;
use axum::{
    extract::{rejection::JsonRejection, State},
    routing::{get, post},
    Json, Router,
};
use putio_sync_core::config::{Config, DlnaConfig};
use putio_sync_core::listen::BindList;
use putio_sync_dlna::PutioHealth;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// `[dlna]` as Settings edits it; a PUT leaves out `available_roots`
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaServerSettings {
    pub enabled: bool,
    pub name: String,
    pub port: u16,
    pub bind: Vec<String>,
    pub roots: Vec<String>,
    pub putio_folder: Option<i64>,
}

impl Default for MediaServerSettings {
    fn default() -> Self {
        media_server_view(&Config::default())
    }
}

/// One folder that can be shared
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AvailableRoot {
    pub key: String,
    pub title: String,
    pub path: Option<String>,
}

/// GET /settings/media-server
#[derive(Debug, Serialize)]
pub struct MediaServerView {
    #[serde(flatten)]
    pub settings: MediaServerSettings,
    pub available_roots: Vec<AvailableRoot>,
}

/// GET /media-server/status
#[derive(Debug, Serialize)]
pub struct MediaServerStatusBody {
    pub state: String,
    pub reason: Option<String>,
    pub addresses: Vec<String>,
    pub streams_active: usize,
    pub players_last_hour: usize,
    pub putio: PutioStatus,
}

#[derive(Debug, Serialize)]
pub struct PutioStatus {
    pub state: String,
    pub reason: Option<String>,
}

/// The settings routes, merged into `/settings`
pub(crate) fn settings_router() -> Router<AppState> {
    Router::new()
        .route("/media-server", get(get_settings).put(update_settings))
        .route("/media-server/preview", post(preview_settings))
}

/// The status route, nested at `/media-server`
pub fn router() -> Router<AppState> {
    Router::new().route("/status", get(get_status))
}

fn media_server_view(config: &Config) -> MediaServerSettings {
    let d = &config.dlna;
    MediaServerSettings {
        enabled: d.enabled,
        name: d.name.clone(),
        port: d.port,
        bind: d.bind.entries(),
        roots: d.roots.clone(),
        putio_folder: d.putio_folder,
    }
}

fn apply(settings: MediaServerSettings, config: &mut Config) -> Result<(), ApiError> {
    let bind: Vec<String> = settings
        .bind
        .into_iter()
        .map(|b| b.trim().to_string())
        .filter(|b| !b.is_empty())
        .collect();
    config.dlna = DlnaConfig {
        enabled: settings.enabled,
        name: settings.name,
        port: settings.port,
        bind: BindList::from_entries(bind),
        roots: settings.roots,
        putio_folder: settings.putio_folder,
    };
    Ok(())
}

/// Every field applies at once: the running server swaps its roots and name, and
/// rebinds when the port or addresses change
fn live() -> (&'static [&'static str], &'static str) {
    (
        &["enabled", "name", "port", "bind", "roots", "putio_folder"],
        "Applies now",
    )
}

/// The folders that can be shared: Downloads, each category folder (the *arr categories
/// and folders on disk), each watch folder while watching is on, and Put.io
pub fn available_roots(config: &Config) -> Vec<AvailableRoot> {
    let download = &config.download.target_directory;
    let mut out = vec![AvailableRoot {
        key: "downloads".into(),
        title: "Downloads".into(),
        path: Some(download.to_string_lossy().into_owned()),
    }];
    let mut names: BTreeSet<String> = config
        .arr
        .servers
        .iter()
        .map(|s| {
            s.category
                .clone()
                .unwrap_or_else(|| s.kind.as_str().to_string())
        })
        .collect();
    if let Ok(dir) = std::fs::read_dir(download) {
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') && entry.path().is_dir() {
                names.insert(name);
            }
        }
    }
    // Folders already shared stay listed, so unticking one is always possible
    names.extend(
        config
            .dlna
            .roots
            .iter()
            .filter_map(|r| r.strip_prefix("cat:").map(String::from)),
    );
    for name in names {
        out.push(AvailableRoot {
            key: format!("cat:{name}"),
            title: name.clone(),
            path: Some(download.join(&name).to_string_lossy().into_owned()),
        });
    }
    if config.watch.enabled {
        for path in &config.watch.paths {
            let text = path.to_string_lossy().into_owned();
            out.push(AvailableRoot {
                key: format!("watch:{text}"),
                title: text.clone(),
                path: Some(text),
            });
        }
    }
    out.push(AvailableRoot {
        key: "putio".into(),
        title: "Put.io".into(),
        path: None,
    });
    out
}

/// GET /settings/media-server
async fn get_settings(State(state): State<AppState>) -> Result<Json<MediaServerView>, ApiError> {
    let config = state.config.read().await;
    Ok(Json(MediaServerView {
        settings: media_server_view(&config),
        available_roots: available_roots(&config),
    }))
}

/// PUT /settings/media-server
async fn update_settings(
    State(state): State<AppState>,
    settings: Result<Json<MediaServerSettings>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    let changed = update(&state, media_server_view, |config| apply(settings, config)).await?;
    let mut notes = Vec::new();
    match &state.media_server {
        Some(server) => {
            let config = state.config.read().await.clone();
            server.update(&config).await;
        }
        None => notes.push(
            "The media server runs under `daemon`; this engine only saves the settings."
                .to_string(),
        ),
    }
    let (fields, when) = live();
    Ok(Json(SuccessResponse {
        message: saved_message("Media server", &changed, fields, when, &notes),
    }))
}

/// POST /settings/media-server/preview
async fn preview_settings(
    State(state): State<AppState>,
    settings: Result<Json<MediaServerSettings>, JsonRejection>,
) -> Result<Json<super::assistant::Preview>, ApiError> {
    let settings = body(settings)?;
    Ok(preview(&state, media_server_view, live(), |c| apply(settings, c)).await)
}

/// GET /media-server/status
async fn get_status(State(state): State<AppState>) -> Json<MediaServerStatusBody> {
    let Some(server) = &state.media_server else {
        return Json(MediaServerStatusBody {
            state: "off".into(),
            reason: None,
            addresses: Vec::new(),
            streams_active: 0,
            players_last_hour: 0,
            putio: PutioStatus {
                state: "off".into(),
                reason: None,
            },
        });
    };
    let s = server.status();
    let (putio_state, putio_reason) = match s.putio {
        PutioHealth::Off => ("off", None),
        PutioHealth::Ok => ("ok", None),
        PutioHealth::Error(e) => ("error", Some(e)),
    };
    Json(MediaServerStatusBody {
        state: s.state.as_str().to_string(),
        reason: s.reason,
        addresses: s.addresses,
        streams_active: s.streams_active,
        players_last_hour: s.players_last_hour,
        putio: PutioStatus {
            state: putio_state.into(),
            reason: putio_reason,
        },
    })
}
