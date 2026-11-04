//! Sign-in routes for the web UI (feature 004, FR-006, FR-007)
//!
//! Both are public (no token needed):
//! - GET  /auth/status - Whether an API token is configured (and `setup_required: false`),
//!   and the Tailscale identity that lets this request in without one (feature 036)
//! - POST /auth/login  - Check a token: 204 on match, 401 otherwise, 429 when rate limited
//!
//! The UI keeps the token itself and sends it as `Authorization: Bearer` afterwards, so
//! there is no session to create. The request body is never logged.

use crate::error::ApiError;
use crate::middleware::auth::{not_configured, tailscale_identity, token_matches};
use crate::middleware::ratelimit::client_ip;
use axum::{
    extract::{ConnectInfo, FromRequest, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tracing::warn;

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
    let (token_configured, tailscale_sign_in) = {
        let config = state.config.read().await;
        (
            config.api.auth_token.is_some(),
            config.tailscale.identity_sign_in,
        )
    };
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
