//! Radarr/Sonarr download client (features 011 and 018)
//!
//! - POST /transmission/rpc            - Transmission RPC, own Basic auth (public route)
//! - GET  /arr                         - Settings card data (protected)
//! - PUT  /arr/client                  - Download client form (protected)
//! - POST /arr/applications/test       - Test unsaved application form values (protected)
//! - POST /arr/applications            - Add an application (protected)
//! - PUT  /arr/applications/{index}    - Edit an application (protected)
//! - DELETE /arr/applications/{index}  - Remove an application from the config (protected)
//! - POST /arr/applications/{index}/test - Test a saved application (protected)
//! - POST /arr/discover                - Find Radarr/Sonarr without a key (protected)
//! - POST /arr/setup/preview           - What auto-setup would change (protected)
//! - POST /arr/setup                   - Auto-setup (protected)
//! - POST /arr/setup/check             - Folder mapping and recommendations (049, protected)
//! - POST /arr/setup/mapping           - Make the proposed mapping (049, protected)
//! - POST /arr/setup/recommendation    - Turn on one recommended setting (049, protected)

use crate::error::ApiError;
use crate::middleware::auth::token_matches;
use crate::middleware::ratelimit::client_ip;
use crate::routes::system::tailscale::tailnet_first_hosts;
use crate::AppState;
use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use base64::Engine;
use putio_sync_arr::clients::{self, ClientInfo};
use putio_sync_arr::discovery::{self, Limits};
use putio_sync_arr::path_check::{self, PathCheck};
use putio_sync_arr::path_mapping::{self, FolderAnalysis, Marker, Proposal, ProposalAction};
use putio_sync_arr::recommend;
use putio_sync_arr::setup::{self, Outcome, SetupInput};
use putio_sync_arr::{rpc, ArrApi, ArrService};
use putio_sync_core::config::{ArrConfig, ArrKind, ArrServer, ArrSettings, Config};
use putio_sync_telemetry::arr as metrics;
use putio_sync_telemetry::labels::{ArrMethod, ArrOutcome};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::{info, warn};

/// Header of the Transmission session handshake
const SESSION_HEADER: &str = "X-Transmission-Session-Id";

/// Largest RPC body: base64 metainfo of a large .torrent
const MAX_RPC_BODY: usize = 16 * 1024 * 1024;

/// The download client and its session id (random per start)
#[derive(Clone)]
pub struct ArrRpc {
    /// The item service
    pub service: Arc<ArrService>,
    /// Value of `X-Transmission-Session-Id` clients must send back
    pub session_id: String,
}

impl ArrRpc {
    /// Wrap `service` with a fresh session id
    pub fn new(service: Arc<ArrService>) -> Self {
        Self {
            service,
            session_id: uuid::Uuid::new_v4().simple().to_string(),
        }
    }
}

/// `POST /transmission/rpc` (public: it has its own Basic auth)
pub async fn transmission_rpc(State(state): State<AppState>, request: Request) -> Response {
    let Some(arr) = state.arr.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Read per request: Settings turns it on and off without a restart (feature 018)
    let settings = arr.service.settings();
    if !settings.enabled {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(password) = settings.password.as_deref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "download client has no password",
        )
            .into_response();
    };

    let ip = client_ip(&request);
    if let Err(secs) = state.limiter.check(ip) {
        warn!(ip = %ip, retry_after_seconds = secs, "Download client rate limited");
        return ApiError::TooManyRequests {
            retry_after_seconds: secs,
        }
        .into_response();
    }

    match basic_credentials(&request) {
        Some((user, pass))
            if token_matches(&user, &settings.username) & token_matches(&pass, password) => {}
        provided => {
            // A request without credentials is not a guess, as in `require_auth`
            if provided.is_some() {
                let failures = state.limiter.record_failure(ip);
                warn!(ip = %ip, failures, "Download client authentication failed");
            }
            metrics::rpc_request(ArrMethod::Other, ArrOutcome::AuthFailed);
            return (
                StatusCode::UNAUTHORIZED,
                [(header::WWW_AUTHENTICATE, r#"Basic realm="putio-sync""#)],
            )
                .into_response();
        }
    }

    let session_ok = request
        .headers()
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == arr.session_id);
    if !session_ok {
        let mut response = StatusCode::CONFLICT.into_response();
        if let Ok(value) = HeaderValue::from_str(&arr.session_id) {
            response.headers_mut().insert(SESSION_HEADER, value);
        }
        return response;
    }
    // After the handshake: Radarr 6 opens it with a GET, as Transmission allows
    if request.method() != Method::POST {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let user_agent = request
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    arr.service.contacts().record(user_agent.as_deref());

    let bytes = match axum::body::to_bytes(request.into_body(), MAX_RPC_BODY).await {
        Ok(bytes) => bytes,
        Err(_) => return Json(rpc::invalid_request()).into_response(),
    };
    let reply = match serde_json::from_slice::<rpc::Request>(&bytes) {
        Ok(rpc_request) => rpc::handle(&arr.service, rpc_request).await,
        Err(_) => rpc::invalid_request(),
    };
    Json(reply).into_response()
}

/// `user:password` from an `Authorization: Basic` header
fn basic_credentials(request: &Request<Body>) -> Option<(String, String)> {
    let value = request
        .headers()
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let (scheme, encoded) = value.trim().split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return Some((String::new(), String::new()));
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_default();
    let (user, pass) = decoded.split_once(':').unwrap_or((&decoded, ""));
    Some((user.to_string(), pass.to_string()))
}

/// Protected `/arr` routes
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(get_arr))
        .route("/status", get(get_status))
        .route("/client", put(put_client))
        .route("/applications", post(add_application))
        .route("/applications/test", post(test_form))
        .route(
            "/applications/:index",
            put(edit_application).delete(delete_application),
        )
        .route("/applications/:index/test", post(test_saved))
        .route("/discover", post(discover))
        .route("/setup/preview", post(setup_preview))
        .route("/setup", post(setup_run))
        .route("/setup/check", post(setup_check))
        .route("/setup/mapping", post(setup_mapping))
        .route("/setup/recommendation", post(setup_recommendation))
}

