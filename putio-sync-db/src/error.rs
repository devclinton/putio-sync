//! Error types for putio-sync-db

use thiserror::Error;

/// Database error type
#[derive(Error, Debug)]
pub enum DbError {
    /// Database operation error
    #[error("Database error: {0}")]
    DatabaseError(String),

    /// SQLx error
    #[error("SQL error: {0}")]
    SqlxError(#[from] sqlx::Error),

    /// IO error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// Convenience type alias for Results that return `DbError`
pub type DbResult<T> = Result<T, DbError>;

/// Build a `map_err` closure that counts the error under `op` and wraps it as
/// `DatabaseError("<context>: <error>")`.
pub(crate) fn tracked<E: std::fmt::Display>(
    op: putio_sync_telemetry::DbOp,
    context: &'static str,
) -> impl Fn(E) -> DbError {
    move |e| {
        putio_sync_telemetry::db::error(op);
        DbError::DatabaseError(format!("{}: {}", context, e))
    }
}

impl From<serde_json::Error> for DbError {
    fn from(err: serde_json::Error) -> Self {
        DbError::SerializationError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_error() {
        let err = DbError::DatabaseError("test error".to_string());
        assert_eq!(err.to_string(), "Database error: test error");
    }
}
