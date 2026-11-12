//! Sign-in routes for the web UI (feature 004, FR-006, FR-007)
//!
//! Both are public (no token needed):
//! - GET  /auth/status - Whether an API token is configured (and `setup_required: false`),
//!   and the Tailscale identity that lets this request in without one (feature 036)
//! - POST /auth/login  - Check a token: 204 on match, 401 otherwise, 429 when rate limited
//!
//! The UI keeps the token itself and sends it as `Authorization: Bearer` afterwards, so
//! there is no session to create. The request body is never logged.
//!
//! Sign-in options (feature 019), also public:
//! - POST /auth/password - Check the `[sign_in]` username and password and set a session
//!   cookie: 204, 401 (counted), 404 when not set up, 429
//! - POST /auth/logout - Clear the session cookie: always 204
//! - GET  /auth/oauth/:provider - Redirect to Google, GitHub or Discord (PKCE and state)
//! - GET  /auth/oauth/:provider/callback - Finish it: back to the UI signed in, or to
//!   `#/login?error=…`

use crate::error::ApiError;
use crate::middleware::auth::{not_configured, tailscale_identity, token_matches};
use crate::middleware::ratelimit::client_ip;
use crate::signin::{self, oauth, session};
use axum::{
    extract::{ConnectInfo, FromRequest, Path, Query, Request, State},
    http::{
        header::{LOCATION, SET_COOKIE},
        HeaderMap, StatusCode,
    },
    response::{AppendHeaders, IntoResponse, Response},
    Json,
};
use putio_sync_core::sign_in::{Provider, SignInConfig};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tracing::{info, warn};

// Re-export AppState from parent
pub use crate::AppState;

/// Response of `GET /auth/status`
#[derive(Debug, Serialize)]
pub struct AuthStatusResponse {
    pub token_configured: bool,
    /// Always false here; setup mode answers true (feature 013)
    pub setup_required: bool,
    /// Who this request is when Tailscale sign-in lets it in without the token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tailscale: Option<TailscaleUser>,
    /// Whether Tailscale sign-in is on (left out when it is off)
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub tailscale_sign_in: bool,
    /// The ways in the sign-in page offers besides the token (feature 019); left out
    /// when there are none
    #[serde(skip_serializing_if = "SignInOptions::is_off")]
    pub sign_in: SignInOptions,
    /// Who this browser's session cookie says it is, when it is still valid
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<SessionUser>,
}

/// What the sign-in page shows
#[derive(Debug, Serialize)]
pub struct SignInOptions {
    pub password: bool,
    pub providers: Vec<Provider>,
}

impl SignInOptions {
    fn is_off(&self) -> bool {
        !self.password && self.providers.is_empty()
    }
}

/// A browser signed in with a password or a provider
#[derive(Debug, Serialize)]
pub struct SessionUser {
    pub login: String,
    pub name: String,
    /// "password", "google", "github" or "discord"
    pub via: String,
    /// The `X-CSRF-Token` its writes must send
    pub csrf: String,
}

/// A Tailscale user let in by identity sign-in
#[derive(Debug, Serialize)]
pub struct TailscaleUser {
    pub login: String,
    pub name: String,
    pub via: &'static str,
}

/// Body of `POST /auth/login`. Deliberately not `Debug`, so it can't end up in a log.
#[derive(Deserialize)]
pub struct LoginRequest {
    pub token: String,
}

/// Whether an API token is configured
///
/// GET /auth/status
pub async fn auth_status(
    State(state): State<AppState>,
    request: Request,
) -> Json<AuthStatusResponse> {
    let (token_configured, tailscale_sign_in, sign_in) = {
        let config = state.config.read().await;
        (
            config.api.auth_token.is_some(),
            config.tailscale.identity_sign_in,
            config.sign_in.clone(),
        )
    };
    let user = signin::current_session(&state.sign_in, &sign_in, request.headers()).map(
        |(found, cookie, key)| SessionUser {
            csrf: session::csrf(&key, &cookie),
            login: found.login,
            name: found.name,
            via: found.how,
        },
    );
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| *addr);
    let tailscale = tailscale_identity(&state, request.headers(), peer)
        .await
        .map(|id| TailscaleUser {
            login: id.login,
            name: id.name,
            via: id.via.as_str(),
        });
    Json(AuthStatusResponse {
        token_configured,
        setup_required: false,
        tailscale,
        tailscale_sign_in,
        sign_in: SignInOptions {
            password: sign_in.password.is_set(),
            providers: sign_in.enabled_providers(),
        },
        user,
    })
}

