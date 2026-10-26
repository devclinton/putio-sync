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
use std::sync::{Mutex, OnceLock};
use tracing_appender::{non_blocking, rolling};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{
    fmt, layer::SubscriberExt, reload, util::SubscriberInitExt, EnvFilter, Layer, Registry,
};

/// Result type for logging operations
pub type LoggingResult<T> = Result<T, CoreError>;

// The level filter is the first layer on the Registry, wrapped in reload::Layer, so every
// output combination shares one handle type and the level can change at runtime
// (apply_level). Other logging settings (format, outputs, file path) need a restart.

/// Handle to the live level filter, set by `init_logging`
static FILTER_HANDLE: OnceLock<reload::Handle<EnvFilter, Registry>> = OnceLock::new();

/// Directive string of the filter currently in effect
static CURRENT_DIRECTIVE: Mutex<String> = Mutex::new(String::new());

/// Result of applying a new `[logging]` level at runtime
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelChange {
    /// The level is the one already in effect
    Unchanged,
    /// The filter was replaced
    Changed {
        /// Previous directive
        old: String,
        /// New directive
        new: String,
    },
    /// `RUST_LOG` is set, so the config level does not apply
    IgnoredEnv,
    /// The new level is invalid; the current filter is kept
    Invalid(String),
}

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

