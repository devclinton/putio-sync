//! Setup-mode handlers (feature 013, `specs/013-setup-wizard/contracts/setup-api.md`).
//!
//! The Put.io token is accepted but never returned; the API token is returned once, by
//! `finish`; the setup code is never returned.

use crate::error::ApiError;
use crate::routes::arr::{browser_host, generate_password, target_of, SetupRequest, SetupTarget};
use crate::setup::{new_api_token, AccountSummary, LinkState, SetupState};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use putio_sync_arr::discovery::{self, Limits};
use putio_sync_arr::setup::{self as arr_setup, Outcome, SetupInput};
use putio_sync_client::oob::{DeviceLink, OobCheck, LINK_URL};
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::{ArrConfig, ArrServer, ArrSettings, Config};
use putio_sync_core::setup::{
    check_folder, check_preset, in_container, preset_choices, resolve_app_id, suggest_download_dir,
    suggest_watch_dir, FolderCheck, FolderChoice,
};
use putio_sync_download::{dry_run as run_dry_run, DRY_RUN_LIMIT};
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// How long a device-link code stays valid
const LINK_EXPIRY: Duration = Duration::from_secs(10 * 60);

/// GET /setup/status (public)
pub async fn status(State(state): State<SetupState>) -> Json<Value> {
    let session = state.inner.session.lock().await;
    Json(json!({
        "setup_required": !session.finished,
        "reasons": session.reasons,
        "api_token_configured": state.api_token_configured(),
    }))
}

/// POST /setup/unlock: the middleware has checked the code
pub async fn unlock() -> StatusCode {
    StatusCode::NO_CONTENT
}

/// GET /setup/session: everything the wizard has chosen, without secrets
pub async fn session(State(state): State<SetupState>) -> Json<Value> {
    let session = state.inner.session.lock().await;
    let base = &session.base;
    let choices = &session.choices;
    let presets: Vec<Value> = preset_choices(&base.download)
        .into_iter()
        .map(|(name, label)| json!({ "name": name, "label": label }))
        .collect();
    let arr_enabled = session.arr.as_ref().is_some_and(|a| a.enabled);
    Json(json!({
        "reasons": session.reasons,
        "account": session.account,
        "download_dir": choices.download_dir,
        "preset": choices.preset.clone().or_else(|| base.download.preset.clone()),
        "folder_rules": choices.folders,
        "watch_dir": choices.watch_dir.clone().unwrap_or_else(|| {
            base.watch.enabled.then(|| base.watch.paths.first().cloned()).flatten()
        }),
        "metrics": choices.metrics.unwrap_or(base.metrics.enabled),
        "metrics_bind": choices.metrics_bind.clone().unwrap_or_else(|| base.metrics.bind.clone()),
        "arr_enabled": arr_enabled,
        "app_id_available": resolve_app_id(base).is_some(),
        "in_container": in_container(),
        "suggestions": {
            "download_dir": suggest_download_dir(Some(base)),
            "watch_dir": suggest_watch_dir(Some(base)),
        },
        "presets": presets,
    }))
}

/// POST /setup/putio/link: ask Put.io for a device-link code
pub async fn link_start(State(state): State<SetupState>) -> Result<Json<Value>, ApiError> {
    let app_id = {
        let session = state.inner.session.lock().await;
        resolve_app_id(&session.base)
    };
    let Some(app_id) = app_id else {
        return Err(ApiError::Conflict(
            "No Put.io app ID is set, so paste a token instead".into(),
        ));
    };
    let code = DeviceLink::with_base_url(&state.inner.putio_base_url)
        .request_code(&app_id)
        .await
        .map_err(|e| ApiError::ServiceUnavailable(e.to_string()))?;
    let body = json!({
        "code": code.code,
        "qr_code_url": code.qr_code_url,
        "link_url": LINK_URL,
    });
    state.inner.session.lock().await.link = Some(LinkState {
        code: code.code,
        created_at: Instant::now(),
    });
    Ok(Json(body))
}

/// GET /setup/putio/link: whether the user has entered the code yet
pub async fn link_check(State(state): State<SetupState>) -> Result<Json<Value>, ApiError> {
    let link = {
        let session = state.inner.session.lock().await;
        if let (Some(account), Some(_)) = (&session.account, &session.choices.putio_token) {
            return Ok(Json(json!({ "state": "linked", "account": account })));
        }
        session.link.clone()
    };
    let Some(link) = link else {
        return Err(ApiError::BadRequest("No link code was requested".into()));
    };
    if link.created_at.elapsed() > LINK_EXPIRY {
        return Ok(Json(json!({ "state": "expired" })));
    }
    let checked = DeviceLink::with_base_url(&state.inner.putio_base_url)
        .check_code(&link.code)
        .await;
    match checked {
        Ok(OobCheck::Pending) => Ok(Json(json!({ "state": "pending" }))),
        Ok(OobCheck::Linked(token)) => {
            let account = connect(&state, token).await?;
            info!(username = %account.username, "Put.io linked");
            Ok(Json(json!({ "state": "linked", "account": account })))
        }
        Err(putio_sync_client::ClientError::AuthError(_)) => {
            Ok(Json(json!({ "state": "expired" })))
        }
        Err(e) => Err(ApiError::ServiceUnavailable(e.to_string())),
    }
}

