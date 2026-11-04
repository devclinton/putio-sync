//! Setup mode (feature 013): the router a daemon serves while it has no usable config.
//!
//! The UI and `/setup/status` are public. Every other `/setup/*` route needs the one-time
//! setup code the daemon printed, or an API token already configured, as
//! `Authorization: Bearer`. Wrong values count toward the auth limiter. Any other path
//! answers 503 until setup is finished.
//!
//! `POST /setup/finish` writes the config once and marks the state finished; the daemon
//! waits on [`SetupState::finished`] and then starts normally.

use crate::error::ApiError;
use crate::middleware::auth::token_matches;
use crate::middleware::ratelimit::{
    client_ip, AuthLimiter, AUTH_FAILURE_LIMIT, AUTH_FAILURE_WINDOW,
};
use crate::routes::setup as handlers;
use crate::ui;
use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use putio_sync_client::DEFAULT_BASE_URL;
use putio_sync_core::config::{ArrConfig, Config};
use putio_sync_core::setup::{SetupChoices, SetupReason};
use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{watch, Mutex};
use tower_http::trace::TraceLayer;
use tracing::warn;

/// Letters and digits that can't be mistaken for each other (no 0/O, 1/I/L)
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// A new setup code: 12 random characters as `XXXX-XXXX-XXXX`
pub fn new_setup_code() -> String {
    let n = CODE_ALPHABET.len();
    // Bytes below the largest multiple of `n` keep `byte % n` uniform
    let cutoff = 256 - 256 % n;
    let mut chars = String::new();
    while chars.len() < 12 {
        let id = uuid::Uuid::new_v4();
        // Bytes 6 and 8 of a v4 UUID carry its version and variant
        for (i, b) in id.as_bytes().iter().enumerate() {
            if i != 6 && i != 8 && (*b as usize) < cutoff && chars.len() < 12 {
                chars.push(CODE_ALPHABET[*b as usize % n] as char);
            }
        }
    }
    format!("{}-{}-{}", &chars[0..4], &chars[4..8], &chars[8..12])
}

/// A new API token: 64 random hex characters
pub fn new_api_token() -> String {
    crate::routes::arr::generate_password()
}

/// A code as typed: upper case, without dashes or spaces
fn normalize_code(code: &str) -> String {
    code.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// The Put.io account the wizard is connected to; never the token or the e-mail
#[derive(Debug, Clone, Serialize)]
pub struct AccountSummary {
    /// Put.io username
    pub username: String,
    /// Bytes used
    pub used: u64,
    /// Bytes available in the plan
    pub size: u64,
}

/// A device-link code the wizard is waiting on
#[derive(Debug, Clone)]
pub(crate) struct LinkState {
    pub(crate) code: String,
    pub(crate) created_at: Instant,
}

/// What the wizard has chosen so far
pub(crate) struct Session {
    pub(crate) base: Config,
    pub(crate) reasons: Vec<SetupReason>,
    pub(crate) choices: SetupChoices,
    pub(crate) account: Option<AccountSummary>,
    pub(crate) link: Option<LinkState>,
    /// The `[arr]` draft for Radarr/Sonarr auto-setup, saved at finish
    pub(crate) arr: Option<ArrConfig>,
    pub(crate) finished: bool,
}

pub(crate) struct Inner {
    code: String,
    api_token: Option<String>,
    pub(crate) config_path: PathBuf,
    pub(crate) putio_base_url: String,
    pub(crate) session: Mutex<Session>,
    pub(crate) limiter: AuthLimiter,
    pub(crate) arr_discovery: Mutex<()>,
    pub(crate) http: reqwest::Client,
    done: watch::Sender<bool>,
}

/// Shared state of the setup router
#[derive(Clone)]
pub struct SetupState {
    pub(crate) inner: Arc<Inner>,
}

impl SetupState {
    /// A setup session for `config_path`. `base` is the config loaded from it, if it
    /// parsed; its other settings are kept when setup saves. A new setup code is made.
    pub fn new(config_path: PathBuf, base: Option<Config>, reasons: Vec<SetupReason>) -> Self {
        let base = base.unwrap_or_default();
        let api_token = base
            .resolve_api()
            .ok()
            .and_then(|r| r.value.token)
            .or_else(|| {
                std::env::var("PUTIO_SYNC_API_TOKEN")
                    .ok()
                    .filter(|t| !t.trim().is_empty())
            });
        let (done, _) = watch::channel(false);
        Self {
            inner: Arc::new(Inner {
                code: new_setup_code(),
                api_token,
                config_path,
                putio_base_url: DEFAULT_BASE_URL.to_string(),
                session: Mutex::new(Session {
                    base,
                    reasons,
                    choices: SetupChoices::default(),
                    account: None,
                    link: None,
                    arr: None,
                    finished: false,
                }),
                limiter: AuthLimiter::new(AUTH_FAILURE_LIMIT, AUTH_FAILURE_WINDOW),
                arr_discovery: Mutex::new(()),
                http: reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(15))
                    .build()
                    .unwrap_or_default(),
                done,
            }),
        }
    }

    /// Talk to Put.io at `url` instead (tests). Call before sharing the state.
    pub fn with_putio_base_url(mut self, url: &str) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.putio_base_url = url.trim_end_matches('/').to_string();
        }
        self
    }

    /// The one-time setup code, for the daemon to print
    pub fn code(&self) -> &str {
        &self.inner.code
    }

    /// Whether an API token was already configured (it unlocks the wizard too)
    pub fn api_token_configured(&self) -> bool {
        self.inner.api_token.is_some()
    }

    /// Whether `POST /setup/finish` has saved the config
    pub fn is_finished(&self) -> bool {
        *self.inner.done.borrow()
    }

    /// Resolves once `POST /setup/finish` has saved the config
    pub async fn finished(&self) {
        let mut rx = self.inner.done.subscribe();
        let _ = rx.wait_for(|done| *done).await;
    }

    pub(crate) fn mark_finished(&self) {
        self.inner.done.send_replace(true);
    }

    /// Whether `provided` is the setup code or the API token, compared in constant time
    fn unlocks(&self, provided: &str) -> bool {
        let by_code = token_matches(&normalize_code(provided), &normalize_code(&self.inner.code));
        let by_token = self
            .inner
            .api_token
            .as_deref()
            .is_some_and(|t| token_matches(provided, t));
        by_code | by_token
    }
}

