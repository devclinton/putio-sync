//! Logging configuration and initialization
//!
//! This module provides flexible logging configuration with support for:
//! - Console and file output
//! - Multiple console formats (compact, pretty, JSON)
//! - Daily log rotation
//! - Module-specific log level filtering
//! - Configurable log levels

use crate::error::CoreError;
use std::path::PathBuf;
use tracing_appender::{non_blocking, rolling};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

/// Result type for logging operations
pub type LoggingResult<T> = Result<T, CoreError>;

// NOTE: Dynamic logging hot reload was attempted but is not currently supported due to
// tracing-subscriber's complex layer type system. The reload::Layer wrapper creates type
// incompatibilities when combined with file_layer and fmt::layer() in the subscriber stack.
//
// Potential future approaches:
// - Use a simpler reload mechanism without wrapping multiple layers
// - Restructure logging initialization to be more reload-friendly
// - Use global state pattern with dynamic log level checking
//
// For now, logging configuration changes require a server restart.
//
// See: https://github.com/tokio-rs/tracing/issues/1629 for related discussions

/// Console output format options
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ConsoleFormat {
    /// Compact format (single line per log)
    #[default]
    Compact,
    /// Pretty format (multi-line, easier to read)
    Pretty,
    /// JSON format (for log aggregation)
    Json,
}

/// Logging configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Enable file logging
    #[serde(default = "default_true")]
    pub file_enabled: bool,

    /// Log file path (supports ~ expansion)
    #[serde(default = "default_log_path")]
    pub file_path: PathBuf,

    /// Maximum log file size before rotation (in MB) - currently unused, rotation is daily
    #[serde(default = "default_max_file_size")]
    pub max_file_size_mb: u64,

    /// Number of rotated log files to keep - currently unused
    #[serde(default = "default_max_backups")]
    pub max_backups: usize,

    /// Enable console logging
    #[serde(default = "default_true")]
    pub console_enabled: bool,

    /// Console output format
    #[serde(default)]
    pub console_format: ConsoleFormat,

    /// Module-specific log filters (e.g., "putio_sync_download=trace")
    #[serde(default)]
    pub module_filters: Vec<String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file_enabled: default_true(),
            file_path: default_log_path(),
            max_file_size_mb: default_max_file_size(),
            max_backups: default_max_backups(),
            console_enabled: default_true(),
            console_format: ConsoleFormat::default(),
            module_filters: Vec::new(),
        }
    }
}

// Default value functions for serde
fn default_log_level() -> String {
    "info".to_string()
}

fn default_true() -> bool {
    true
}

fn default_log_path() -> PathBuf {
    // Default to ~/.putiosync/logs/putio-sync.log
    if let Some(home) = dirs::home_dir() {
        home.join(".putiosync/logs/putio-sync.log")
    } else {
        PathBuf::from("putio-sync.log")
    }
}

fn default_max_file_size() -> u64 {
    10
}

fn default_max_backups() -> usize {
    5
}

