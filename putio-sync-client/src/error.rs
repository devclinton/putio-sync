//! Error types for putio-sync-client
//!
//! This module defines custom error types for API client operations.

use thiserror::Error;

/// The main error type for Put.io API client operations.
#[derive(Error, Debug)]
pub enum ClientError {
    /// Error occurred while making an API request
    #[error("API error: {0}")]
    ApiError(String),

    /// Error occurred with HTTP client
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Error occurred during authentication
    #[error("Authentication error: {0}")]
    AuthError(String),

    /// Error occurred during serialization/deserialization
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Error during I/O operations (file upload)
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Generic error for other cases
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Convenience type alias for Results that return `ClientError`
pub type ClientResult<T> = Result<T, ClientError>;

impl ClientError {
    /// Create an AuthError with a helpful suggestion
    pub fn auth_error_with_suggestion(msg: &str) -> Self {
        ClientError::AuthError(format!(
            "{}

Troubleshooting:
- Verify your OAuth token is valid at https://app.put.io/settings/account
- Or try logging in again
- Check that your token hasn't expired or been revoked",
            msg
        ))
    }

    /// Create an ApiError for network issues with helpful suggestion
    pub fn network_error(msg: &str) -> Self {
        ClientError::ApiError(format!(
            "{}

Troubleshooting:
- Check your internet connection
- Verify Put.io service status at https://status.put.io
- Try again in a few moments
- If the problem persists, check firewall/proxy settings",
            msg
        ))
    }
}

// Implement conversion from serde_json::Error
impl From<serde_json::Error> for ClientError {
    fn from(err: serde_json::Error) -> Self {
        ClientError::SerializationError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_api_error() {
        let err = ClientError::ApiError("test error".to_string());
        assert_eq!(err.to_string(), "API error: test error");
    }

    #[test]
    fn test_auth_error() {
        let err = ClientError::AuthError("invalid token".to_string());
        assert_eq!(err.to_string(), "Authentication error: invalid token");
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err: ClientError = io_err.into();
        assert!(matches!(err, ClientError::IoError(_)));
    }

    #[test]
    fn test_serde_json_error_conversion() {
        let json_err =
            serde_json::from_str::<serde_json::Value>("{invalid json}").expect_err("should fail");
        let err: ClientError = json_err.into();
        assert!(matches!(err, ClientError::SerializationError(_)));
    }

    #[test]
    fn test_auth_error_with_suggestion() {
        let err = ClientError::auth_error_with_suggestion("Invalid token");
        let msg = err.to_string();
        assert!(msg.contains("Invalid token"));
        assert!(msg.contains("Troubleshooting"));
        assert!(msg.contains("https://app.put.io/settings/account"));
    }

    #[test]
    fn test_network_error() {
        let err = ClientError::network_error("Connection timeout");
        let msg = err.to_string();
        assert!(msg.contains("Connection timeout"));
        assert!(msg.contains("Troubleshooting"));
        assert!(msg.contains("internet connection"));
        assert!(msg.contains("https://status.put.io"));
    }
}