/// The setup-mode router: public status and UI, gated `/setup/*`, and 503 for the rest
pub fn setup_router(state: SetupState) -> Router {
    let gated = Router::new()
        .route("/setup/unlock", post(handlers::unlock))
        .route("/setup/session", get(handlers::session))
        .route(
            "/setup/putio/link",
            post(handlers::link_start).get(handlers::link_check),
        )
        .route("/setup/putio/token", post(handlers::putio_token))
        .route("/setup/putio/folders", get(handlers::putio_folders))
        .route("/setup/folder/check", post(handlers::folder_check))
        .route("/setup/folder", put(handlers::folder_set))
        .route("/setup/download", put(handlers::download_set))
        .route("/setup/watch/check", post(handlers::watch_check))
        .route("/setup/extras", put(handlers::extras_set))
        .route("/setup/arr/discover", post(handlers::arr_discover))
        .route("/setup/arr/preview", post(handlers::arr_preview))
        .route("/setup/arr/run", post(handlers::arr_run))
        .route("/setup/dry-run", post(handlers::dry_run))
        .route("/setup/finish", post(handlers::finish))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_setup_auth,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/auth/status", get(auth_status))
        .route("/setup/status", get(handlers::status))
        .route("/", get(ui::index))
        .route("/assets/*path", get(ui::asset))
        .merge(gated)
        .fallback(setup_needed)
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

/// Bearer setup code or API token; 429 while rate limited
async fn require_setup_auth(
    State(state): State<SetupState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let ip = client_ip(&request);
    if let Err(secs) = state.inner.limiter.check(ip) {
        warn!(ip = %ip, retry_after_seconds = secs, "Setup authentication rate limited");
        return Err(ApiError::TooManyRequests {
            retry_after_seconds: secs,
        });
    }
    let provided = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .map(|h| {
            let h = h.trim();
            h.strip_prefix("Bearer").unwrap_or(h).trim().to_string()
        })
        .unwrap_or_default();
    if provided.is_empty() {
        return Err(ApiError::Unauthorized("Enter the setup code".to_string()));
    }
    if state.unlocks(&provided) {
        return Ok(next.run(request).await);
    }
    let failures = state.inner.limiter.record_failure(ip);
    warn!(ip = %ip, failures, "Setup authentication failed");
    Err(ApiError::Unauthorized("Wrong setup code".to_string()))
}

/// GET /health in setup mode
async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "setup",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// GET /auth/status in setup mode
async fn auth_status(State(state): State<SetupState>) -> Json<serde_json::Value> {
    Json(json!({
        "token_configured": state.api_token_configured(),
        "setup_required": !state.is_finished(),
    }))
}

/// Every other path while setup is needed
async fn setup_needed() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": "Setup needed", "setup_required": true })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_codes_are_unambiguous_and_differ() {
        let a = new_setup_code();
        let b = new_setup_code();
        assert_ne!(a, b);
        assert_eq!(a.len(), 14);
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(parts.len(), 3);
        for c in a.chars().filter(|c| *c != '-') {
            assert!(CODE_ALPHABET.contains(&(c as u8)), "{c}");
        }
    }

    #[test]
    fn codes_are_matched_as_typed() {
        let state = SetupState::new(PathBuf::from("/nonexistent"), None, vec![]);
        let code = state.code().to_string();
        assert!(state.unlocks(&code));
        assert!(state.unlocks(&code.to_lowercase().replace('-', " ")));
        assert!(!state.unlocks("AAAA-AAAA-AAAA"));
    }
}