/// The settings in use: the service's, or resolved from the config without a database
async fn current_settings(state: &AppState) -> Result<Arc<ArrSettings>, ApiError> {
    match &state.arr {
        Some(arr) => Ok(arr.service.settings()),
        None => state
            .config
            .read()
            .await
            .resolve_arr()
            .map(Arc::new)
            .map_err(|e| ApiError::InternalError(e.to_string())),
    }
}

/// The port the *arr apps should connect to
async fn listen_port(state: &AppState) -> u16 {
    state.api_port().await
}

/// The host the browser used to reach putio-sync, without its port
pub(crate) fn browser_host(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|h| setup::strip_port(h).to_string())
        .filter(|h| !h.is_empty())
}

/// Change `[arr]`, then save it and apply it. Nothing changes in memory or on disk unless
/// the whole change is valid and, with a config file, was written (FR-006, R1, R2).
async fn save_arr(
    state: &AppState,
    change: impl FnOnce(&mut ArrConfig) -> Result<(), ApiError>,
) -> Result<ArrSettings, ApiError> {
    let mut config = state.config.write().await;
    let mut candidate: Config = config.clone();
    change(&mut candidate.arr)?;
    let resolved = candidate
        .resolve_arr()
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    if let Some(path) = &state.config_path {
        Config::save_arr_section(path.as_ref(), &candidate.arr).map_err(|e| {
            ApiError::InternalError("Could not write the config file".into())
                .with_details(e.to_string())
        })?;
    }
    if let Some(arr) = &state.arr {
        arr.service.apply(resolved.clone());
    }
    putio_sync_core::log_redact::set_secrets(candidate.secret_values());
    *config = candidate;
    Ok(resolved)
}

/// A new download-client password: 64 random hex characters (R11)
pub(crate) fn generate_password() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// The parts of a server URL the form shows
fn url_parts(url: &str) -> Value {
    match reqwest::Url::parse(url) {
        Ok(u) => json!({
            "host": u.host_str().unwrap_or(""),
            "port": u.port_or_known_default(),
            "ssl": u.scheme() == "https",
            "url_base": u.path().trim_end_matches('/'),
        }),
        Err(_) => json!({ "host": "", "port": null, "ssl": false, "url_base": "" }),
    }
}

/// `GET /arr`: what the Settings card shows (contracts/api.md). No password or API key.
pub async fn get_arr(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let settings = current_settings(&state).await?;
    let download_dir = download_dir(&state).await;
    let legacy: Vec<Option<String>> = state
        .config
        .read()
        .await
        .arr
        .servers
        .iter()
        .map(|s| s.category.clone())
        .collect();
    let hosts = tailnet_first_hosts(&state, browser_host(&headers).as_deref(), None).await;
    let info = |url: &str| state.arr.as_ref().and_then(|a| a.service.client_info(url));

    let applications: Vec<Value> = settings
        .servers
        .iter()
        .enumerate()
        .map(|(index, s)| {
            let read = info(&s.url);
            let mut app = json!({
                "index": index,
                "kind": s.kind.as_str(),
                "name": s.name,
                "url": s.url,
                "api_key_set": s.api_key.is_some(),
                "api_key_env": ArrSettings::api_key_from_env(&settings.servers, index),
                // Read from the *arr's own putio-sync client (R4); null before the first read
                "categories": read.as_ref().map(|i| i.categories.clone()).unwrap_or_default(),
                "client_present": read.as_ref().and_then(|i| i.read_at.map(|_| i.client_present)),
                "read_at": read.as_ref().and_then(|i| i.read_at),
                "read_error": read.as_ref().and_then(|i| i.last_error),
                "legacy_category": legacy.get(index).cloned().flatten(),
                "path_check": read.as_ref().and_then(|i| i.path_check.clone()),
            });
            if let (Value::Object(app), Value::Object(parts)) = (&mut app, url_parts(&s.url)) {
                app.extend(parts);
            }
            app
        })
        .collect();

    Ok(Json(json!({
        "client": {
            "enabled": settings.enabled,
            // The RPC needs the database
            "available": state.arr.is_some(),
            "username": settings.username,
            "password_set": settings.password.is_some(),
            "env": { "password": ArrSettings::password_from_env() },
            "connection": {
                "implementation": "Transmission",
                "port": listen_port(&state).await,
                "ssl": false,
                "url_base": putio_sync_arr::clients::URL_BASE,
                "hosts": hosts,
            },
            "download_dir": download_dir,
        },
        "applications": applications,
    })))
}