impl ConsoleFormat {
    /// The config value
    pub fn as_str(&self) -> &'static str {
        match self {
            ConsoleFormat::Compact => "compact",
            ConsoleFormat::Pretty => "pretty",
            ConsoleFormat::Json => "json",
        }
    }
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
    let (filter, source, directive) = build_filter(config)?;
    let (format, format_source) = resolve_format(config)?;
    let (filter, handle) = reload::Layer::new(filter);

    // Initialize based on enabled outputs and console format
    match (config.console_enabled, config.file_enabled, &format) {
        // Both console and file logging
        (true, true, ConsoleFormat::Compact) => {
            let file_layer = create_file_layer_concrete(&config.file_path)?;
            tracing_subscriber::registry()
                .with(filter)
                .with(file_layer)
                .with(fmt::layer().compact())
                .try_init()
                .map_err(|e| {
                    CoreError::ConfigError(format!("Failed to initialize logging: {}", e))
                })?;
        }
        (true, true, ConsoleFormat::Pretty) => {
            let file_layer = create_file_layer_concrete(&config.file_path)?;
            tracing_subscriber::registry()
                .with(filter)
                .with(file_layer)
                .with(fmt::layer().pretty())
                .try_init()
                .map_err(|e| {
                    CoreError::ConfigError(format!("Failed to initialize logging: {}", e))
                })?;
        }
        (true, true, ConsoleFormat::Json) => {
            let file_layer = create_file_layer_concrete(&config.file_path)?;
            tracing_subscriber::registry()
                .with(filter)
                .with(file_layer)
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
                .with(filter)
                .with(file_layer)
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

    let _ = FILTER_HANDLE.set(handle);
    *CURRENT_DIRECTIVE.lock().unwrap_or_else(|e| e.into_inner()) = directive.clone();

    // RUST_LOG silently replaced the configured level before; say which one won.
    tracing::info!(
        level = %directive,
        source = ?source,
        format = %format.as_str(),
        format_source = ?format_source,
        "Log level"
    );

    Ok(())
}

/// Create a file logging layer with daily rotation
fn create_file_layer_concrete<S>(
    file_path: &PathBuf,
) -> LoggingResult<impl Layer<S> + Send + Sync + 'static>
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
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

/// Where the effective log level came from
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelSource {
    /// `[logging]` in the config file
    Config,
    /// The `RUST_LOG` environment variable, which overrides the config
    Env,
}

/// Resolve the console format: `PUTIO_SYNC_LOG_FORMAT` wins over `logging.console_format`.
fn resolve_format(config: &LoggingConfig) -> LoggingResult<(ConsoleFormat, LevelSource)> {
    let Ok(value) = std::env::var("PUTIO_SYNC_LOG_FORMAT") else {
        return Ok((config.console_format.clone(), LevelSource::Config));
    };
    let format = match value.as_str() {
        "compact" => ConsoleFormat::Compact,
        "pretty" => ConsoleFormat::Pretty,
        "json" => ConsoleFormat::Json,
        other => {
            return Err(CoreError::ConfigError(format!(
                "invalid logging.console_format '{}': expected one of compact, pretty, json",
                other
            )))
        }
    };
    Ok((format, LevelSource::Env))
}

/// Apply a new `[logging]` level and module filters to the running subscriber.
///
/// Called by the config reload poller. Logs one line describing what happened.
pub fn apply_level(config: &LoggingConfig) -> LevelChange {
    if std::env::var_os("RUST_LOG").is_some() {
        tracing::info!("Log level change in config ignored; RUST_LOG is set");
        return LevelChange::IgnoredEnv;
    }

    let (filter, _, directive) = match build_filter(config) {
        Ok(built) => built,
        Err(e) => {
            tracing::error!(error = %e, "Log level not changed");
            return LevelChange::Invalid(e.to_string());
        }
    };

    let mut current = CURRENT_DIRECTIVE.lock().unwrap_or_else(|e| e.into_inner());
    if *current == directive {
        return LevelChange::Unchanged;
    }
    if let Some(handle) = FILTER_HANDLE.get() {
        if let Err(e) = handle.reload(filter) {
            tracing::error!(error = %e, "Log level not changed");
            return LevelChange::Invalid(e.to_string());
        }
    }
    let old = std::mem::replace(&mut *current, directive.clone());
    drop(current);

    tracing::info!(old = %old, new = %directive, source = ?LevelSource::Config, "Log level changed");
    LevelChange::Changed {
        old,
        new: directive,
    }
}

/// Check that `level` and `module_filters` parse, without touching the subscriber.
///
/// The settings API calls this before saving, so a bad value is refused instead of being
/// written to the config file and rejected later by the reload poller.
pub fn validate_filters(config: &LoggingConfig) -> LoggingResult<()> {
    config_filter(config).map(|_| ())
}

/// The filter for `level` plus `module_filters`, ignoring `RUST_LOG`
fn config_filter(config: &LoggingConfig) -> LoggingResult<EnvFilter> {
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

    Ok(filter)
}

/// Build the environment filter from configuration
///
/// Returns the filter, where its level came from, and the effective directive string.
fn build_filter(config: &LoggingConfig) -> LoggingResult<(EnvFilter, LevelSource, String)> {
    let filter = config_filter(config)?;

    // Allow RUST_LOG environment variable to override if set
    if let Ok(env_filter) = std::env::var("RUST_LOG") {
        let filter = EnvFilter::try_new(&env_filter).map_err(|e| {
            CoreError::ConfigError(format!("Invalid RUST_LOG environment variable: {}", e))
        })?;
        return Ok((filter, LevelSource::Env, env_filter));
    }

    let directive = std::iter::once(config.level.as_str())
        .chain(config.module_filters.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(",");
    Ok((filter, LevelSource::Config, directive))
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

    /// Serialises tests that change RUST_LOG
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn log_level_source_reports_env_override() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var("RUST_LOG").ok();
        let config = LoggingConfig {
            level: "debug".to_string(),
            ..Default::default()
        };

        std::env::set_var("RUST_LOG", "info");
        let (_, source, directive) = build_filter(&config).unwrap();
        assert_eq!(source, LevelSource::Env);
        assert_eq!(directive, "info");

        std::env::remove_var("RUST_LOG");
        let (_, source, directive) = build_filter(&config).unwrap();
        assert_eq!(source, LevelSource::Config);
        assert!(directive.starts_with("debug"), "{directive}");

        if let Some(v) = previous {
            std::env::set_var("RUST_LOG", v);
        }
    }

    fn with_env<T>(name: &str, value: Option<&str>, f: impl FnOnce() -> T) -> T {
        let previous = std::env::var(name).ok();
        match value {
            Some(v) => std::env::set_var(name, v),
            None => std::env::remove_var(name),
        }
        let out = f();
        match previous {
            Some(v) => std::env::set_var(name, v),
            None => std::env::remove_var(name),
        }
        out
    }

    fn level(level: &str) -> LoggingConfig {
        LoggingConfig {
            level: level.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn log_format_env_overrides_config() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let config = LoggingConfig::default();

        let resolved = with_env("PUTIO_SYNC_LOG_FORMAT", Some("json"), || {
            resolve_format(&config).unwrap()
        });
        assert_eq!(resolved, (ConsoleFormat::Json, LevelSource::Env));

        let resolved = with_env("PUTIO_SYNC_LOG_FORMAT", None, || {
            resolve_format(&config).unwrap()
        });
        assert_eq!(resolved, (ConsoleFormat::Compact, LevelSource::Config));
    }

    #[test]
    fn invalid_log_format_names_allowed_values() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let err = with_env("PUTIO_SYNC_LOG_FORMAT", Some("x"), || {
            resolve_format(&LoggingConfig::default()).unwrap_err()
        });
        assert_eq!(
            err.to_string(),
            "Configuration error: invalid logging.console_format 'x': expected one of compact, pretty, json"
        );
    }

    #[test]
    fn apply_level_reports_each_outcome() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        with_env("RUST_LOG", None, || {
            *CURRENT_DIRECTIVE.lock().unwrap() = "info".to_string();

            assert_eq!(
                apply_level(&level("debug")),
                LevelChange::Changed {
                    old: "info".to_string(),
                    new: "debug".to_string()
                }
            );
            assert_eq!(apply_level(&level("debug")), LevelChange::Unchanged);
            assert!(matches!(
                apply_level(&level("loud")),
                LevelChange::Invalid(_)
            ));
            assert_eq!(*CURRENT_DIRECTIVE.lock().unwrap(), "debug");
        });

        with_env("RUST_LOG", Some("warn"), || {
            assert_eq!(apply_level(&level("trace")), LevelChange::IgnoredEnv);
        });
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
    fn validate_filters_checks_level_and_module_filters() {
        assert!(validate_filters(&level("debug")).is_ok());
        assert!(validate_filters(&level("loud")).is_err());
        let config = LoggingConfig {
            module_filters: vec!["a=b=c".to_string()],
            ..Default::default()
        };
        assert!(validate_filters(&config).is_err());
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
