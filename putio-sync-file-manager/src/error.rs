//! Error types for file management operations

use thiserror::Error;

/// File management operation errors
#[derive(Error, Debug)]
pub enum FileManagerError {
    /// Client error
    #[error("API client error: {0}")]
    ClientError(#[from] putio_sync_client::error::ClientError),

    /// Database error
    #[error("Database error: {0}")]
    DbError(#[from] putio_sync_db::error::DbError),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Watch error
    #[error("Filesystem watch error: {0}")]
    WatchError(#[from] notify::Error),

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Internal error
    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Result type for file management operations
pub type FileManagerResult<T> = Result<T, FileManagerError>;
