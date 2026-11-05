//! Logging configuration and initialization
//!
//! This module provides flexible logging configuration with support for:
//! - Console and file output
//! - Multiple console formats (compact, pretty, JSON)
//! - Daily log rotation
//! - Module-specific log level filtering
//! - Configurable log levels

use crate::error::CoreError;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime};
use tracing_appender::{non_blocking, rolling};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{
    fmt, layer::SubscriberExt, reload, util::SubscriberInitExt, EnvFilter, Layer, Registry,
};

/// Result type for logging operations
pub type LoggingResult<T> = Result<T, CoreError>;

// The level filter is the first layer on the Registry, wrapped in reload::Layer, so every
// output combination shares one handle type and the level can change at runtime
// (apply_level, and the Logs page's overrides). Other logging settings (format, outputs,
// file path) need a restart. The capture layer for the Logs page comes right after the
// filter, so it keeps only the lines the level in effect lets through.

/// Handle to the live level filter, set by `init_logging`
static FILTER_HANDLE: OnceLock<reload::Handle<EnvFilter, Registry>> = OnceLock::new();

/// The base level and the Logs page's overrides on top of it (feature 047)
static STATE: Mutex<LevelState> = Mutex::new(LevelState::new());

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

    /// Recent lines kept for the Logs page (feature 047), [`BUFFER_LINES_RANGE`]
    #[serde(default = "default_buffer_lines")]
    pub buffer_lines: usize,
}

/// Allowed `buffer_lines`
pub const BUFFER_LINES_RANGE: std::ops::RangeInclusive<usize> = 100..=50_000;

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
            buffer_lines: default_buffer_lines(),
        }
    }
}

// Default value functions for serde
fn default_log_level() -> String {
    "info".to_string()
}

fn default_buffer_lines() -> usize {
    2000
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
    crate::log_capture::set_capacity(config.buffer_lines);

    // Initialize based on enabled outputs and console format
    match (config.console_enabled, config.file_enabled, &format) {
        // Both console and file logging
        (true, true, ConsoleFormat::Compact) => {
            let file_layer = create_file_layer_concrete(&config.file_path)?;
            tracing_subscriber::registry()
                .with(filter)
                .with(crate::log_capture::CaptureLayer::new())
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
                .with(crate::log_capture::CaptureLayer::new())
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
                .with(crate::log_capture::CaptureLayer::new())
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
                .with(crate::log_capture::CaptureLayer::new())
                .with(fmt::layer().compact())
                .try_init()
                .map_err(|e| {
                    CoreError::ConfigError(format!("Failed to initialize logging: {}", e))
                })?;
        }
        (true, false, ConsoleFormat::Pretty) => {
            tracing_subscriber::registry()
                .with(filter)
                .with(crate::log_capture::CaptureLayer::new())
                .with(fmt::layer().pretty())
                .try_init()
                .map_err(|e| {
                    CoreError::ConfigError(format!("Failed to initialize logging: {}", e))
                })?;
        }
        (true, false, ConsoleFormat::Json) => {
            tracing_subscriber::registry()
                .with(filter)
                .with(crate::log_capture::CaptureLayer::new())
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
                .with(crate::log_capture::CaptureLayer::new())
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
                .with(crate::log_capture::CaptureLayer::new())
                .with(fmt::layer().compact())
                .try_init()
                .map_err(|e| {
                    CoreError::ConfigError(format!("Failed to initialize logging: {}", e))
                })?;
        }
    }

    let _ = FILTER_HANDLE.set(handle);
    {
        let mut state = state();
        state.base = directive.clone();
        state.source = source;
    }

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
/// Called by the config reload poller. It replaces the base level; overrides set on the
/// Logs page stay on top of it. Logs one line describing what happened.
pub fn apply_level(config: &LoggingConfig) -> LevelChange {
    if std::env::var_os("RUST_LOG").is_some() {
        tracing::info!("Log level change in config ignored; RUST_LOG is set");
        return LevelChange::IgnoredEnv;
    }

    let (_, _, directive) = match build_filter(config) {
        Ok(built) => built,
        Err(e) => {
            tracing::error!(error = %e, "Log level not changed");
            return LevelChange::Invalid(e.to_string());
        }
    };

    let result = change(LevelReason::Config, |state| {
        state.base = directive;
        state.source = LevelSource::Config;
        Ok(())
    });
    match result {
        Ok(Some((old, new))) => LevelChange::Changed { old, new },
        Ok(None) => LevelChange::Unchanged,
        Err(e) => {
            tracing::error!(error = %e, "Log level not changed");
            LevelChange::Invalid(e.to_string())
        }
    }
}

/// Levels the Logs page can set
pub const LEVELS: [&str; 6] = ["off", "error", "warn", "info", "debug", "trace"];

/// Most module overrides at once
pub const MAX_MODULE_OVERRIDES: usize = 20;

/// Allowed length of a timed override, in minutes
pub const OVERRIDE_MINUTES: std::ops::RangeInclusive<u32> = 1..=1440;

/// A level set on the Logs page, for everything or for one module
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    /// One of [`LEVELS`]
    pub level: String,
    /// When it reverts; `None` keeps it until restart
    pub until: Option<SystemTime>,
}

