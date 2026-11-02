//! Error types for download operations

use putio_sync_telemetry::FailReason;
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

    /// The download was stopped on request (pause or the Pause NAS downloads switch).
    /// Not retried and not a failure: the partial file is kept for resume (feature 039).
    #[error("Download stopped")]
    Stopped,
}

/// Result type for download operations
pub type DownloadResult<T> = Result<T, DownloadError>;

impl DownloadError {
    /// Metric reason for a failed download
    pub fn fail_reason(&self) -> FailReason {
        match self {
            DownloadError::HttpError(e) if e.status().is_some() => FailReason::Http,
            DownloadError::HttpError(_) => FailReason::Network,
            DownloadError::IoError(_) => FailReason::Io,
            DownloadError::ApiError(msg)
                if msg.starts_with("Size mismatch") || msg.starts_with("Checksum mismatch") =>
            {
                FailReason::Integrity
            }
            DownloadError::ApiError(msg) if msg.starts_with("HTTP error") => FailReason::Http,
            DownloadError::ApiError(msg) if msg.starts_with("Download stalled") => {
                FailReason::Network
            }
            _ => FailReason::Other,
        }
    }
}
