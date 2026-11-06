//! Errors of the archive commands, one variant per exit code

use thiserror::Error;

/// Why an archive command stopped. The text is `<what failed>: <cause>`; the CLI puts
/// `archive <cmd>: ` in front. None of them carry a token or a URL with credentials.
#[derive(Error, Debug)]
pub enum ArchiveError {
    /// Bad arguments or config (exit 1)
    #[error("{0}")]
    Args(String),
    /// Prometheus was unreachable or returned an error (exit 2)
    #[error("{0}")]
    Prometheus(String),
    /// The database could not be read or copied (exit 3)
    #[error("{0}")]
    Database(String),
    /// The archive folder is missing, not writable, or the disk is full (exit 4)
    #[error("{0}")]
    Folder(String),
}

/// Result type of the archive commands
pub type ArchiveResult<T> = Result<T, ArchiveError>;

impl ArchiveError {
    /// The process exit code of contracts/archive-cli.md
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Args(_) => 1,
            Self::Prometheus(_) => 2,
            Self::Database(_) => 3,
            Self::Folder(_) => 4,
        }
    }

    pub(crate) fn args(what: &str, cause: impl std::fmt::Display) -> Self {
        Self::Args(format!("{what}: {cause}"))
    }

    pub(crate) fn prometheus(what: &str, cause: impl std::fmt::Display) -> Self {
        Self::Prometheus(format!("{what}: {cause}"))
    }

    pub(crate) fn database(what: &str, cause: impl std::fmt::Display) -> Self {
        Self::Database(format!("{what}: {cause}"))
    }

    pub(crate) fn folder(what: &str, cause: impl std::fmt::Display) -> Self {
        Self::Folder(format!("{what}: {cause}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_variant_has_its_exit_code() {
        assert_eq!(ArchiveError::args("a", "b").exit_code(), 1);
        assert_eq!(ArchiveError::prometheus("a", "b").exit_code(), 2);
        assert_eq!(ArchiveError::database("a", "b").exit_code(), 3);
        assert_eq!(ArchiveError::folder("a", "b").exit_code(), 4);
        assert_eq!(ArchiveError::folder("a", "b").to_string(), "a: b");
    }
}
