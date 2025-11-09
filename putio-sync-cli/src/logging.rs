//! Logging configuration for CLI
//!
//! Maps CLI verbosity flags to tracing log levels with colored output support.

use tracing::Level;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize logging based on verbosity level
///
/// # Arguments
///
/// * `verbosity` - Verbosity level from CLI (0-3)
/// * `quiet` - Whether quiet mode is enabled
///
/// # Verbosity Mapping
///
/// - 0 (default): WARN level
/// - 1 (-v): INFO level
/// - 2 (-vv): DEBUG level
/// - 3+ (-vvv): TRACE level
/// - quiet: ERROR level only
///
/// # Examples
///
/// ```no_run
/// use putio_sync_cli::logging;
///
/// // Default level (WARN)
/// logging::init(0, false);
///
/// // Info level
/// logging::init(1, false);
///
/// // Quiet mode (ERROR only)
/// logging::init(0, true);
/// ```
pub fn init(verbosity: u8, quiet: bool) {
    let level = if quiet {
        Level::ERROR
    } else {
        match verbosity {
            0 => Level::WARN,
            1 => Level::INFO,
            2 => Level::DEBUG,
            _ => Level::TRACE,
        }
    };

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level.as_str()));

    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_target(false) // Don't show target module
                .with_thread_ids(false)
                .with_ansi(supports_color()) // Colored output if supported
                .with_level(true),
        )
        .init();
}

/// Check if the terminal supports colored output
fn supports_color() -> bool {
    // Check if we're in a TTY and not in certain CI environments
    atty::is(atty::Stream::Stdout)
        && std::env::var("NO_COLOR").is_err()
        && std::env::var("TERM").as_deref() != Ok("dumb")
}

/// Map verbosity level to tracing Level
///
/// # Arguments
///
/// * `verbosity` - Verbosity count from CLI
/// * `quiet` - Whether quiet mode is enabled
///
/// # Returns
///
/// The corresponding tracing Level
#[cfg(test)]
pub(crate) fn verbosity_to_level(verbosity: u8, quiet: bool) -> Level {
    if quiet {
        Level::ERROR
    } else {
        match verbosity {
            0 => Level::WARN,
            1 => Level::INFO,
            2 => Level::DEBUG,
            _ => Level::TRACE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verbosity_mapping() {
        assert_eq!(verbosity_to_level(0, false), Level::WARN);
        assert_eq!(verbosity_to_level(1, false), Level::INFO);
        assert_eq!(verbosity_to_level(2, false), Level::DEBUG);
        assert_eq!(verbosity_to_level(3, false), Level::TRACE);
        assert_eq!(verbosity_to_level(4, false), Level::TRACE);
    }

    #[test]
    fn test_quiet_mode() {
        assert_eq!(verbosity_to_level(0, true), Level::ERROR);
        assert_eq!(verbosity_to_level(1, true), Level::ERROR);
        assert_eq!(verbosity_to_level(2, true), Level::ERROR);
    }

    #[test]
    fn test_supports_color() {
        // This test just ensures the function doesn't panic
        // Actual result depends on environment
        let _ = supports_color();
    }
}