/// Body of `POST /setup/putio/token`. Not `Debug`, so it can't be logged.
#[derive(Deserialize)]
pub struct TokenBody {
    token: String,
}

/// POST /setup/putio/token: a pasted token
pub async fn putio_token(
    State(state): State<SetupState>,
    Json(body): Json<TokenBody>,
) -> Result<Json<Value>, ApiError> {
    let token = body.token.trim().to_string();
    if token.is_empty() {
        return Err(ApiError::BadRequest("Paste the Put.io token".into()));
    }
    let account = connect(&state, token).await?;
    info!(username = %account.username, "Put.io token accepted");
    Ok(Json(json!({ "account": account })))
}

/// Check `token` against Put.io and keep it with the account. A token already kept is
/// replaced only by one that works.
async fn connect(state: &SetupState, token: String) -> Result<AccountSummary, ApiError> {
    let client = PutioApiClient::with_base_url(token.clone(), state.inner.putio_base_url.clone());
    let probe = client.probe_account().await;
    if probe.unauthorized() {
        return Err(ApiError::BadRequest("Put.io refused the token".into()));
    }
    let info = probe
        .result
        .map_err(|e| ApiError::ServiceUnavailable(format!("Put.io could not be reached: {e}")))?;
    let account = AccountSummary {
        username: info.username,
        used: u64::try_from(info.disk.used).unwrap_or(0),
        size: u64::try_from(info.disk.size).unwrap_or(0),
    };
    let mut session = state.inner.session.lock().await;
    session.choices.putio_token = Some(token);
    session.account = Some(account.clone());
    session.link = None;
    Ok(account)
}

/// The connected Put.io client, or 400
async fn putio_client(state: &SetupState) -> Result<PutioApiClient, ApiError> {
    let token = state.inner.session.lock().await.choices.putio_token.clone();
    let token = token.ok_or_else(|| ApiError::BadRequest("Connect Put.io first".into()))?;
    Ok(PutioApiClient::with_base_url(
        token,
        state.inner.putio_base_url.clone(),
    ))
}

/// GET /setup/putio/folders: top-level folders, for folder rules
pub async fn putio_folders(State(state): State<SetupState>) -> Result<Json<Value>, ApiError> {
    let client = putio_client(&state).await?;
    let listing = client
        .list_files(0)
        .await
        .map_err(|e| ApiError::ServiceUnavailable(e.to_string()))?;
    let folders: Vec<Value> = listing
        .files
        .into_iter()
        .filter(|f| f.file_type == "FOLDER")
        .map(|f| json!({ "name": f.name, "path": format!("/{}", f.name), "size": f.size }))
        .collect();
    Ok(Json(json!(folders)))
}

/// Body of the folder checks
#[derive(Debug, Deserialize)]
pub struct FolderBody {
    path: String,
    #[serde(default)]
    create: bool,
}

fn absolute(path: &str) -> PathBuf {
    PathBuf::from(path.trim())
}

/// POST /setup/folder/check
pub async fn folder_check(
    State(state): State<SetupState>,
    Json(body): Json<FolderBody>,
) -> Json<FolderCheck> {
    let used = state
        .inner
        .session
        .lock()
        .await
        .account
        .as_ref()
        .map(|a| a.used);
    Json(check_folder(&absolute(&body.path), used, body.create))
}

/// PUT /setup/folder: keep the folder when it is usable
pub async fn folder_set(State(state): State<SetupState>, Json(body): Json<FolderBody>) -> Response {
    let path = absolute(&body.path);
    let mut session = state.inner.session.lock().await;
    let used = session.account.as_ref().map(|a| a.used);
    let check = check_folder(&path, used, body.create);
    if !check.usable() {
        return (StatusCode::BAD_REQUEST, Json(json!(check))).into_response();
    }
    session.choices.download_dir = Some(path);
    Json(json!(check)).into_response()
}

/// Body of `PUT /setup/download`
#[derive(Debug, Deserialize)]
pub struct DownloadBody {
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    folder_rules: Vec<FolderChoice>,
}

