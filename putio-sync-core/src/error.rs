//! Error types for putio-sync-core
//!
//! This module defines custom error types using `thiserror` for
//! comprehensive error handling across the crate.

use thiserror::Error;

/// The main error type for putio-sync-core operations.
///
/// This enum provides variants for all error categories that can
/// occur during core operations.
#[derive(Error, Debug)]
pub enum CoreError {
    /// Error occurred while making an API request
    #[error("API error: {0}")]
    ApiError(String),

    /// Error occurred with HTTP client
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Error occurred while reading or writing configuration
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Error occurred during database operations
    #[error("Database error: {0}")]
    DatabaseError(String),

    /// Error occurred during I/O operations
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Error occurred during authentication
    #[error("Authentication error: {0}")]
    AuthError(String),

    /// Error occurred during serialization/deserialization
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Error occurred during path operations
    #[error("Path error: {0}")]
    PathError(String),

    /// Error for invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Generic error for other cases
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Convenience type alias for Results that return `CoreError`
pub type CoreResult<T> = Result<T, CoreError>;

// Implement conversion from serde_json::Error
impl From<serde_json::Error> for CoreError {
    fn from(err: serde_json::Error) -> Self {
        CoreError::SerializationError(err.to_string())
    }
}

// Implement conversion from toml parsing errors
impl From<toml::de::Error> for CoreError {
    fn from(err: toml::de::Error) -> Self {
        CoreError::ConfigError(err.to_string())
    }
}

// Implement conversion from toml serialization errors
impl From<toml::ser::Error> for CoreError {
    fn from(err: toml::ser::Error) -> Self {
        CoreError::ConfigError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn test_api_error() {
        let err = CoreError::ApiError("test error".to_string());
        assert_eq!(err.to_string(), "API error: test error");
    }

    #[test]
    fn test_config_error() {
        let err = CoreError::ConfigError("invalid config".to_string());
        assert_eq!(err.to_string(), "Configuration error: invalid config");
    }

    #[test]
    fn test_auth_error() {
        let err = CoreError::AuthError("invalid token".to_string());
        assert_eq!(err.to_string(), "Authentication error: invalid token");
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err: CoreError = io_err.into();
        assert!(matches!(err, CoreError::IoError(_)));
    }

    #[test]
    fn test_serde_json_error_conversion() {
        let json_err =
            serde_json::from_str::<serde_json::Value>("{invalid json}").expect_err("should fail");
        let err: CoreError = json_err.into();
        assert!(matches!(err, CoreError::SerializationError(_)));
    }

    #[test]
    fn test_toml_error_conversion() {
        let toml_err =
            toml::from_str::<toml::Value>("invalid = toml = syntax").expect_err("should fail");
        let err: CoreError = toml_err.into();
        assert!(matches!(err, CoreError::ConfigError(_)));
    }

    #[test]
    fn test_result_type() {
        fn example_function() -> CoreResult<String> {
            Ok("success".to_string())
        }

        let result = example_function();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
    }

    #[test]
    fn test_result_type_error() {
        fn example_function() -> CoreResult<String> {
            Err(CoreError::InvalidInput("test".to_string()))
        }

        let result = example_function();
        assert!(result.is_err());
    }
}
