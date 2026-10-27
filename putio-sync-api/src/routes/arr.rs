//! Radarr/Sonarr download client (feature 011)
//!
//! - POST /transmission/rpc            - Transmission RPC, own Basic auth (public route)
//! - GET  /arr                         - Settings card data (protected)
//! - POST /arr/servers/{index}/test    - *arr connection test (protected)

use crate::error::ApiError;
use crate::middleware::auth::token_matches;
use crate::middleware::ratelimit::client_ip;
use crate::AppState;
use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use putio_sync_arr::rpc;
use putio_sync_arr::{ArrApi, ArrService};
use putio_sync_telemetry::arr as metrics;
use putio_sync_telemetry::labels::{ArrMethod, ArrOutcome};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::warn;

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
    if request.method() != Method::POST {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let settings = arr.service.settings();
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
        .route("/servers/:index/test", post(test_server))
}

/// `GET /arr`: what the Settings card shows. No password or API key.
pub async fn get_arr(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let resolved;
    let (settings, arr) = match &state.arr {
        Some(arr) => (arr.service.settings(), Some(arr)),
        None => {
            resolved = state
                .config
                .read()
                .await
                .resolve_arr()
                .map_err(|e| ApiError::InternalError(e.to_string()))?;
            (&resolved, None)
        }
    };

    let mut items = json!({ "putio": 0, "local": 0, "done": 0, "failed": 0 });
    let mut clients_seen = Vec::new();
    let download_dir = match arr {
        Some(arr) => {
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
            arr.service.download_dir()
        }
        None => {
            let target = state.config.read().await.download.target_directory.clone();
            putio_sync_arr::view::download_dir(&target, "")
        }
    };

    let servers: Vec<Value> = settings
        .servers
        .iter()
        .enumerate()
        .map(|(index, s)| {
            json!({
                "index": index,
                "kind": s.kind.as_str(),
                "url": s.url,
                "category": s.category,
                "api_key_set": s.api_key.is_some(),
            })
        })
        .collect();

    Ok(Json(json!({
        // Enabled in config, and running: the RPC needs the database
        "enabled": settings.enabled && arr.is_some(),
        "password_set": settings.password.is_some(),
        "username": settings.username,
        "url_base": "/transmission/",
        "download_dir": download_dir,
        "clients_seen": clients_seen,
        "servers": servers,
        "items": items,
    })))
}

/// `POST /arr/servers/{index}/test`
pub async fn test_server(
    State(state): State<AppState>,
    Path(index): Path<usize>,
) -> Result<Json<Value>, ApiError> {
    let settings = match &state.arr {
        Some(arr) => arr.service.settings().clone(),
        None => state
            .config
            .read()
            .await
            .resolve_arr()
            .map_err(|e| ApiError::InternalError(e.to_string()))?,
    };
    let server = settings
        .servers
        .get(index)
        .ok_or_else(|| ApiError::NotFound(format!("No arr server at index {}", index)))?;
    let Some(key) = server.api_key.as_deref() else {
        return Ok(Json(json!({
            "ok": false,
            "reason": "no_api_key",
            "detail": format!("Set api_key for this server or PUTIO_SYNC_{}_API_KEY", server.kind.as_str().to_uppercase()),
        })));
    };
    let http = match &state.arr {
        Some(arr) => arr.service.http().clone(),
        None => reqwest::Client::new(),
    };
    let api = ArrApi::new(server.kind, &server.url, key, http);
    Ok(Json(match api.system_status().await {
        Ok(status) => json!({ "ok": true, "app": status.app_name, "version": status.version }),
        Err(e) => json!({ "ok": false, "reason": e.kind(), "detail": e.to_string() }),
    }))
}