/// The base level from the config (or `RUST_LOG`) and the overrides on top of it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelState {
    /// Directive from the config or `RUST_LOG`
    pub base: String,
    /// Where `base` came from
    pub source: LevelSource,
    /// Overall level from the page; replaces the base's bare level
    pub global: Option<Override>,
    /// Module levels from the page; each replaces the base's entry for that module
    pub modules: BTreeMap<String, Override>,
}

/// Why the level in effect changed, logged with the change
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LevelReason {
    Config,
    Page,
    Reverted,
    Expired,
}

impl LevelReason {
    fn as_str(self) -> &'static str {
        match self {
            LevelReason::Config => "config",
            LevelReason::Page => "page",
            LevelReason::Reverted => "reverted",
            LevelReason::Expired => "expired",
        }
    }
}

/// A level change the Logs page asked for that could not be made
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LevelError {
    /// The level, module or duration is not allowed; the message says which
    #[error("{0}")]
    Invalid(String),
    /// Logging was set up without a reload handle, so the level cannot change
    #[error("the log level can't be changed in this process")]
    Unavailable,
}

impl LevelState {
    const fn new() -> Self {
        LevelState {
            base: String::new(),
            source: LevelSource::Config,
            global: None,
            modules: BTreeMap::new(),
        }
    }

    /// The directive in effect: the base, with its bare level replaced by the overall
    /// override and its module entries replaced by module overrides
    pub fn effective(&self) -> String {
        let mut items: Vec<String> = Vec::new();
        let mut had_level = false;
        for item in self
            .base
            .split(',')
            .map(str::trim)
            .filter(|i| !i.is_empty())
        {
            match item.split_once('=') {
                None => {
                    had_level = true;
                    items.push(self.global.as_ref().map_or(item, |g| &g.level).to_string());
                }
                Some((target, _)) => {
                    let target = target.split('[').next().unwrap_or(target);
                    if !self.modules.contains_key(target) {
                        items.push(item.to_string());
                    }
                }
            }
        }
        if let (false, Some(global)) = (had_level, &self.global) {
            items.insert(0, global.level.clone());
        }
        items.extend(
            self.modules
                .iter()
                .map(|(target, o)| format!("{}={}", target, o.level)),
        );
        items.join(",")
    }

    /// Set the overall level (`target` None) or one module's.
    pub fn set(
        &mut self,
        target: Option<&str>,
        level: &str,
        until: Option<SystemTime>,
    ) -> Result<(), LevelError> {
        let level = level.trim().to_ascii_lowercase();
        if !LEVELS.contains(&level.as_str()) {
            return Err(LevelError::Invalid(format!(
                "level must be one of {}",
                LEVELS.join(", ")
            )));
        }
        let o = Override { level, until };
        match target.map(str::trim).filter(|t| !t.is_empty()) {
            None => self.global = Some(o),
            Some(target) => {
                if !valid_target(target) {
                    return Err(LevelError::Invalid(
                        "module must be a path like putio_sync_core::download".to_string(),
                    ));
                }
                if !self.modules.contains_key(target) && self.modules.len() >= MAX_MODULE_OVERRIDES
                {
                    return Err(LevelError::Invalid(format!(
                        "at most {} module levels at once; revert one first",
                        MAX_MODULE_OVERRIDES
                    )));
                }
                self.modules.insert(target.to_string(), o);
            }
        }
        Ok(())
    }

    /// Drop one module's override, the overall one (`Some("")`), or with `None` all of them.
    pub fn clear(&mut self, target: Option<&str>) {
        match target.map(str::trim) {
            None => {
                self.global = None;
                self.modules.clear();
            }
            Some("") => self.global = None,
            Some(target) => {
                self.modules.remove(target);
            }
        }
    }

    /// Drop overrides that ended at or before `now`; true if any did.
    pub fn expire(&mut self, now: SystemTime) -> bool {
        let ended = |o: &Override| o.until.is_some_and(|until| until <= now);
        let before = self.modules.len() + usize::from(self.global.is_some());
        if self.global.as_ref().is_some_and(ended) {
            self.global = None;
        }
        self.modules.retain(|_, o| !ended(o));
        before != self.modules.len() + usize::from(self.global.is_some())
    }

