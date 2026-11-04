//! Sign-in routes for the web UI (feature 004, FR-006, FR-007)
//!
//! Both are public (no token needed):
//! - GET  /auth/status - Whether an API token is configured (and `setup_required: false`)
//! - POST /auth/login  - Check a token: 204 on match, 401 otherwise, 429 when rate limited
//!
//! The UI keeps the token itself and sends it as `Authorization: Bearer` afterwards, so
//! there is no session to create. The request body is never logged.

use crate::error::ApiError;
use crate::middleware::auth::{not_configured, token_matches};
use crate::middleware::ratelimit::client_ip;
use axum::{
    extract::{FromRequest, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::warn;

// Re-export AppState from parent
pub use crate::AppState;

/// Response of `GET /auth/status`
#[derive(Debug, Serialize)]
pub struct AuthStatusResponse {
    pub token_configured: bool,
    /// Always false here; setup mode answers true (feature 013)
    pub setup_required: bool,
}

/// Body of `POST /auth/login`. Deliberately not `Debug`, so it can't end up in a log.
#[derive(Deserialize)]
pub struct LoginRequest {
    pub token: String,
}

/// Whether an API token is configured
///
/// GET /auth/status
pub async fn auth_status(State(state): State<AppState>) -> Json<AuthStatusResponse> {
    let token_configured = state.config.read().await.api.auth_token.is_some();
    Json(AuthStatusResponse {
        token_configured,
        setup_required: false,
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