/// PUT /setup/download: the account-wide preset and top-level folder rules
pub async fn download_set(
    State(state): State<SetupState>,
    Json(body): Json<DownloadBody>,
) -> Result<StatusCode, ApiError> {
    let mut session = state.inner.session.lock().await;
    let preset = body.preset.filter(|p| !p.trim().is_empty());
    if let Some(name) = &preset {
        check_preset(&session.base.download, name)
            .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    }
    let mut choices = session.choices.clone();
    choices.preset = preset;
    choices.folders = body.folder_rules;
    validate(&session.base, &choices, session.arr.as_ref())?;
    session.choices = choices;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /setup/watch/check
pub async fn watch_check(Json(body): Json<FolderBody>) -> Json<FolderCheck> {
    Json(check_folder(&absolute(&body.path), None, body.create))
}

/// `Some(None)` for an explicit `null`, `None` when the field is absent
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

/// Body of `PUT /setup/extras`
#[derive(Debug, Deserialize)]
pub struct ExtrasBody {
    #[serde(default, deserialize_with = "present")]
    watch_dir: Option<Option<String>>,
    #[serde(default)]
    metrics: Option<bool>,
    #[serde(default)]
    metrics_bind: Option<String>,
}

/// PUT /setup/extras: watch folder and metrics
pub async fn extras_set(
    State(state): State<SetupState>,
    Json(body): Json<ExtrasBody>,
) -> Result<StatusCode, ApiError> {
    let mut session = state.inner.session.lock().await;
    let mut choices = session.choices.clone();
    if let Some(watch) = body.watch_dir {
        let dir = watch.filter(|d| !d.trim().is_empty()).map(|d| absolute(&d));
        if let Some(dir) = &dir {
            if !check_folder(dir, None, false).usable() {
                return Err(ApiError::BadRequest(
                    "The watch folder can't be used".into(),
                ));
            }
        }
        choices.watch_dir = Some(dir);
    }
    if body.metrics.is_some() {
        choices.metrics = body.metrics;
    }
    if let Some(bind) = body.metrics_bind.filter(|b| !b.trim().is_empty()) {
        choices.metrics_bind = Some(bind.trim().to_string());
    }
    validate(&session.base, &choices, session.arr.as_ref())?;
    session.choices = choices;
    Ok(StatusCode::NO_CONTENT)
}

/// The config `choices` would save, or 400 with why it is invalid
fn validate(
    base: &Config,
    choices: &putio_sync_core::setup::SetupChoices,
    arr: Option<&ArrConfig>,
) -> Result<Config, ApiError> {
    let mut choices = choices.clone();
    if let Some(arr) = arr {
        choices.arr = Some(arr.clone());
    }
    let mut config = base.clone();
    choices
        .apply(&mut config)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(config)
}

/// The `[arr]` draft and what it resolves to
async fn arr_draft(state: &SetupState) -> Result<(Config, ArrSettings), ApiError> {
    let session = state.inner.session.lock().await;
    let mut config = session.base.clone();
    if let Some(arr) = &session.arr {
        config.arr = arr.clone();
    }
    let settings = config
        .resolve_arr()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok((config, settings))
}

/// POST /setup/arr/discover: as `POST /arr/discover` (018)
pub async fn arr_discover(
    State(state): State<SetupState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let (_, settings) = arr_draft(&state).await?;
    let saved: Vec<String> = settings.servers.iter().map(|s| s.url.clone()).collect();
    let gateway = arr_setup::SystemHosts::detect().gateway;
    let candidates = discovery::candidates(
        &saved,
        gateway.as_deref(),
        browser_host(&headers).as_deref(),
    );
    let found = discovery::discover(
        &state.inner.arr_discovery,
        candidates,
        state.inner.http.clone(),
        Limits::default(),
    )
    .await
    .map_err(|_| ApiError::Conflict("Discovery already running".into()))?;
    Ok(Json(json!(found)))
}

fn arr_input(
    config: &Config,
    headers: &HeaderMap,
    req: &SetupRequest,
    settings: &ArrSettings,
) -> Result<SetupInput, ApiError> {
    let (url, api_key) = target_of(req, settings)?;
    Ok(SetupInput {
        url,
        api_key,
        category: req.category.clone().filter(|c| !c.trim().is_empty()),
        hosts: arr_setup::candidate_hosts(browser_host(headers).as_deref(), req.host.as_deref()),
        port: config
            .resolve_api()
            .map(|r| r.value.bind.port())
            .unwrap_or(15432),
        username: settings.username.clone(),
        disable_blackhole: req.disable_blackhole,
        password: settings.password.clone().unwrap_or_default(),
    })
}

/// POST /setup/arr/preview: as `POST /arr/setup/preview` (018); changes nothing
pub async fn arr_preview(
    State(state): State<SetupState>,
    headers: HeaderMap,
    Json(req): Json<SetupRequest>,
) -> Result<Json<Value>, ApiError> {
    let (config, settings) = arr_draft(&state).await?;
    let input = arr_input(&config, &headers, &req, &settings)?;
    Ok(Json(
        match arr_setup::preview(&input, state.inner.http.clone()).await {
            Ok(mut plan) => {
                plan.download_client.will_enable = !settings.enabled;
                plan.download_client.will_generate_password = settings.password.is_none();
                let mut body = json!(plan);
                body["ok"] = json!(true);
                body
            }
            Err(f) => json!({ "ok": false, "reason": f.reason, "detail": f.detail }),
        },
    ))
}

/// POST /setup/arr/run: as `POST /arr/setup` (018). Radarr/Sonarr change at once; the
/// download client settings are saved with the rest at finish.
pub async fn arr_run(
    State(state): State<SetupState>,
    headers: HeaderMap,
    Json(req): Json<SetupRequest>,
) -> Result<Json<Value>, ApiError> {
    let (mut config, before) = arr_draft(&state).await?;
    let (_, key) = target_of(&req, &before)?;
    // The download client is on, with a password, before Radarr/Sonarr is pointed at it
    config.arr.enabled = true;
    if !ArrSettings::password_from_env() && config.arr.password.as_deref().is_none_or(str::is_empty)
    {
        config.arr.password = Some(generate_password());
    }
    let settings = config
        .resolve_arr()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let input = arr_input(&config, &headers, &req, &settings)?;
    let mut result = arr_setup::run(&input, state.inner.http.clone()).await;
    if result.outcome != Outcome::Failed {
        if req.remember && matches!(req.target, SetupTarget::Url { .. }) {
            if let (Some(url), Some(kind)) = (result.url.clone(), result.kind) {
                let normal = |u: &str| u.trim_end_matches('/').to_ascii_lowercase();
                match config
                    .arr
                    .servers
                    .iter_mut()
                    .find(|s| normal(&s.url) == normal(&url))
                {
                    Some(server) => {
                        server.kind = kind;
                        server.api_key = Some(key.clone());
                    }
                    None => config.arr.servers.push(ArrServer {
                        kind,
                        name: None,
                        url: url.trim_end_matches('/').to_string(),
                        api_key: Some(key.clone()),
                        category: None,
                    }),
                }
                result.remembered = true;
            }
        }
        state.inner.session.lock().await.arr = Some(config.arr);
    } else {
        warn!("Radarr/Sonarr auto-setup failed during setup");
    }
    Ok(Json(json!(result)))
}

/// POST /setup/dry-run: what the first sync would download
pub async fn dry_run(State(state): State<SetupState>) -> Result<Json<Value>, ApiError> {
    let config = {
        let session = state.inner.session.lock().await;
        if session.choices.download_dir.is_none() {
            return Err(ApiError::BadRequest(
                "Choose the download folder first".into(),
            ));
        }
        validate(&session.base, &session.choices, session.arr.as_ref())?
    };
    let client = putio_client(&state).await?;
    let download = crate::extract_download_config(&config);
    let report = run_dry_run(&client, &download, DRY_RUN_LIMIT)
        .await
        .map_err(|e| ApiError::ServiceUnavailable(format!("The dry run failed: {e}")))?;
    Ok(Json(json!(report)))
}

/// POST /setup/finish: save the config once and end setup mode
pub async fn finish(State(state): State<SetupState>) -> Result<Response, ApiError> {
    let mut session = state.inner.session.lock().await;
    if session.finished {
        return Err(ApiError::Conflict("Setup is already finished".into()));
    }
    let missing = session.choices.missing();
    if !missing.is_empty() {
        let names: Vec<&str> = missing
            .iter()
            .map(|m| if *m == "putio" { "account" } else { m })
            .collect();
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Still to choose: {}", names.join(", ")),
                "missing": names,
            })),
        )
            .into_response());
    }
    let mut choices = session.choices.clone();
    let created = (!state.api_token_configured()).then(new_api_token);
    choices.api_token = created.clone();
    let config = validate(&session.base, &choices, session.arr.as_ref())?;
    if let Some(dir) = &choices.download_dir {
        let check = check_folder(dir, None, true);
        if !check.usable() || !Path::new(dir).is_dir() {
            return Err(ApiError::BadRequest(format!(
                "The download folder {} can't be used: {}",
                dir.display(),
                check.error.unwrap_or_else(|| "not writable".into())
            )));
        }
    }
    config
        .save_setup(&state.inner.config_path, created.is_some())
        .map_err(|e| {
            ApiError::InternalError("Could not write the config file".into())
                .with_details(e.to_string())
        })?;
    session.finished = true;
    drop(session);
    info!(
        path = %state.inner.config_path.display(),
        api_token_created = created.is_some(),
        "Setup finished; config saved"
    );
    state.mark_finished();
    Ok(Json(match created {
        Some(token) => json!({ "api_token": token }),
        None => json!({}),
    })
    .into_response())
}