/// `PUT /arr/client` body
#[derive(Deserialize)]
pub struct ClientForm {
    enabled: bool,
    username: String,
    /// Empty keeps the current one
    #[serde(default)]
    password: String,
}

/// `PUT /arr/client`: the Download client form (US3). Applies without a restart.
pub async fn put_client(
    State(state): State<AppState>,
    Json(form): Json<ClientForm>,
) -> Result<Json<Value>, ApiError> {
    let from_env = ArrSettings::password_from_env();
    let mut password_changed = false;
    let resolved = save_arr(&state, |arr| {
        arr.enabled = form.enabled;
        arr.username = form.username.trim().to_string();
        // An env password wins, so the form's is ignored (FR-007)
        if !form.password.is_empty() && !from_env {
            password_changed = arr.password.as_deref() != Some(form.password.as_str());
            arr.password = Some(form.password.clone());
        }
        let has_password = from_env || arr.password.as_deref().is_some_and(|p| !p.is_empty());
        if arr.enabled && !has_password {
            return Err(ApiError::BadRequest(
                "Set or generate a password first".into(),
            ));
        }
        Ok(())
    })
    .await?;
    info!(
        enabled = resolved.enabled,
        username = %resolved.username,
        password_changed,
        "Download client settings saved"
    );
    Ok(Json(json!({
        "message": "Saved. Applies now.",
        "password_changed": password_changed,
    })))
}

/// The application body (contracts/api.md, "Applications")
#[derive(Deserialize)]
pub struct ApplicationForm {
    kind: ArrKind,
    #[serde(default)]
    name: String,
    host: String,
    /// Empty uses 7878 or 8989
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    ssl: bool,
    #[serde(default)]
    url_base: String,
    /// Empty keeps the saved key
    #[serde(default)]
    api_key: String,
    /// `POST /arr/applications/test` only: the saved application whose key to use
    #[serde(default)]
    index: Option<usize>,
}

impl ApplicationForm {
    /// Host, port, SSL and URL base joined into the server URL
    fn url(&self) -> Result<String, ApiError> {
        let bad = |m: &str| ApiError::BadRequest(m.to_string());
        let host = self.host.trim();
        if host.is_empty() {
            return Err(bad("Enter the host"));
        }
        if host.contains("://") || host.contains('/') {
            return Err(bad("Enter the host only, without http:// or a path"));
        }
        let scheme = if self.ssl { "https" } else { "http" };
        let mut url = reqwest::Url::parse(&format!("{}://placeholder", scheme))
            .map_err(|_| bad("Invalid address"))?;
        url.set_host(Some(host)).map_err(|_| bad("Invalid host"))?;
        let port = self.port.unwrap_or(match self.kind {
            ArrKind::Radarr => 7878,
            ArrKind::Sonarr => 8989,
        });
        url.set_port(Some(port)).map_err(|_| bad("Invalid port"))?;
        let base = self.url_base.trim().trim_matches('/');
        url.set_path(&if base.is_empty() {
            String::new()
        } else {
            format!("/{}", base)
        });
        Ok(url.as_str().trim_end_matches('/').to_string())
    }

    fn name(&self) -> Option<String> {
        Some(self.name.trim().to_string()).filter(|n| !n.is_empty())
    }

    fn key(&self) -> Option<String> {
        Some(self.api_key.trim().to_string()).filter(|k| !k.is_empty())
    }
}

fn no_application(index: usize) -> ApiError {
    ApiError::NotFound(format!("No application at index {}", index))
}

