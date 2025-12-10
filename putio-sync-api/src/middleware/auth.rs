//! Authentication middleware
//!
//! This module provides authentication middleware for API endpoints.
//!
//! When an API auth token is configured, all requests must include:
//! - Authorization header with format: "Bearer <token>"
//!
//! If no auth token is configured, all requests are allowed (development mode).

use crate::error::ApiError;
use crate::AppState;
use axum::{
    extract::{Request, State},
    http::header::AUTHORIZATION,
    middleware::Next,
    response::Response,
};

/// Authentication middleware
///
/// This middleware checks for valid authentication tokens in requests.
///
/// - If `api.auth_token` is set in config, validates the Authorization header
/// - If not set, allows all requests (development mode)
/// - Supports Bearer token format: "Bearer <token>"
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
    // Get auth token from config
    let config = state.config.read().await;
    let expected_token = &config.api.auth_token;

    // If no auth token is configured, allow all requests (development mode)
    if expected_token.is_none() {
        return Ok(next.run(request).await);
    }

    // Extract Authorization header
    let auth_header = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    match auth_header {
        Some(header) => {
            // Check if token is valid
            if validate_token(header, expected_token.as_deref().unwrap()) {
                Ok(next.run(request).await)
            } else {
                Err(ApiError::Unauthorized(
                    "Invalid authentication token".to_string(),
                ))
            }
        }
        None => Err(ApiError::Unauthorized(
            "Missing Authorization header. Use format: 'Authorization: Bearer <token>'".to_string(),
        )),
    }
}

/// Validate an authentication token
///
/// Supports Bearer token format: "Bearer <token>"
/// Performs constant-time comparison to prevent timing attacks
fn validate_token(auth_header: &str, expected_token: &str) -> bool {
    // Extract token from "Bearer <token>" format
    let provided_token = if let Some(token) = auth_header.strip_prefix("Bearer ") {
        token.trim()
    } else {
        // Also support raw token without "Bearer " prefix for backward compatibility
        auth_header.trim()
    };

    // Constant-time comparison to prevent timing attacks
    constant_time_compare(provided_token.as_bytes(), expected_token.as_bytes())
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
    fn test_constant_time_compare() {
        assert!(constant_time_compare(b"hello", b"hello"));
        assert!(!constant_time_compare(b"hello", b"world"));
        assert!(!constant_time_compare(b"hello", b"hello!"));
        assert!(!constant_time_compare(b"hello!", b"hello"));
    }
}
