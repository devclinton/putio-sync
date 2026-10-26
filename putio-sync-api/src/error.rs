//! Error types for the putio-sync API

use axum::{
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::fmt;

/// API error type
#[derive(Debug)]
pub enum ApiError {
    /// Core library error
    Core(putio_sync_core::error::CoreError),
    /// Client error
    Client(putio_sync_client::error::ClientError),
    /// Not found error
    NotFound(String),
    /// Bad request error
    BadRequest(String),
    /// Unauthorized error
    Unauthorized(String),
    /// Internal server error
    Internal(String),
    /// Internal error (alias for Internal)
    InternalError(String),
    /// Conflict with the current state (409)
    Conflict(String),
    /// A dependency the route needs is missing (503)
    ServiceUnavailable(String),
    /// Too many failed authentication attempts (429 with `Retry-After`)
    TooManyRequests {
        /// Seconds until the client may try again
        retry_after_seconds: u64,
    },
    /// Another error with `details` filled in the response body
    WithDetails(Box<ApiError>, String),
}

/// API error response body
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Error message
    pub error: String,
    /// Error details (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Core(e) => write!(f, "Core error: {}", e),
            ApiError::Client(e) => write!(f, "Client error: {}", e),
            ApiError::NotFound(msg) => write!(f, "Not found: {}", msg),
            ApiError::BadRequest(msg) => write!(f, "Bad request: {}", msg),
            ApiError::Unauthorized(msg) => write!(f, "Unauthorized: {}", msg),
            ApiError::Internal(msg) => write!(f, "Internal error: {}", msg),
            ApiError::InternalError(msg) => write!(f, "Internal error: {}", msg),
            ApiError::Conflict(msg) => write!(f, "Conflict: {}", msg),
            ApiError::ServiceUnavailable(msg) => write!(f, "Service unavailable: {}", msg),
            ApiError::TooManyRequests {
                retry_after_seconds,
            } => write!(f, "Too many requests: retry after {}s", retry_after_seconds),
            ApiError::WithDetails(inner, details) => write!(f, "{} ({})", inner, details),
        }
    }
}

impl ApiError {
    /// Attach `details` to the error body, for example the setting to change
    pub fn with_details(self, details: impl Into<String>) -> Self {
        ApiError::WithDetails(Box::new(self), details.into())
    }
}

impl std::error::Error for ApiError {}

impl From<putio_sync_core::error::CoreError> for ApiError {
    fn from(err: putio_sync_core::error::CoreError) -> Self {
        ApiError::Core(err)
    }
}

impl From<putio_sync_client::error::ClientError> for ApiError {
    fn from(err: putio_sync_client::error::ClientError) -> Self {
        ApiError::Client(err)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // The outermost details win; nested ones are dropped
        let mut inner = self;
        let mut details = None;
        while let ApiError::WithDetails(next, d) = inner {
            details.get_or_insert(d);
            inner = *next;
        }

        let (status, error_message) = match &inner {
            ApiError::Core(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            ApiError::Client(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            ApiError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            ApiError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            ApiError::InternalError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            ApiError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone()),
            ApiError::ServiceUnavailable(msg) => (StatusCode::SERVICE_UNAVAILABLE, msg.clone()),
            ApiError::TooManyRequests { .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                "Too many failed authentication attempts".to_string(),
            ),
            ApiError::WithDetails(_, _) => unreachable!("details unwrapped above"),
        };

        let body = ErrorResponse {
            error: error_message,
            details,
        };

        let mut response = (status, Json(body)).into_response();
        match &inner {
            ApiError::Unauthorized(_) => {
                response
                    .headers_mut()
                    .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
            }
            ApiError::TooManyRequests {
                retry_after_seconds,
            } => {
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, HeaderValue::from(*retry_after_seconds));
            }
            _ => {}
        }
        response
    }
}

/// Result type for API operations
pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ApiError::NotFound("File not found".to_string());
        assert_eq!(err.to_string(), "Not found: File not found");

        let err = ApiError::BadRequest("Invalid ID".to_string());
        assert_eq!(err.to_string(), "Bad request: Invalid ID");

        let err = ApiError::Unauthorized("Invalid token".to_string());
        assert_eq!(err.to_string(), "Unauthorized: Invalid token");

        let err = ApiError::Internal("Something went wrong".to_string());
        assert_eq!(err.to_string(), "Internal error: Something went wrong");
    }

    #[test]
    fn test_error_response_serialization() {
        let response = ErrorResponse {
            error: "Test error".to_string(),
            details: Some("Test details".to_string()),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("Test error"));
        assert!(json.contains("Test details"));
    }

    #[test]
    fn test_error_response_without_details() {
        let response = ErrorResponse {
            error: "Test error".to_string(),
            details: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("Test error"));
        assert!(!json.contains("details"));
    }

    #[test]
    fn test_conflict_is_409() {
        let response = ApiError::Conflict("busy".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[test]
    fn test_service_unavailable_is_503() {
        let response = ApiError::ServiceUnavailable("no db".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn test_too_many_requests_sets_retry_after() {
        let response = ApiError::TooManyRequests {
            retry_after_seconds: 41,
        }
        .into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[header::RETRY_AFTER], "41");
    }

    #[test]
    fn test_unauthorized_sets_www_authenticate() {
        let response = ApiError::Unauthorized("Unauthorized".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
    }

    #[test]
    fn test_with_details_keeps_status_and_headers() {
        let response = ApiError::Unauthorized("API token not configured".to_string())
            .with_details("Set it: here")
            .into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
    }

    #[tokio::test]
    async fn test_with_details_fills_body() {
        let response = ApiError::Conflict("File is queued or downloading".to_string())
            .with_details("Skip or cancel it first")
            .into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: ErrorResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body.error, "File is queued or downloading");
        assert_eq!(body.details.as_deref(), Some("Skip or cancel it first"));
    }

    #[tokio::test]
    async fn test_too_many_requests_body() {
        let response = ApiError::TooManyRequests {
            retry_after_seconds: 5,
        }
        .into_response();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            bytes.as_ref(),
            br#"{"error":"Too many failed authentication attempts"}"#
        );
    }
}