/// `GET /arr/status`: the Status page card (FR-002). Item counts by state and each *arr's
/// last contact.
pub async fn get_status(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let settings = current_settings(&state).await?;
    let mut items = json!({ "putio": 0, "local": 0, "done": 0, "failed": 0 });
    let mut clients_seen = Vec::new();
    let mut held = Vec::new();
    if let Some(arr) = &state.arr {
        for (state, n) in arr
            .service
            .counts()
            .await
            .map_err(|e| ApiError::InternalError(e.to_string()))?
        {
            items[state.as_str()] = json!(n);
        }
        clients_seen = arr
            .service
            .contacts()
            .snapshot()
            .into_iter()
            .map(|(agent, at)| json!({ "agent": agent, "last_contact": at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true) }))
            .collect();
        // Magnets held until a cache has their file list (043). Never the magnet.
        held = arr
            .service
            .held()
            .await
            .map_err(|e| ApiError::InternalError(e.to_string()))?
            .into_iter()
            .map(|(name, h)| {
                json!({
                    "hash": h.hash,
                    "name": name,
                    "since": unix_rfc3339(h.held_at),
                    "until": unix_rfc3339(h.until_at),
                    "attempts": h.attempts,
                    "last_lookup": h.last_lookup,
                })
            })
            .collect();
    }
    Ok(Json(json!({
        "enabled": settings.enabled,
        "clients_seen": clients_seen,
        "items": items,
        "held": held,
    })))
}

/// Unix seconds as `2026-10-28T10:00:00Z`
fn unix_rfc3339(secs: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// The download folder the *arr apps are told about
async fn download_dir(state: &AppState) -> String {
    match &state.arr {
        Some(arr) => arr.service.download_dir(),
        None => {
            let target = state.config.read().await.download.target_directory.clone();
            putio_sync_arr::view::download_dir(&target, "")
        }
    }
}

/// Can `server` see the download folder, as its client at `client_host` reports it (R10).
/// With `keep`, the result is kept with the application's [`ClientInfo`].
async fn check_path(
    state: &AppState,
    server: &ArrServer,
    client_host: &str,
    keep: Option<&ArrService>,
) -> Option<PathCheck> {
    if let Some(service) = keep {
        return service.check_path(server, client_host).await;
    }
    let key = server.api_key.as_deref()?;
    let http = match &state.arr {
        Some(arr) => arr.service.http().clone(),
        None => reqwest::Client::new(),
    };
    let api = ArrApi::new(server.kind, &server.url, key, http);
    Some(path_check::check(&api, &download_dir(state).await, client_host).await)
}

/// Test one application: its status, its putio-sync clients, then whether it can see the
/// download folder through them. With `keep`, the results are kept as the application's
/// [`ClientInfo`].
async fn test_application(
    state: &AppState,
    headers: &HeaderMap,
    server: &ArrServer,
    keep: Option<&ArrService>,
) -> Value {
    let Some(key) = server.api_key.as_deref() else {
        return json!({
            "ok": false,
            "reason": "no_api_key",
            "detail": format!("Enter the API key, or set PUTIO_SYNC_{}_API_KEY", server.kind.as_str().to_uppercase()),
        });
    };
    let http = match &state.arr {
        Some(arr) => arr.service.http().clone(),
        None => reqwest::Client::new(),
    };
    let api = ArrApi::new(server.kind, &server.url, key, http);
    let status = match api.system_status().await {
        Ok(status) => status,
        Err(e) => return json!({ "ok": false, "reason": e.kind(), "detail": e.to_string() }),
    };
    let port = listen_port(state).await;
    let hosts = tailnet_first_hosts(state, browser_host(headers).as_deref(), None).await;
    let info: ClientInfo = match keep {
        Some(service) => service
            .refresh_client_info(server, port, &hosts)
            .await
            .unwrap_or_default(),
        None => clients::read_clients(&api, port, &hosts, None).await,
    };
    // Without a putio-sync client there's no folder for the *arr to look for yet
    let path_check = match info.host.as_deref() {
        Some(host) if info.client_present => check_path(state, server, host, keep).await,
        _ => None,
    };
    json!({
        "ok": true,
        "app": status.app_name,
        "version": status.version,
        "categories": info.categories,
        "client_present": info.client_present,
        "read_error": info.last_error,
        "path_check": path_check,
    })
}

/// `POST /arr/applications/test`: unsaved form values. An empty key with `index` uses that
/// application's saved key.
pub async fn test_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(form): Json<ApplicationForm>,
) -> Result<Json<Value>, ApiError> {
    let url = form.url()?;
    let api_key = match (form.key(), form.index) {
        (Some(key), _) => Some(key),
        (None, Some(index)) => current_settings(&state)
            .await?
            .servers
            .get(index)
            .ok_or_else(|| no_application(index))?
            .api_key
            .clone(),
        (None, None) => None,
    };
    let server = ArrServer {
        kind: form.kind,
        name: form.name(),
        url,
        api_key,
        category: None,
    };
    Ok(Json(
        test_application(&state, &headers, &server, None).await,
    ))
}

