//! Logging infrastructure for putio-sync-core
//!
//! This module provides structured logging using the `tracing` crate with
//! support for multiple log levels and environment variable configuration.

use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initializes the tracing subscriber with the specified log level.
///
/// # Arguments
///
/// * `level` - The log level to use (e.g., "trace", "debug", "info", "warn", "error")
///
/// # Examples
///
/// ```no_run
/// use putio_sync_core::logging;
///
/// // Initialize with info level
/// logging::init("info");
/// ```
pub fn init(level: &str) {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true).with_thread_ids(false))
        .init();
}

/// Initializes the tracing subscriber using the `RUST_LOG` environment variable.
///
/// If `RUST_LOG` is not set, defaults to "info" level.
///
/// # Examples
///
/// ```no_run
/// use putio_sync_core::logging;
///
/// // Initialize with RUST_LOG env var or default to info
/// logging::init_from_env();
/// ```
pub fn init_from_env() {
    init("info");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_with_valid_level() {
        // This test just ensures the function doesn't panic
        // In a real scenario, we'd capture and verify logs
        // For now, we just test that init completes without error

        // Note: We can't actually test this easily without more setup
        // because tracing can only be initialized once per process
        // This is a placeholder for future testing with proper isolation
    }

    #[test]
    fn test_logging_levels() {
        // Test that we can create filters for each level
        let levels = ["trace", "debug", "info", "warn", "error"];

        for level in levels {
            let filter = EnvFilter::try_new(level);
            assert!(filter.is_ok(), "Failed to create filter for level: {}", level);
        }
    }
}
