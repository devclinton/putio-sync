//! Authentication middleware
//!
//! This module provides authentication middleware for API endpoints.
//!
//! Every protected request must include:
//! - Authorization header with format: "Bearer <token>"
//!
//! A token is always required. When none is configured, every protected request is
//! refused with a body that names the setting. Failures are counted per client IP and
//! rate limited (see [`crate::middleware::ratelimit`]).
//!
//! With `[tailscale] identity_sign_in` on, a request without a valid token is also let
//! in when its Tailscale identity is on `allowed_users` (feature 036). The identity is
//! attached to the request as a [`putio_sync_tailscale::Identity`] extension.

use crate::error::ApiError;
use crate::middleware::ratelimit::client_ip;
use crate::AppState;
use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header::AUTHORIZATION, HeaderMap},
    middleware::Next,
    response::Response,
};
use putio_sync_tailscale::{Identity, Lookup};
use std::net::SocketAddr;
use tracing::{info, warn};

/// `tailscale serve`'s header with the user's login
pub const TAILSCALE_USER_LOGIN: &str = "Tailscale-User-Login";
/// `tailscale serve`'s header with the user's display name
pub const TAILSCALE_USER_NAME: &str = "Tailscale-User-Name";

/// Authentication middleware
///
/// This middleware checks for valid authentication tokens in requests.
///
/// - Refuses with 429 while the client IP is rate limited, before comparing anything
/// - Refuses with 401 when `api.auth_token` is not configured
/// - Otherwise validates the Authorization header ("Bearer <token>", or the raw token)
///
/// Only a token that doesn't match counts toward the limiter. A request without one
/// (or when no token is configured) gets its 401 without being counted.
///
/// # Examples
///
/// ```ignore
/// use axum::Router;
/// use axum::middleware;
/// use putio_sync_api::middleware::auth::require_auth;
///
/// let app = Router::new()
///     .layer(middleware::from_fn_with_state(state.clone(), require_auth));
/// ```
pub async fn require_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let ip = client_ip(&request);
    if let Err(secs) = state.limiter.check(ip) {
        warn!(ip = %ip, retry_after_seconds = secs, "Authentication rate limited");
        return Err(ApiError::TooManyRequests {
            retry_after_seconds: secs,
        });
    }

    // Get auth token from config. With none configured there is nothing to guess, so
    // the refusal is not counted
    let expected_token = state.config.read().await.api.auth_token.clone();
    let Some(expected_token) = expected_token else {
        return Err(not_configured());
    };

    // Extract the token from the Authorization header; `Some(None)` is a header that
    // isn't valid text, which counts as a wrong token. `None` here is no token at all
    let wrong_token = match request
        .headers()
        .get(AUTHORIZATION)
        .map(|h| h.to_str().ok().map(extract_token))
    {
        None | Some(Some("")) => false,
        Some(Some(token)) if token_matches(token, &expected_token) => {
            return Ok(next.run(request).await)
        }
        _ => true,
    };

    // Tailscale sign-in: after the token, so scripts never wait on tailscaled
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| *addr);
    if let Some(identity) = tailscale_identity(&state, request.headers(), peer).await {
        let mut request = request;
        request.extensions_mut().insert(identity);
        return Ok(next.run(request).await);
    }

    // No token at all is not a failed authentication: browsers ask for paths nobody
    // links to (robots.txt, touch icons) and must not lock their operator out
    if wrong_token {
        let failures = state.limiter.record_failure(ip);
        warn!(ip = %ip, failures, "Authentication failed");
    }
    Err(ApiError::Unauthorized("Unauthorized".to_string()))
}

/// The Tailscale identity that lets this request in, when identity sign-in is on and
/// the user is on the allow-list. Counted on the metrics; refusals are logged.
///
/// `peer` is the TCP peer from `ConnectInfo`. Without it there is no identity; it is
/// never assumed to be loopback.
pub async fn tailscale_identity(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
) -> Option<Identity> {
    let ts = state.config.read().await.tailscale.clone();
    if !ts.identity_sign_in {
        return None;
    }
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let lookup = state
        .tailscale
        .identity_for(
            peer,
            header(TAILSCALE_USER_LOGIN),
            header(TAILSCALE_USER_NAME),
        )
        .await;
    match lookup {
        Lookup::None => None,
        Lookup::Found(identity) if ts.allows(&identity.login) => {
            putio_sync_telemetry::tailscale::record_identity(identity.via.as_str(), true);
            Some(identity)
        }
        Lookup::Found(identity) => {
            putio_sync_telemetry::tailscale::record_identity(identity.via.as_str(), false);
            info!(
                login = %identity.login,
                via = identity.via.as_str(),
                "Tailscale sign-in refused: not on the allowed users"
            );
            None
        }
        Lookup::Refused { via, reason } => {
            putio_sync_telemetry::tailscale::record_identity(via.as_str(), false);
            info!(via = via.as_str(), %reason, "Tailscale sign-in refused");
            None
        }
    }
}

/// The 401 for a server with no token configured
pub fn not_configured() -> ApiError {
    ApiError::Unauthorized("API token not configured".to_string())
        .with_details("Set [api] auth_token in the config file or PUTIO_SYNC_API_TOKEN")
}

/// Whether `provided` equals `expected`, compared in constant time (for `/auth/login`)
pub fn token_matches(provided: &str, expected: &str) -> bool {
    constant_time_compare(provided.as_bytes(), expected.as_bytes())
}

/// The token in an Authorization header value: "Bearer <token>", or the raw token for
/// backward compatibility. Empty when the header carries none.
fn extract_token(auth_header: &str) -> &str {
    let auth_header = auth_header.trim();
    if auth_header == "Bearer" {
        return "";
    }
    auth_header
        .strip_prefix("Bearer ")
        .unwrap_or(auth_header)
        .trim()
}

/// Validate an authentication token
///
/// Supports Bearer token format: "Bearer <token>"
/// Performs constant-time comparison to prevent timing attacks
#[cfg(test)]
fn validate_token(auth_header: &str, expected_token: &str) -> bool {
    token_matches(extract_token(auth_header), expected_token)
}

/// Constant-time string comparison to prevent timing attacks
fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut result = 0u8;
    for (byte_a, byte_b) in a.iter().zip(b.iter()) {
        result |= byte_a ^ byte_b;
    }
    result == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_token_with_bearer() {
        let expected = "my-secret-token";
        assert!(validate_token("Bearer my-secret-token", expected));
        assert!(validate_token("Bearer  my-secret-token  ", expected)); // with spaces
        assert!(!validate_token("Bearer wrong-token", expected));
    }

    #[test]
    fn test_validate_token_without_bearer() {
        let expected = "my-secret-token";
        assert!(validate_token("my-secret-token", expected));
        assert!(!validate_token("wrong-token", expected));
    }

    #[test]
    fn test_extract_token() {
        assert_eq!(extract_token("Bearer abc"), "abc");
        assert_eq!(extract_token(" abc "), "abc");
        assert_eq!(extract_token("Bearer"), "");
        assert_eq!(extract_token("Bearer   "), "");
        assert_eq!(extract_token(""), "");
    }

    #[test]
    fn test_token_matches() {
        assert!(token_matches("abc", "abc"));
        assert!(!token_matches("abc", "abd"));
        assert!(!token_matches("", "abc"));
    }

    #[test]
    fn test_constant_time_compare() {
        assert!(constant_time_compare(b"hello", b"hello"));
        assert!(!constant_time_compare(b"hello", b"world"));
        assert!(!constant_time_compare(b"hello", b"hello!"));
        assert!(!constant_time_compare(b"hello!", b"hello"));
    }
}