/// `POST /arr/applications/{index}/test`: also refreshes its categories (R4)
pub async fn test_saved(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(index): Path<usize>,
) -> Result<Json<Value>, ApiError> {
    let settings = current_settings(&state).await?;
    let server = settings
        .servers
        .get(index)
        .ok_or_else(|| no_application(index))?;
    let service = state.arr.as_ref().map(|a| a.service.clone());
    Ok(Json(
        test_application(&state, &headers, server, service.as_deref()).await,
    ))
}

/// Re-read a saved application's categories after it changed (R4). Best effort.
async fn refresh_saved(state: &AppState, headers: &HeaderMap, url: &str) {
    let Some(arr) = &state.arr else {
        return;
    };
    let settings = arr.service.settings();
    let wanted = putio_sync_arr::service::url_key(url);
    let Some(server) = settings
        .servers
        .iter()
        .find(|s| putio_sync_arr::service::url_key(&s.url) == wanted)
    else {
        return;
    };
    let port = listen_port(state).await;
    let hosts = tailnet_first_hosts(state, browser_host(headers).as_deref(), None).await;
    arr.service.refresh_client_info(server, port, &hosts).await;
}

/// `POST /arr/applications`: adds one (FR-012). The UI tests it first.
pub async fn add_application(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(form): Json<ApplicationForm>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let url = form.url()?;
    let api_key = form
        .key()
        .ok_or_else(|| ApiError::BadRequest("Enter the API key".into()))?;
    let mut index = 0;
    save_arr(&state, |arr| {
        index = arr.servers.len();
        arr.servers.push(ArrServer {
            kind: form.kind,
            name: form.name(),
            url: url.clone(),
            api_key: Some(api_key),
            category: None,
        });
        Ok(())
    })
    .await?;
    info!(index, kind = form.kind.as_str(), url = %url, "Application added");
    refresh_saved(&state, &headers, &url).await;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "index": index, "message": "Saved. Applies now." })),
    ))
}

/// `PUT /arr/applications/{index}`: an empty key keeps the saved one (US4 #4)
pub async fn edit_application(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(index): Path<usize>,
    Json(form): Json<ApplicationForm>,
) -> Result<Json<Value>, ApiError> {
    let url = form.url()?;
    let key_changed = form.key().is_some();
    save_arr(&state, |arr| {
        let server = arr
            .servers
            .get_mut(index)
            .ok_or_else(|| no_application(index))?;
        server.kind = form.kind;
        server.name = form.name();
        server.url = url.clone();
        if let Some(key) = form.key() {
            server.api_key = Some(key);
        }
        Ok(())
    })
    .await?;
    info!(index, kind = form.kind.as_str(), url = %url, key_changed, "Application saved");
    refresh_saved(&state, &headers, &url).await;
    Ok(Json(json!({ "message": "Saved. Applies now." })))
}