/// Check a token for the sign-in page
///
/// POST /auth/login
///
/// The limiter is checked first, so a rate-limited client gets 429 before anything is
/// compared. A token that doesn't match counts as a failure.
pub async fn login(State(state): State<AppState>, request: Request) -> Response {
    let ip = client_ip(&request);
    if let Err(secs) = state.limiter.check(ip) {
        warn!(ip = %ip, retry_after_seconds = secs, "Authentication rate limited");
        return ApiError::TooManyRequests {
            retry_after_seconds: secs,
        }
        .into_response();
    }

    let Json(body) = match Json::<LoginRequest>::from_request(request, &state).await {
        Ok(body) => body,
        Err(rejection) => return rejection.into_response(),
    };

    let expected = state.config.read().await.api.auth_token.clone();
    let Some(expected) = expected else {
        return not_configured().into_response();
    };

    if token_matches(&body.token, &expected) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        let failures = state.limiter.record_failure(ip);
        warn!(ip = %ip, failures, "Authentication failed");
        ApiError::Unauthorized("Unauthorized".to_string()).into_response()
    }
}

/// Body of `POST /auth/password`. Not `Debug`, like [`LoginRequest`].
#[derive(Deserialize)]
pub struct PasswordRequest {
    pub username: String,
    pub password: String,
}

fn rate_limited(state: &AppState, ip: std::net::IpAddr) -> Option<Response> {
    let secs = state.limiter.check(ip).err()?;
    warn!(ip = %ip, retry_after_seconds = secs, "Authentication rate limited");
    Some(
        ApiError::TooManyRequests {
            retry_after_seconds: secs,
        }
        .into_response(),
    )
}

/// A 204 or redirect that sets a fresh session cookie for `found`
fn session_cookie(
    state: &AppState,
    config: &SignInConfig,
    headers: &HeaderMap,
    found: &session::Session,
) -> String {
    let key = state.sign_in.key_or_create();
    session::set_cookie(
        &session::sign(&key, found),
        config.session_hours,
        session::is_secure(headers, config.base_url()),
    )
}

/// Sign in with the `[sign_in]` username and password
///
/// POST /auth/password
pub async fn password(State(state): State<AppState>, request: Request) -> Response {
    let ip = client_ip(&request);
    if let Some(limited) = rate_limited(&state, ip) {
        return limited;
    }
    let headers = request.headers().clone();
    let Json(body) = match Json::<PasswordRequest>::from_request(request, &state).await {
        Ok(body) => body,
        Err(rejection) => return rejection.into_response(),
    };
    let config = state.config.read().await.sign_in.clone();
    if !config.password.is_set() {
        return ApiError::NotFound("Password sign-in is not set up".to_string()).into_response();
    }
    let hash = config.password.password_hash.clone();
    let user_ok = body.username == config.password.username;
    let pass = body.password;
    let check = hash.clone();
    let pass_ok = tokio::task::spawn_blocking(move || signin::verify_password(&pass, &check))
        .await
        .unwrap_or(false);
    if !(user_ok && pass_ok) {
        let failures = state.limiter.record_failure(ip);
        warn!(ip = %ip, failures, "Password sign-in failed");
        return ApiError::Unauthorized("Wrong username or password".to_string()).into_response();
    }
    let found = session::Session::new(
        "password",
        &config.password.username,
        &config.password.username,
        config.session_hours,
        signin::password_stamp(&hash),
    );
    info!(login = %found.login, "Signed in with the password");
    let cookie = session_cookie(&state, &config, &headers, &found);
    (
        StatusCode::NO_CONTENT,
        AppendHeaders([(SET_COOKIE, cookie)]),
    )
        .into_response()
}

/// Clear this browser's session cookie
///
/// POST /auth/logout
pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let public = state
        .config
        .read()
        .await
        .sign_in
        .base_url()
        .map(str::to_string);
    let cookie = session::clear_cookie(session::is_secure(&headers, public.as_deref()));
    (
        StatusCode::NO_CONTENT,
        AppendHeaders([(SET_COOKIE, cookie)]),
    )
        .into_response()
}

/// Query of `GET /auth/oauth/:provider`
#[derive(Debug, Deserialize)]
pub struct OauthStartQuery {
    #[serde(rename = "return")]
    pub return_to: Option<String>,
}

fn enabled_provider(config: &SignInConfig, name: &str) -> Option<Provider> {
    Provider::parse(name).filter(|p| config.provider(*p).enabled)
}