    /// The state for the API and the doctor
    pub fn status(&self, available: bool) -> LevelStatus {
        let effective = self.effective();
        LevelStatus {
            base: self.base.clone(),
            source: match self.source {
                LevelSource::Config => "config",
                LevelSource::Env => "env",
            },
            verbose: is_verbose(&effective),
            effective,
            global: self.global.as_ref().map(|o| OverrideStatus {
                target: None,
                level: o.level.clone(),
                until: o.until.map(rfc3339),
            }),
            modules: self
                .modules
                .iter()
                .map(|(target, o)| OverrideStatus {
                    target: Some(target.clone()),
                    level: o.level.clone(),
                    until: o.until.map(rfc3339),
                })
                .collect(),
            available,
        }
    }
}

/// The level in effect and the overrides, as the Logs page and the doctor show them
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LevelStatus {
    /// Directive from the config or `RUST_LOG`
    pub base: String,
    /// `config` or `env`
    pub source: &'static str,
    /// Directive in effect
    pub effective: String,
    /// Overall override
    pub global: Option<OverrideStatus>,
    /// Module overrides, sorted by module
    pub modules: Vec<OverrideStatus>,
    /// Something is at debug or trace
    pub verbose: bool,
    /// The level can be changed in this process
    pub available: bool,
}

/// One override in a [`LevelStatus`]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OverrideStatus {
    /// Module, for a module override
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Level
    pub level: String,
    /// When it reverts (RFC 3339), or null when kept until restart
    pub until: Option<String>,
}

