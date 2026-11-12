//! Update checker routes (feature 055)
//!
//! - GET /update - The running version, how it was installed, the last check and any
//!   newer release with its notes
//! - POST /update/check - Check now; `409 checked_recently` within a minute of the last
//! - POST /update/apply - "Update now": `{"interrupt": false}`; `202` with the GET body, then
//!   poll GET for `attempt`
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
use putio_sync_update::channel::{deb_arch, Channel, Instruction, UpdateMode};
use putio_sync_update::{ApplyError, Attempt, CheckNowError, Snapshot, UpdateService};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub use crate::AppState;

/// Create the update router
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_update))
        .route("/check", post(check))
        .route("/apply", post(apply))
        .route("/dismiss", post(dismiss))
}

#[derive(Serialize)]
struct UpdateView {
    current: String,
    channel: ChannelView,
    check: CheckView,
    available: Option<AvailableView>,
    auto_install: bool,
    busy: BusyView,
    attempt: Option<Attempt>,
    rolled_back: Option<RolledBackView>,
}

#[derive(Serialize)]
struct BusyView {
    active_transfers: usize,
}

#[derive(Serialize)]
struct ChannelView {
    kind: &'static str,
    label: String,
    mode: UpdateMode,
    /// Container only
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    variant: Option<String>,
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
    /// How to update this channel to `version`
    instruction: Instruction,
    /// Mode `swap`: a patch from the running version exists
    patch: bool,
    /// What "Update now" downloads (the patch, the full binary or the installer)
    download_bytes: Option<u64>,
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

#[derive(Deserialize, Default)]
struct ApplyRequest {
    #[serde(default)]
    interrupt: bool,
}

#[derive(Deserialize)]
struct DismissRequest {
    version: Option<String>,
    #[serde(default)]
    rolled_back: bool,
}

fn view(snapshot: Snapshot, busy: usize) -> UpdateView {
    let plan = match snapshot.channel.mode() {
        UpdateMode::Swap => snapshot.plan(),
        _ => None,
    };
    let installer_bytes = snapshot.latest().and_then(|latest| {
        let name = putio_sync_update::installer::asset_name(&snapshot.channel, latest.version)?;
        Some(latest.asset(&name)?.size)
    });
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
        instruction: snapshot.channel.instruction(&latest.version, deb_arch()),
        patch: plan
            .as_ref()
            .is_some_and(|p| p.kind == putio_sync_update::fetch::Kind::Patch),
        download_bytes: plan.as_ref().map(|p| p.asset.size).or(installer_bytes),
    });
    let (image, variant) = match &snapshot.channel {
        Channel::Container { image, variant } => (Some(image.clone()), variant.clone()),
        _ => (None, None),
    };
    UpdateView {
        current: snapshot.current.to_string(),
        channel: ChannelView {
            kind: snapshot.channel.kind(),
            label: snapshot.channel.label(),
            mode: snapshot.channel.mode(),
            image,
            variant,
        },
        check: CheckView {
            enabled: snapshot.settings.check,
            last_attempt_at: snapshot.last_attempt_at.map(|t| t.to_rfc3339()),
            last_success_at: snapshot.last_success_at.map(|t| t.to_rfc3339()),
            error: snapshot.error.clone(),
        },
        available,
        auto_install: snapshot.settings.auto_install,
        busy: BusyView {
            active_transfers: busy,
        },
        attempt: snapshot.attempt.clone(),
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
    let updates = service(&state)?;
    let busy = updates.busy().await;
    Ok(Json(view(updates.snapshot(), busy)))
}

/// POST /update/apply
async fn apply(
    State(state): State<AppState>,
    request: Result<Json<ApplyRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    // An empty body is `{"interrupt": false}`
    let request = match request {
        Ok(Json(request)) => request,
        Err(JsonRejection::MissingJsonContentType(_)) => ApplyRequest::default(),
        Err(e) => return Err(ApiError::BadRequest(e.body_text())),
    };
    let updates = service(&state)?;
    let refused = |status, body| Ok((status, Json(body)).into_response());
    match updates.apply(request.interrupt).await {
        Ok(snapshot) => {
            let busy = updates.busy().await;
            Ok((StatusCode::ACCEPTED, Json(view(snapshot, busy))).into_response())
        }
        Err(ApplyError::Busy(n)) => refused(
            StatusCode::CONFLICT,
            json!({ "error": "busy", "active_transfers": n }),
        ),
        Err(ApplyError::NotSupported(mode)) => refused(
            StatusCode::CONFLICT,
            json!({ "error": "not_supported", "mode": mode }),
        ),
        Err(ApplyError::InProgress) => {
            refused(StatusCode::CONFLICT, json!({ "error": "in_progress" }))
        }
        Err(ApplyError::NotWritable(path)) => refused(
            StatusCode::CONFLICT,
            json!({ "error": "not_writable", "path": path }),
        ),
        Err(ApplyError::NoUpdate) => {
            refused(StatusCode::NOT_FOUND, json!({ "error": "no_update" }))
        }
    }
}

/// POST /update/check
async fn check(State(state): State<AppState>) -> Result<Response, ApiError> {
    let updates = service(&state)?;
    match updates.check_now().await {
        Ok(snapshot) => Ok(Json(view(snapshot, updates.busy().await)).into_response()),
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
