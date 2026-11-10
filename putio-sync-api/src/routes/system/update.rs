//! Update checker routes (feature 055)
//!
//! - GET /update - The running version, how it was installed, the last check and any
//!   newer release with its notes
//! - POST /update/check - Check now; `409 checked_recently` within a minute of the last
//! - POST /update/dismiss - `{"version": "0.4.0"}` hides that notice; `{"rolled_back": true}`
//!   hides the rollback message

use crate::changelog;
use crate::error::ApiError;
use axum::{
    extract::{rejection::JsonRejection, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use putio_sync_update::{CheckNowError, Snapshot, UpdateService};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub use crate::AppState;

/// Create the update router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_update))
        .route("/check", post(check))
        .route("/dismiss", post(dismiss))
}

#[derive(Serialize)]
struct UpdateView {
    current: String,
    channel: ChannelView,
    check: CheckView,
    available: Option<AvailableView>,
    auto_install: bool,
    rolled_back: Option<RolledBackView>,
}

#[derive(Serialize)]
struct ChannelView {
    kind: &'static str,
    label: &'static str,
    mode: putio_sync_update::channel::UpdateMode,
}

#[derive(Serialize)]
struct CheckView {
    enabled: bool,
    last_attempt_at: Option<String>,
    last_success_at: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct AvailableView {
    version: String,
    date: Option<String>,
    url: String,
    dismissed: bool,
    /// Every newer release, newest first, shaped like `GET /whats-new`'s
    releases: Vec<ReleaseView>,
}

#[derive(Serialize)]
struct ReleaseView {
    name: String,
    date: Option<String>,
    url: String,
    kinds: Vec<KindView>,
}

#[derive(Serialize)]
struct KindView {
    name: String,
    entries: Vec<EntryView>,
}

#[derive(Serialize)]
struct EntryView {
    id: String,
    text: String,
    new: bool,
}

#[derive(Serialize)]
struct RolledBackView {
    version: String,
    reason: String,
}

#[derive(Deserialize)]
struct DismissRequest {
    version: Option<String>,
    #[serde(default)]
    rolled_back: bool,
}

fn view(snapshot: Snapshot) -> UpdateView {
    let available = snapshot.latest().map(|latest| AvailableView {
        version: latest.version.to_string(),
        date: latest.published_at.map(|t| t.date_naive().to_string()),
        url: latest.html_url.clone(),
        dismissed: snapshot.dismissed(),
        releases: snapshot
            .releases
            .iter()
            .map(|r| {
                let date = r.published_at.map(|t| t.date_naive().to_string());
                let parsed =
                    changelog::parse_release(&r.version.to_string(), date.as_deref(), &r.body);
                ReleaseView {
                    name: parsed.name,
                    date: parsed.date,
                    url: r.html_url.clone(),
                    kinds: parsed
                        .kinds
                        .into_iter()
                        .map(|k| KindView {
                            name: k.name,
                            entries: k
                                .entries
                                .into_iter()
                                .map(|e| EntryView {
                                    id: e.id,
                                    text: e.text,
                                    new: false,
                                })
                                .collect(),
                        })
                        .collect(),
                }
            })
            .collect(),
    });
    UpdateView {
        current: snapshot.current.to_string(),
        channel: ChannelView {
            kind: snapshot.channel.kind(),
            label: snapshot.channel.label(),
            mode: snapshot.channel.mode(),
        },
        check: CheckView {
            enabled: snapshot.settings.check,
            last_attempt_at: snapshot.last_attempt_at.map(|t| t.to_rfc3339()),
            last_success_at: snapshot.last_success_at.map(|t| t.to_rfc3339()),
            error: snapshot.error.clone(),
        },
        available,
        auto_install: snapshot.settings.auto_install,
        rolled_back: snapshot
            .rolled_back
            .map(|(version, reason)| RolledBackView { version, reason }),
    }
}

fn service(state: &AppState) -> Result<&UpdateService, ApiError> {
    state
        .updates
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable("the update checker runs in the daemon".into()))
}

/// GET /update
async fn get_update(State(state): State<AppState>) -> Result<Json<UpdateView>, ApiError> {
    Ok(Json(view(service(&state)?.snapshot())))
}

/// POST /update/check
async fn check(State(state): State<AppState>) -> Result<Response, ApiError> {
    match service(&state)?.check_now().await {
        Ok(snapshot) => Ok(Json(view(snapshot)).into_response()),
        Err(CheckNowError::CheckedRecently) => Ok((
            StatusCode::CONFLICT,
            Json(json!({ "error": "checked_recently" })),
        )
            .into_response()),
    }
}

/// POST /update/dismiss
async fn dismiss(
    State(state): State<AppState>,
    request: Result<Json<DismissRequest>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(request) = request.map_err(|e| ApiError::BadRequest(e.body_text()))?;
    let updates = service(&state)?;
    if request.rolled_back {
        updates.clear_rollback().await;
    }
    match request.version {
        Some(version) => {
            let version: putio_sync_update::version::Version =
                version
                    .parse()
                    .map_err(|e: putio_sync_update::version::ParseVersionError| {
                        ApiError::BadRequest(e.to_string())
                    })?;
            updates.dismiss(&version.to_string()).await;
        }
        None if !request.rolled_back => {
            return Err(ApiError::BadRequest(
                "give a version or rolled_back: true".into(),
            ))
        }
        None => {}
    }
    Ok(StatusCode::NO_CONTENT)
}