/// Initialize logging based on configuration
///
/// This sets up tracing with the specified configuration, including
/// console and file output, log rotation, and filtering.
///
/// # Arguments
///
/// * `config` - Logging configuration
///
/// # Returns
///
/// A guard that must be kept alive for the duration of the program
/// to ensure logs are flushed properly.
///
/// # Example
///
/// ```no_run
/// use putio_sync_core::logging::{LoggingConfig, init_logging};
///
/// let config = LoggingConfig::default();
/// let _guard = init_logging(&config).unwrap();
/// ```
pub fn init_logging(config: &LoggingConfig) -> LoggingResult<()> {
    // Build the filter
    let filter = build_filter(config)?;

    // Initialize based on enabled outputs and console format
    match (
        config.console_enabled,
        config.file_enabled,
        &config.console_format,
    ) {
        // Both console and file logging
        (true, true, ConsoleFormat::Compact) => {
            let file_layer = create_file_layer_concrete(&config.file_path)?;
            tracing_subscriber::registry()
                .with(file_layer)
                .with(filter)
                .with(fmt::layer().compact())
                .try_init()
                .map_err(|e| {
                    CoreError::ConfigError(format!("Failed to initialize logging: {}", e))
                })?;
        }
        (true, true, ConsoleFormat::Pretty) => {
            let file_layer = create_file_layer_concrete(&config.file_path)?;
            tracing_subscriber::registry()
                .with(file_layer)
                .with(filter)
                .with(fmt::layer().pretty())
                .try_init()
                .map_err(|e| {
                    CoreError::ConfigError(format!("Failed to initialize logging: {}", e))
                })?;
        }
        (true, true, ConsoleFormat::Json) => {
            let file_layer = create_file_layer_concrete(&config.file_path)?;
            tracing_subscriber::registry()
                .with(file_layer)
                .with(filter)
                .with(fmt::layer().json())
                .try_init()
                .map_err(|e| {
                    CoreError::ConfigError(format!("Failed to initialize logging: {}", e))
                })?;
        }
        // Console only
        (true, false, ConsoleFormat::Compact) => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().compact())
                .try_init()
                .map_err(|e| {
                    CoreError::ConfigError(format!("Failed to initialize logging: {}", e))
                })?;
        }
        (true, false, ConsoleFormat::Pretty) => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().pretty())
                .try_init()
                .map_err(|e| {
                    CoreError::ConfigError(format!("Failed to initialize logging: {}", e))
                })?;
        }
        (true, false, ConsoleFormat::Json) => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json())
                .try_init()
                .map_err(|e| {
                    CoreError::ConfigError(format!("Failed to initialize logging: {}", e))
                })?;
        }
        // File only
        (false, true, _) => {
            let file_layer = create_file_layer_concrete(&config.file_path)?;
            tracing_subscriber::registry()
                .with(file_layer)
                .with(filter)
                .try_init()
                .map_err(|e| {
                    CoreError::ConfigError(format!("Failed to initialize logging: {}", e))
                })?;
        }
        // Neither enabled - use default console
        (false, false, _) => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().compact())
                .try_init()
                .map_err(|e| {
                    CoreError::ConfigError(format!("Failed to initialize logging: {}", e))
                })?;
        }
    }

    Ok(())
}

/// Create a file logging layer with daily rotation (concrete type)
fn create_file_layer_concrete(
    file_path: &PathBuf,
) -> LoggingResult<impl Layer<tracing_subscriber::Registry> + Send + Sync + 'static> {
    // Expand ~ in path
    let expanded_path = crate::path_utils::expand_path(file_path)?;

    // Create parent directory if it doesn't exist
    if let Some(parent) = expanded_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            CoreError::ConfigError(format!(
                "Failed to create log directory {}: {}",
                parent.display(),
                e
            ))
        })?;
    }

    // Extract directory and filename
    let directory = expanded_path
        .parent()
        .ok_or_else(|| CoreError::ConfigError("Invalid log file path".to_string()))?;

    let filename = expanded_path
        .file_name()
        .ok_or_else(|| CoreError::ConfigError("Invalid log file name".to_string()))?;

    // Create rolling file appender (daily rotation)
    let file_appender = rolling::daily(directory, filename);
    let (non_blocking, _guard) = non_blocking(file_appender);

    // Create file layer without ANSI colors
    let file_layer = fmt::layer().with_writer(non_blocking).with_ansi(false);

    // Note: We're intentionally leaking the guard here because we need it to live
    // for the entire program lifetime. This is the recommended approach for
    // application-level logging initialization.
    std::mem::forget(_guard);

    Ok(file_layer)
}