fn rfc3339(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(time).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Whether a directive puts anything at debug or trace
pub fn is_verbose(directive: &str) -> bool {
    directive.split(',').any(|item| {
        let level = item.rsplit('=').next().unwrap_or(item).trim();
        level.eq_ignore_ascii_case("debug") || level.eq_ignore_ascii_case("trace")
    })
}

/// `a::b_c` style module paths
fn valid_target(target: &str) -> bool {
    target.len() <= 200
        && target.split("::").all(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

fn state() -> MutexGuard<'static, LevelState> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Apply `edit` to the level state and reload the filter if the directive changed.
///
/// The state is kept only if the reload worked. Returns the old and new directive when
/// it changed, and logs one line saying why.
fn change(
    reason: LevelReason,
    edit: impl FnOnce(&mut LevelState) -> Result<(), LevelError>,
) -> Result<Option<(String, String)>, LevelError> {
    let mut state = state();
    let mut next = state.clone();
    edit(&mut next)?;
    let old = state.effective();
    let new = next.effective();
    if old != new {
        if let Some(handle) = FILTER_HANDLE.get() {
            let filter =
                EnvFilter::try_new(&new).map_err(|e| LevelError::Invalid(e.to_string()))?;
            handle
                .reload(filter)
                .map_err(|e| LevelError::Invalid(e.to_string()))?;
        }
    }
    *state = next;
    drop(state);
    if old == new {
        return Ok(None);
    }
    tracing::info!(old = %old, new = %new, reason = reason.as_str(), "Log level changed");
    Ok(Some((old, new)))
}

/// The level in effect and the overrides, dropping any that ended
pub fn level_status() -> LevelStatus {
    expire_now();
    state().status(FILTER_HANDLE.get().is_some())
}

/// Set the overall level (`target` None) or a module's from the Logs page, for `minutes`
/// or (None) until restart.
pub fn set_override(
    target: Option<&str>,
    level: &str,
    minutes: Option<u32>,
) -> Result<LevelStatus, LevelError> {
    if FILTER_HANDLE.get().is_none() {
        return Err(LevelError::Unavailable);
    }
    if let Some(m) = minutes.filter(|m| !OVERRIDE_MINUTES.contains(m)) {
        return Err(LevelError::Invalid(format!(
            "minutes must be 1 to 1440, or left out to keep the level until restart, not {}",
            m
        )));
    }
    let length = minutes.map(|m| Duration::from_secs(u64::from(m) * 60));
    let until = length.map(|d| SystemTime::now() + d);
    change(LevelReason::Page, |state| state.set(target, level, until))?;
    if let (Some(length), Ok(runtime)) = (length, tokio::runtime::Handle::try_current()) {
        runtime.spawn(async move {
            // A second late, so `until <= now` holds when it runs
            tokio::time::sleep(length + Duration::from_secs(1)).await;
            expire_now();
        });
    }
    Ok(level_status())
}

/// Revert one module's override, the overall one (`Some("")`), or (None) all of them.
pub fn clear_override(target: Option<&str>) -> Result<LevelStatus, LevelError> {
    change(LevelReason::Reverted, |state| {
        state.clear(target);
        Ok(())
    })?;
    Ok(level_status())
}

/// Drop overrides that have ended.
fn expire_now() {
    let ended = state().clone().expire(SystemTime::now());
    if ended {
        let result = change(LevelReason::Expired, |state| {
            state.expire(SystemTime::now());
            Ok(())
        });
        if let Err(e) = result {
            tracing::error!(error = %e, "Log level not reverted");
        }
    }
}

/// Check that `level` and `module_filters` parse, without touching the subscriber.
///
/// The settings API calls this before saving, so a bad value is refused instead of being
/// written to the config file and rejected later by the reload poller.
pub fn validate_filters(config: &LoggingConfig) -> LoggingResult<()> {
    if !BUFFER_LINES_RANGE.contains(&config.buffer_lines) {
        return Err(CoreError::ConfigError(format!(
            "logging.buffer_lines must be 100 to 50000, not {}",
            config.buffer_lines
        )));
    }
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
            *state() = LevelState {
                base: "info".to_string(),
                ..LevelState::new()
            };

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
            assert_eq!(state().effective(), "debug");

            // A config change replaces the base and keeps the page's overrides
            state()
                .set(Some("putio_sync_hooks"), "trace", None)
                .unwrap();
            assert_eq!(
                apply_level(&level("warn")),
                LevelChange::Changed {
                    old: "debug,putio_sync_hooks=trace".to_string(),
                    new: "warn,putio_sync_hooks=trace".to_string()
                }
            );
            state().clear(None);
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

    fn base(base: &str) -> LevelState {
        LevelState {
            base: base.to_string(),
            ..LevelState::new()
        }
    }

    #[test]
    fn overrides_compose_onto_the_base() {
        let mut s = base("info,putio_sync_hooks=warn,hyper=error");
        assert_eq!(s.effective(), "info,putio_sync_hooks=warn,hyper=error");

        s.set(None, "DEBUG", None).unwrap();
        s.set(Some("putio_sync_hooks"), "trace", None).unwrap();
        s.set(Some("putio_sync_core::download"), "debug", None)
            .unwrap();
        assert_eq!(
            s.effective(),
            "debug,hyper=error,putio_sync_core::download=debug,putio_sync_hooks=trace"
        );

        // A base without a bare level gets the overall one in front
        let mut s = base("putio_sync_api=info");
        s.set(None, "warn", None).unwrap();
        assert_eq!(s.effective(), "warn,putio_sync_api=info");

        s.clear(Some(""));
        assert_eq!(s.effective(), "putio_sync_api=info");
    }

    #[test]
    fn overrides_expire_at_their_end_time() {
        let now = SystemTime::now();
        let mut s = base("info");
        s.set(None, "debug", Some(now + Duration::from_secs(60)))
            .unwrap();
        s.set(Some("a"), "trace", Some(now + Duration::from_secs(1800)))
            .unwrap();
        s.set(Some("b"), "trace", None).unwrap();

        assert!(!s.expire(now));
        assert!(s.expire(now + Duration::from_secs(60)));
        assert_eq!(s.effective(), "info,a=trace,b=trace");
        assert!(s.expire(now + Duration::from_secs(3600)));
        assert_eq!(s.effective(), "info,b=trace");
        assert!(!s.expire(now + Duration::from_secs(86_400 * 365)));
    }

    #[test]
    fn invalid_overrides_are_refused_and_change_nothing() {
        let mut s = base("info");
        for (target, level) in [
            (None, "loud"),
            (Some("bad module"), "debug"),
            (Some("a::"), "debug"),
            (Some("1abc"), "debug"),
            (Some("a=b"), "debug"),
        ] {
            assert!(
                matches!(s.set(target, level, None), Err(LevelError::Invalid(_))),
                "{target:?} {level}"
            );
        }
        assert_eq!(s, base("info"));

        for i in 0..MAX_MODULE_OVERRIDES {
            s.set(Some(&format!("m{i}")), "debug", None).unwrap();
        }
        let err = s.set(Some("one_more"), "debug", None).unwrap_err();
        assert!(err.to_string().contains("at most 20"), "{err}");
        // Changing one already set is still allowed
        s.set(Some("m0"), "trace", None).unwrap();
    }

    #[test]
    fn status_reports_verbose_levels() {
        let mut s = base("info");
        assert!(!s.status(true).verbose);
        s.set(Some("putio_sync_hooks"), "debug", None).unwrap();
        let status = s.status(true);
        assert!(status.verbose);
        assert_eq!(
            status.modules[0].target.as_deref(),
            Some("putio_sync_hooks")
        );
        assert_eq!(status.modules[0].until, None);
        assert!(is_verbose("TRACE"));
        assert!(!is_verbose("info,a=warn"));
    }

    #[test]
    fn page_changes_need_a_reload_handle() {
        if FILTER_HANDLE.get().is_none() {
            assert_eq!(
                set_override(None, "debug", Some(30)),
                Err(LevelError::Unavailable)
            );
            assert!(!level_status().available);
        }
    }
}
