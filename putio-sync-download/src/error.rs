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

    /// The download was stopped on request (pause or the Pause local downloads switch).
    /// Not retried and not a failure: the partial file is kept for resume (feature 039).
    #[error("Download stopped")]
    Stopped,
}

/// Result type for download operations
pub type DownloadResult<T> = Result<T, DownloadError>;

/// The status code in an `HTTP error 404 Not Found: ...` or `API error (404 ...): ...`
/// message, when there is one
fn status_in_message(msg: &str) -> Option<u16> {
    let rest = msg
        .strip_prefix("HTTP error ")
        .or_else(|| msg.strip_prefix("API error ("))?;
    rest.get(..3)?.parse().ok()
}

/// A 4xx status that repeating the request cannot change. Timeouts (408) and rate
/// limits (429) pass.
fn permanent_status(status: u16) -> bool {
    (400..500).contains(&status) && status != 408 && status != 429
}

/// An I/O error that waiting does not fix: the disk is full, the path is not allowed or
/// not usable
fn io_is_permanent(e: &std::io::Error) -> bool {
    use std::io::ErrorKind::*;
    matches!(
        e.kind(),
        StorageFull
            | QuotaExceeded
            | PermissionDenied
            | ReadOnlyFilesystem
            | InvalidFilename
            | NotADirectory
            | IsADirectory
            | FileTooLarge
    )
}

impl DownloadError {
    /// The disk (or the user's quota) has no room left
    pub fn is_storage_full(&self) -> bool {
        matches!(self, DownloadError::IoError(e)
            if matches!(e.kind(), std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded))
    }

    /// Whether trying again later can succeed: a dropped connection, a stall, a server
    /// error. `false` for what repeating cannot fix: 401/403/404, a full or read-only
    /// disk, a bad path, a stop. A transient failure is retried with backoff and, once
    /// the retries are used up, queued again by a later sync.
    pub fn is_transient(&self) -> bool {
        use putio_sync_client::error::ClientError;
        match self {
            DownloadError::Stopped
            | DownloadError::InvalidInput(_)
            | DownloadError::InternalError(_) => false,
            DownloadError::IoError(e) => !io_is_permanent(e),
            DownloadError::DbError(_) => true,
            DownloadError::HttpError(e) => {
                !e.status().is_some_and(|s| permanent_status(s.as_u16()))
            }
            // The file is not the size Put.io said; another try gets the same bytes
            DownloadError::ApiError(msg) if msg.starts_with("Size mismatch") => false,
            DownloadError::ApiError(msg) => !status_in_message(msg).is_some_and(permanent_status),
            DownloadError::ClientError(e) => match e {
                ClientError::AuthError(_) | ClientError::InternalError(_) => false,
                ClientError::IoError(e) => !io_is_permanent(e),
                ClientError::HttpError(e) => {
                    !e.status().is_some_and(|s| permanent_status(s.as_u16()))
                }
                ClientError::ApiError(msg) => {
                    !(e.is_not_found()
                        || e.is_unauthorized()
                        || msg.starts_with("Access forbidden")
                        || status_in_message(msg).is_some_and(permanent_status))
                }
                ClientError::SerializationError(_) => true,
            },
        }
    }

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
            DownloadError::ApiError(msg)
                if msg.starts_with("Download stalled")
                    || msg.starts_with("Incomplete download") =>
            {
                FailReason::Network
            }
            _ => FailReason::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};

    #[test]
    fn client_errors_are_permanent_but_timeouts_and_server_errors_are_not() {
        let api = |m: &str| DownloadError::ApiError(m.to_string());
        assert!(!api("HTTP error 404 Not Found: - Response: gone").is_transient());
        assert!(!api("HTTP error 403 Forbidden: - Response: no").is_transient());
        assert!(!api("HTTP error 401 Unauthorized: - Response: no").is_transient());
        assert!(api("HTTP error 429 Too Many Requests: - Response: slow").is_transient());
        assert!(api("HTTP error 408 Request Timeout: - Response: slow").is_transient());
        assert!(api("HTTP error 503 Service Unavailable: - Response: busy").is_transient());
        assert!(api("Download stalled - no data received for 60 seconds").is_transient());
    }

    #[test]
    fn a_full_disk_is_permanent_and_named() {
        let full = DownloadError::IoError(Error::new(ErrorKind::StorageFull, "no space"));
        assert!(full.is_storage_full());
        assert!(!full.is_transient());
        let reset = DownloadError::IoError(Error::new(ErrorKind::ConnectionReset, "reset"));
        assert!(!reset.is_storage_full());
        assert!(reset.is_transient());
        let denied = DownloadError::IoError(Error::new(ErrorKind::PermissionDenied, "no"));
        assert!(!denied.is_transient());
    }

    #[test]
    fn stopped_and_invalid_input_are_not_retried() {
        assert!(!DownloadError::Stopped.is_transient());
        assert!(!DownloadError::InvalidInput("x".into()).is_transient());
    }

    #[test]
    fn put_io_not_found_and_unauthorized_are_permanent() {
        use putio_sync_client::error::ClientError;
        let not_found =
            DownloadError::ClientError(ClientError::ApiError("Resource not found.".to_string()));
        assert!(!not_found.is_transient());
        let down = DownloadError::ClientError(ClientError::ApiError(
            "API error (502 Bad Gateway): x".to_string(),
        ));
        assert!(down.is_transient());
    }
}
