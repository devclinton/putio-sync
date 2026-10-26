//! Error types for daemon operations

use thiserror::Error;

/// Daemon operation errors
#[derive(Error, Debug)]
pub enum DaemonError {
    /// Client error
    #[error("API client error: {0}")]
    ClientError(#[from] putio_sync_client::error::ClientError),

    /// Database error
    #[error("Database error: {0}")]
    DbError(#[from] putio_sync_db::error::DbError),

    /// Download error
    #[error("Download error: {0}")]
    DownloadError(#[from] putio_sync_download::error::DownloadError),

    /// File manager error
    #[error("File manager error: {0}")]
    FileManagerError(#[from] putio_sync_file_manager::error::FileManagerError),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Config error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Internal error
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Result type for daemon operations
pub type DaemonResult<T> = Result<T, DaemonError>;
