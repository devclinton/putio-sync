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

use crate::error::ApiError;
use crate::middleware::ratelimit::client_ip;
use crate::AppState;
use axum::{
    extract::{Request, State},
    http::header::AUTHORIZATION,
    middleware::Next,
    response::Response,
};
use tracing::warn;

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
    // isn't valid text, which counts as a wrong token
    let provided = request
        .headers()
        .get(AUTHORIZATION)
        .map(|h| h.to_str().ok().map(extract_token));

    match provided {
        // No token at all is not a failed authentication: browsers ask for paths
        // nobody links to (robots.txt, touch icons) and must not lock their operator out
        None | Some(Some("")) => Err(ApiError::Unauthorized("Unauthorized".to_string())),
        Some(Some(token)) if token_matches(token, &expected_token) => Ok(next.run(request).await),
        _ => {
            let failures = state.limiter.record_failure(ip);
            warn!(ip = %ip, failures, "Authentication failed");
            Err(ApiError::Unauthorized("Unauthorized".to_string()))
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