/// Send the browser to the provider
///
/// GET /auth/oauth/:provider?return=#/…
pub async fn oauth_start(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(query): Query<OauthStartQuery>,
    request: Request,
) -> Response {
    let ip = client_ip(&request);
    if let Some(limited) = rate_limited(&state, ip) {
        return limited;
    }
    let config = state.config.read().await.sign_in.clone();
    let Some(provider) = enabled_provider(&config, &name) else {
        return ApiError::NotFound(format!("Sign-in with {name} is not on")).into_response();
    };
    let headers = request.headers();
    let redirect_uri = signin::redirect_url(&config, headers, provider);
    let flow_state = signin::random_token();
    let verifier = signin::random_token();
    let binding = signin::random_token();
    let base = state
        .sign_in
        .endpoints
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let endpoints = oauth::endpoints(provider, base.as_deref());
    let url = oauth::authorize_url(
        &endpoints,
        provider,
        config.provider(provider),
        &redirect_uri,
        &flow_state,
        &verifier,
    );
    state.sign_in.start_flow(
        flow_state,
        provider,
        verifier,
        oauth::binding_digest(&binding),
        redirect_uri,
        oauth::safe_return(query.return_to.as_deref()),
    );
    let secure = session::is_secure(headers, config.base_url());
    (
        StatusCode::FOUND,
        AppendHeaders([
            (LOCATION, url),
            (SET_COOKIE, session::flow_cookie(Some(&binding), secure)),
        ]),
    )
        .into_response()
}

/// Query of the provider's callback
#[derive(Deserialize)]
pub struct OauthCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

/// Back from the provider: check the state and browser, trade the code, check the
/// allow-list, and land in the UI
///
/// GET /auth/oauth/:provider/callback
pub async fn oauth_callback(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Query(query): Query<OauthCallbackQuery>,
    request: Request,
) -> Response {
    let ip = client_ip(&request);
    if let Some(limited) = rate_limited(&state, ip) {
        return limited;
    }
    let headers = request.headers().clone();
    let config = state.config.read().await.sign_in.clone();
    let secure = session::is_secure(&headers, config.base_url());
    let clear_flow = session::flow_cookie(None, secure);
    // Relative, so it works under a reverse proxy's path prefix too
    let to_ui = |fragment: &str| format!("../../../{fragment}");
    let fail = |failure: oauth::Failure| {
        let location = to_ui(&format!("#/login?error={}", failure.code()));
        (
            StatusCode::FOUND,
            AppendHeaders([(LOCATION, location), (SET_COOKIE, clear_flow.clone())]),
        )
            .into_response()
    };

    let flow = query
        .state
        .as_deref()
        .and_then(|s| state.sign_in.take_flow(s));
    let binding = session::cookie(&headers, session::FLOW_COOKIE).map(oauth::binding_digest);
    let provider = enabled_provider(&config, &name);
    let flow = match (flow, provider) {
        (Some(flow), Some(provider))
            if flow.provider == provider && binding == Some(flow.binding) =>
        {
            flow
        }
        _ => {
            info!(provider = %name, "OAuth sign-in refused: unknown state or another browser");
            return fail(oauth::Failure::State);
        }
    };
    let provider = flow.provider;
    if let Some(error) = query.error {
        info!(provider = provider.as_str(), %error, "OAuth sign-in denied by the provider");
        return fail(oauth::Failure::Denied);
    }
    let Some(code) = query.code.filter(|c| !c.is_empty()) else {
        return fail(oauth::Failure::Denied);
    };

    let base = state
        .sign_in
        .endpoints
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let endpoints = oauth::endpoints(provider, base.as_deref());
    let provider_config = config.provider(provider);
    let account = match oauth::exchange(
        &state.sign_in.http,
        &endpoints,
        provider_config,
        &flow,
        &code,
    )
    .await
    {
        Ok(token) => oauth::account(&state.sign_in.http, &endpoints, provider, &token).await,
        Err(failure) => Err(failure),
    };
    let account = match account {
        Ok(account) => account,
        Err(failure) => {
            if let oauth::Failure::Exchange(why) = &failure {
                warn!(provider = provider.as_str(), %why, "OAuth sign-in failed");
            }
            return fail(failure);
        }
    };
    let Some(login) = oauth::allowed_login(&config, provider, &account) else {
        let failures = state.limiter.record_failure(ip);
        info!(
            provider = provider.as_str(),
            account = %account.login(),
            failures,
            "OAuth sign-in refused: not on the allowed users"
        );
        return fail(oauth::Failure::NotAllowed(account.login()));
    };
    let found = session::Session::new(
        provider.as_str(),
        &login,
        &account.display_name,
        config.session_hours,
        String::new(),
    );
    info!(provider = provider.as_str(), login = %login, "Signed in with OAuth");
    let cookie = session_cookie(&state, &config, &headers, &found);
    (
        StatusCode::FOUND,
        AppendHeaders([
            (LOCATION, to_ui(&flow.return_to)),
            (SET_COOKIE, cookie),
            (SET_COOKIE, clear_flow.clone()),
        ]),
    )
        .into_response()
}