/// `DELETE /arr/applications/{index}`: from the config only; the *arr is left as it is
/// (FR-013)
pub async fn delete_application(
    State(state): State<AppState>,
    Path(index): Path<usize>,
) -> Result<StatusCode, ApiError> {
    let mut removed = None;
    save_arr(&state, |arr| {
        if index >= arr.servers.len() {
            return Err(no_application(index));
        }
        removed = Some(arr.servers.remove(index));
        Ok(())
    })
    .await?;
    if let Some(server) = removed {
        info!(index, kind = server.kind.as_str(), url = %server.url, "Application removed");
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Auto-setup needs the item service: the RPC it sets up can't run without the database
fn arr_service(state: &AppState) -> Result<Arc<ArrService>, ApiError> {
    state
        .arr
        .as_ref()
        .map(|a| a.service.clone())
        .ok_or_else(|| {
            ApiError::ServiceUnavailable(
                "The Radarr/Sonarr download client needs the database".into(),
            )
        })
}

/// `POST /arr/discover`: at most 10 s, one at a time (R9)
pub async fn discover(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let service = arr_service(&state)?;
    let saved: Vec<String> = service
        .settings()
        .servers
        .iter()
        .map(|s| s.url.clone())
        .collect();
    let gateway = setup::SystemHosts::detect().gateway;
    let candidates = discovery::candidates(
        &saved,
        gateway.as_deref(),
        browser_host(&headers).as_deref(),
    );
    let found = discovery::discover(
        service.discovery_lock(),
        candidates,
        service.http().clone(),
        Limits::default(),
    )
    .await
    .map_err(|_| ApiError::Conflict("Discovery already running".into()))?;
    Ok(Json(json!(found)))
}

/// Which *arr a setup is for
#[derive(Deserialize)]
#[serde(untagged)]
pub enum SetupTarget {
    /// A saved application, with its saved key
    Application {
        /// Its index in `applications`
        application: usize,
    },
    /// A new address
    Url {
        /// The *arr's base URL
        url: String,
    },
}

/// `POST /arr/setup` and `/arr/setup/preview` body (data-model "SetupRequest")
#[derive(Deserialize)]
pub struct SetupRequest {
    pub(crate) target: SetupTarget,
    #[serde(default)]
    pub(crate) api_key: String,
    #[serde(default)]
    pub(crate) category: Option<String>,
    /// Overrides the host the *arr uses for putio-sync (FR-022)
    #[serde(default)]
    pub(crate) host: Option<String>,
    /// US5
    #[serde(default)]
    pub(crate) disable_blackhole: bool,
    #[serde(default)]
    pub(crate) remember: bool,
}

/// The *arr's URL and key for a request
pub(crate) fn target_of(
    req: &SetupRequest,
    settings: &ArrSettings,
) -> Result<(String, String), ApiError> {
    target_key(&req.target, &req.api_key, settings)
}

/// The *arr's URL and key for a target, with the saved key when `api_key` is empty
fn target_key(
    target: &SetupTarget,
    api_key: &str,
    settings: &ArrSettings,
) -> Result<(String, String), ApiError> {
    match target {
        SetupTarget::Application { application } => {
            let server = settings.servers.get(*application).ok_or_else(|| {
                ApiError::NotFound(format!("No application at index {}", application))
            })?;
            let key = Some(api_key.trim())
                .filter(|k| !k.is_empty())
                .or(server.api_key.as_deref())
                .ok_or_else(|| ApiError::BadRequest("Enter the API key".into()))?;
            Ok((server.url.clone(), key.to_string()))
        }
        SetupTarget::Url { url } => {
            let url = url.trim().trim_end_matches('/');
            if url.is_empty() {
                return Err(ApiError::BadRequest("Enter the address".into()));
            }
            let key = api_key.trim();
            if key.is_empty() {
                return Err(ApiError::BadRequest("Enter the API key".into()));
            }
            // A bare host is expanded by setup (US2)
            Ok((url.to_string(), key.to_string()))
        }
    }
}

async fn setup_input(
    state: &AppState,
    headers: &HeaderMap,
    req: &SetupRequest,
    settings: &ArrSettings,
) -> Result<SetupInput, ApiError> {
    let (url, api_key) = target_of(req, settings)?;
    Ok(SetupInput {
        url,
        api_key,
        category: req.category.clone().filter(|c| !c.trim().is_empty()),
        hosts: tailnet_first_hosts(state, browser_host(headers).as_deref(), req.host.as_deref())
            .await,
        port: listen_port(state).await,
        username: settings.username.clone(),
        disable_blackhole: req.disable_blackhole,
        // Empty in a preview without a password yet; `setup_run` makes one first
        password: settings.password.clone().unwrap_or_default(),
    })
}

/// `POST /arr/setup/preview`: checks the address and key; changes nothing
pub async fn setup_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SetupRequest>,
) -> Result<Json<Value>, ApiError> {
    let service = arr_service(&state)?;
    let settings = service.settings();
    let input = setup_input(&state, &headers, &req, &settings).await?;
    Ok(Json(
        match setup::preview(&input, service.http().clone()).await {
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

/// Turn the download client on, with a generated password when there's none (FR-023a)
async fn ensure_client_on(
    state: &AppState,
    settings: &ArrSettings,
) -> Result<ArrSettings, ApiError> {
    if settings.enabled && settings.password.is_some() {
        return Ok(settings.clone());
    }
    let resolved = save_arr(state, |arr| {
        arr.enabled = true;
        if !ArrSettings::password_from_env() && arr.password.as_deref().is_none_or(str::is_empty) {
            arr.password = Some(generate_password());
        }
        Ok(())
    })
    .await?;
    info!("Download client turned on for auto-setup");
    Ok(resolved)
}

/// Save the application with its key, replacing one with the same URL (FR-023b)
async fn remember(
    state: &AppState,
    result: &setup::SetupResult,
    url: &str,
    key: &str,
) -> Result<(), ApiError> {
    let Some(kind) = result.kind else {
        return Ok(());
    };
    let normal = |u: &str| u.trim_end_matches('/').to_ascii_lowercase();
    save_arr(state, |arr| {
        match arr
            .servers
            .iter_mut()
            .find(|s| normal(&s.url) == normal(url))
        {
            Some(server) => {
                server.kind = kind;
                server.api_key = Some(key.to_string());
            }
            None => arr.servers.push(ArrServer {
                kind,
                name: None,
                url: url.trim_end_matches('/').to_string(),
                api_key: Some(key.to_string()),
                category: None,
            }),
        }
        Ok(())
    })
    .await?;
    Ok(())
}

/// `POST /arr/setup`: returns 200 with a SetupResult, which may be a failed one
pub async fn setup_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SetupRequest>,
) -> Result<Json<Value>, ApiError> {
    let service = arr_service(&state)?;
    let before = service.settings();
    // Refuse a bad request before turning anything on
    let (_, key) = target_of(&req, &before)?;
    let settings = match ensure_client_on(&state, &before).await {
        Ok(settings) => settings,
        Err(e) => {
            return Ok(Json(json!({
                "outcome": "failed",
                "reason": "config_write_failed",
                "detail": format!("The download client could not be turned on: {}", e),
                "hosts_tried": [],
                "blackhole_disabled": false,
                "remembered": false,
            })));
        }
    };
    let input = setup_input(&state, &headers, &req, &settings).await?;
    let mut result = setup::run(&input, service.http().clone()).await;
    let saved_target = matches!(req.target, SetupTarget::Url { .. });
    if req.remember && saved_target && result.outcome != Outcome::Failed {
        // The URL that answered, so a bare host is saved with its port
        let url = result.url.clone().unwrap_or_default();
        match remember(&state, &result, &url, &key).await {
            Ok(()) => result.remembered = true,
            Err(e) => {
                warn!(error = %e, "Auto-setup worked, but the application was not saved");
                result.warning = Some(format!("Set up, but the application was not saved: {}", e));
            }
        }
    }
    if result.outcome != Outcome::Failed {
        if let (Some(url), Some(kind)) = (result.url.clone(), result.kind) {
            refresh_saved(&state, &headers, &url).await;
            let saved = service.settings().servers.iter().any(|s| {
                putio_sync_arr::service::url_key(&s.url) == putio_sync_arr::service::url_key(&url)
            });
            let server = ArrServer {
                kind,
                name: None,
                url,
                api_key: Some(input.api_key.clone()),
                category: None,
            };
            let host = result.host.clone().unwrap_or_default();
            let keep = saved.then_some(service.as_ref());
            result.path_check = check_path(&state, &server, &host, keep).await;
        }
    }
    Ok(Json(json!(result)))
}

/// `POST /arr/setup/check`, `/mapping` and `/recommendation` body (feature 049)
#[derive(Deserialize)]
pub struct SetupStepRequest {
    target: SetupTarget,
    #[serde(default)]
    api_key: String,
    /// The host in the *arr's putio-sync client, when the caller knows it
    #[serde(default)]
    host: Option<String>,
    /// `/mapping`: the proposal the operator confirmed
    #[serde(default)]
    proposal: Option<Proposal>,
    /// `/recommendation`: which setting
    #[serde(default)]
    name: Option<String>,
}

/// A connected *arr for one step
struct StepTarget {
    api: ArrApi,
    key: String,
    /// The saved application's URL, whose row keeps the folder check
    saved_url: Option<String>,
}

/// `{ok: false, reason, detail}` with the key taken out of the detail
fn step_failure(reason: &str, detail: &str, key: &str) -> Value {
    let detail = if key.is_empty() {
        detail.to_string()
    } else {
        detail.replace(key, "********")
    };
    json!({ "ok": false, "reason": reason, "detail": detail })
}

/// Find the *arr a step is for. `Err(Ok(body))` is a 200 failure body.
async fn step_target(
    service: &ArrService,
    req: &SetupStepRequest,
) -> Result<Result<StepTarget, Value>, ApiError> {
    let settings = service.settings();
    let (url, key) = target_key(&req.target, &req.api_key, &settings)?;
    let saved_url = match req.target {
        SetupTarget::Application { .. } => Some(url.clone()),
        SetupTarget::Url { .. } => None,
    };
    Ok(
        match setup::connect(&url, &key, service.http().clone()).await {
            Ok(api) => Ok(StepTarget {
                api,
                key,
                saved_url,
            }),
            Err(f) => Err(step_failure(f.reason, &f.detail, &key)),
        },
    )
}

/// The host in the *arr's putio-sync client: the one given, or read from its clients
async fn client_host(
    state: &AppState,
    headers: &HeaderMap,
    api: &ArrApi,
    given: Option<&str>,
) -> Option<String> {
    if let Some(h) = given.map(str::trim).filter(|h| !h.is_empty()) {
        return Some(h.to_string());
    }
    let hosts = tailnet_first_hosts(state, browser_host(headers).as_deref(), None).await;
    clients::read_clients(api, listen_port(state).await, &hosts, None)
        .await
        .host
        .filter(|h| !h.is_empty())
}

/// Make the marker and work out the folder step. `Err` is a `{reason, detail}`.
async fn analyse_folder(
    state: &AppState,
    api: &ArrApi,
    host: &str,
    key: &str,
) -> Result<FolderAnalysis, Value> {
    let folder = download_dir(state).await;
    let marker = Marker::create(std::path::Path::new(&folder)).map_err(|e| {
        warn!(error = %e, "Could not make the path check marker");
        json!({ "reason": "marker_failed", "detail": e.to_string() })
    })?;
    path_mapping::analyse(
        api,
        &folder,
        host,
        marker.name(),
        path_mapping::Limits::default(),
    )
    .await
    .map_err(|e| {
        let failure = step_failure(e.kind(), &e.to_string(), key);
        json!({ "reason": failure["reason"], "detail": failure["detail"] })
    })
}

/// `POST /arr/setup/check`: the folder step and the recommendations; changes nothing in the
/// *arr
pub async fn setup_check(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SetupStepRequest>,
) -> Result<Json<Value>, ApiError> {
    let service = arr_service(&state)?;
    let target = match step_target(&service, &req).await? {
        Ok(t) => t,
        Err(body) => return Ok(Json(body)),
    };
    let api = &target.api;
    let host = client_host(&state, &headers, api, req.host.as_deref()).await;
    let (folder, folder_error) = match &host {
        None => (
            None,
            Some(json!({ "reason": "no_client",
                "detail": "This app has no putio-sync download client yet" })),
        ),
        Some(host) => match analyse_folder(&state, api, host, &target.key).await {
            Ok(a) => {
                if let Some(url) = &target.saved_url {
                    service.keep_path_check(url, a.path_check.clone());
                }
                (Some(a), None)
            }
            Err(e) => (None, Some(e)),
        },
    };
    let (recommendations, recommendations_error) = match recommend::read(api).await {
        Ok(r) => (json!(r), Value::Null),
        Err(e) => {
            let failure = step_failure(e.kind(), &e.to_string(), &target.key);
            (
                json!([]),
                json!({ "reason": failure["reason"], "detail": failure["detail"] }),
            )
        }
    };
    Ok(Json(json!({
        "ok": true,
        "kind": api.kind(),
        "app": api.kind().app_name(),
        "url": api.url(),
        "host": host,
        "folder": folder,
        "folder_error": folder_error,
        "recommendations": recommendations,
        "recommendations_error": recommendations_error,
    })))
}

/// `POST /arr/setup/mapping`: re-runs the folder step and makes the change only when it
/// still proposes the one confirmed (R4)
pub async fn setup_mapping(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SetupStepRequest>,
) -> Result<Json<Value>, ApiError> {
    let service = arr_service(&state)?;
    let Some(confirmed) = req.proposal.clone() else {
        return Err(ApiError::BadRequest("No proposal given".into()));
    };
    let target = match step_target(&service, &req).await? {
        Ok(t) => t,
        Err(body) => return Ok(Json(body)),
    };
    let api = &target.api;
    let Some(host) = client_host(&state, &headers, api, req.host.as_deref()).await else {
        return Ok(Json(step_failure(
            "no_client",
            "This app has no putio-sync download client yet",
            "",
        )));
    };
    let fresh = match analyse_folder(&state, api, &host, &target.key).await {
        Ok(a) => a,
        Err(e) => {
            return Ok(Json(
                json!({ "ok": false, "reason": e["reason"], "detail": e["detail"] }),
            ))
        }
    };
    if fresh.proposal != confirmed {
        return Ok(Json(
            json!({ "ok": false, "reason": "changed", "proposal": fresh.proposal }),
        ));
    }
    let writes = matches!(
        confirmed.action,
        ProposalAction::Create | ProposalAction::Update | ProposalAction::Remove
    );
    if !writes {
        return Ok(Json(
            json!({ "ok": true, "done": "none", "path_check": fresh.path_check }),
        ));
    }
    if let Err(e) = path_mapping::apply(api, &confirmed).await {
        return Ok(Json(step_failure(e.kind(), &e.to_string(), &target.key)));
    }
    // The check after the change, with a fresh marker
    let after = match analyse_folder(&state, api, &host, &target.key).await {
        Ok(a) => a.path_check,
        Err(_) => path_check::check(api, &download_dir(&state).await, &host).await,
    };
    if let Some(url) = &target.saved_url {
        service.keep_path_check(url, after.clone());
    }
    Ok(Json(
        json!({ "ok": true, "done": confirmed.action, "path_check": after }),
    ))
}

/// `POST /arr/setup/recommendation`: turn on one recommended setting
pub async fn setup_recommendation(
    State(state): State<AppState>,
    Json(req): Json<SetupStepRequest>,
) -> Result<Json<Value>, ApiError> {
    let service = arr_service(&state)?;
    let name = req.name.clone().unwrap_or_default();
    if !recommend::NAMES.contains(&name.as_str()) {
        return Err(ApiError::BadRequest(format!("Unknown setting '{}'", name)));
    }
    let target = match step_target(&service, &req).await? {
        Ok(t) => t,
        Err(body) => return Ok(Json(body)),
    };
    let result = match recommend::turn_on(&target.api, &name).await {
        Ok(_) => recommend::read(&target.api).await,
        Err(e) => Err(e),
    };
    Ok(Json(match result {
        Ok(r) => json!({ "ok": true, "recommendations": r }),
        Err(e) => step_failure(e.kind(), &e.to_string(), &target.key),
    }))
}
