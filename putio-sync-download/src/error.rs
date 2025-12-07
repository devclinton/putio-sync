//! Error types for download operations

use thiserror::Error;

/// Download operation errors
#[derive(Error, Debug)]
pub enum DownloadError {
    /// API client error
    #[error("API client error: {0}")]
    ClientError(#[from] putio_sync_client::error::ClientError),

    /// Database error
    #[error("Database error: {0}")]
    DbError(#[from] putio_sync_db::error::DbError),

    /// HTTP error
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// API error
    #[error("API error: {0}")]
    ApiError(String),

    /// Internal error
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Result type for download operations
pub type DownloadResult<T> = Result<T, DownloadError>;