/// Build the environment filter from configuration
fn build_filter(config: &LoggingConfig) -> LoggingResult<EnvFilter> {
    // A bare word is accepted by EnvFilter as a target name, so a typo like "inf" would
    // silently disable logging. Plain values must be real levels; directive strings
    // ("info,putio_sync_download=debug") are passed through as-is.
    let is_directive = config.level.contains('=') || config.level.contains(',');
    if !is_directive {
        config
            .level
            .parse::<tracing_subscriber::filter::LevelFilter>()
            .map_err(|e| {
                CoreError::ConfigError(format!("Invalid log level '{}': {}", config.level, e))
            })?;
    }

    // Start with the base level
    let mut filter = EnvFilter::try_new(&config.level).map_err(|e| {
        CoreError::ConfigError(format!("Invalid log level '{}': {}", config.level, e))
    })?;

    // Add module-specific filters
    for module_filter in &config.module_filters {
        filter = filter.add_directive(module_filter.parse().map_err(|e| {
            CoreError::ConfigError(format!("Invalid module filter '{}': {}", module_filter, e))
        })?);
    }

    // Allow RUST_LOG environment variable to override if set
    if let Ok(env_filter) = std::env::var("RUST_LOG") {
        filter = EnvFilter::try_new(env_filter).map_err(|e| {
            CoreError::ConfigError(format!("Invalid RUST_LOG environment variable: {}", e))
        })?;
    }

    Ok(filter)
}

/// Legacy initialization function for backward compatibility
///
/// # Arguments
///
/// * `level` - The log level to use (e.g., "trace", "debug", "info", "warn", "error")
pub fn init(level: &str) {
    let config = LoggingConfig {
        level: level.to_string(),
        console_enabled: true,
        file_enabled: false,
        ..Default::default()
    };

    let _ = init_logging(&config);
}

/// Legacy initialization from environment for backward compatibility
pub fn init_from_env() {
    init("info");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_logging_config() {
        let config = LoggingConfig::default();
        assert_eq!(config.level, "info");
        assert!(config.console_enabled);
        assert!(config.file_enabled);
        assert_eq!(config.console_format, ConsoleFormat::Compact);
    }

    #[test]
    fn test_console_format_serialization() {
        let compact = ConsoleFormat::Compact;
        let json = serde_json::to_string(&compact).unwrap();
        assert_eq!(json, "\"compact\"");

        let pretty = ConsoleFormat::Pretty;
        let json = serde_json::to_string(&pretty).unwrap();
        assert_eq!(json, "\"pretty\"");

        let json_format = ConsoleFormat::Json;
        let json = serde_json::to_string(&json_format).unwrap();
        assert_eq!(json, "\"json\"");
    }

    #[test]
    fn test_console_format_deserialization() {
        let compact: ConsoleFormat = serde_json::from_str("\"compact\"").unwrap();
        assert_eq!(compact, ConsoleFormat::Compact);

        let pretty: ConsoleFormat = serde_json::from_str("\"pretty\"").unwrap();
        assert_eq!(pretty, ConsoleFormat::Pretty);

        let json: ConsoleFormat = serde_json::from_str("\"json\"").unwrap();
        assert_eq!(json, ConsoleFormat::Json);
    }

    #[test]
    fn test_build_filter() {
        let config = LoggingConfig {
            level: "debug".to_string(),
            ..Default::default()
        };

        let filter = build_filter(&config);
        assert!(filter.is_ok());
    }

    #[test]
    fn test_build_filter_with_module_filters() {
        let config = LoggingConfig {
            module_filters: vec![
                "putio_sync_download=trace".to_string(),
                "putio_sync_client=debug".to_string(),
            ],
            ..Default::default()
        };

        let filter = build_filter(&config);
        assert!(filter.is_ok());
    }

    #[test]
    fn test_directive_log_level() {
        let config = LoggingConfig {
            level: "info,putio_sync_download=debug".to_string(),
            ..Default::default()
        };

        assert!(build_filter(&config).is_ok());
    }

    #[test]
    fn test_invalid_log_level() {
        let config = LoggingConfig {
            level: "invalid_level".to_string(),
            ..Default::default()
        };

        let filter = build_filter(&config);
        assert!(filter.is_err());
    }
}
