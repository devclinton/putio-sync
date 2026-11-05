//! # putio-sync-core
//!
//! Core shared utilities for putio-sync including configuration, filtering,
//! and path handling.
//!
//! This crate provides common functionality used across all putio-sync crates:
//! - Configuration management (Config, AuthConfig, etc.)
//! - File filtering and sorting utilities
//! - Path handling and expansion
//! - Error types
//! - Logging setup
//! - Notification system

#![warn(missing_docs)]

/// Put.io empty-folder cleanup service
pub mod cleanup;

/// Bandwidth schedules, limits and timed overrides (feature 032)
pub mod bandwidth;

/// Configuration management
pub mod config;

/// Settings risks, diffs, export and import (feature 015)
pub mod assistant;

/// Config file warnings for the doctor
pub mod config_lint;

/// Error types and result aliases
pub mod error;

/// Events on the internal event bus, and their JSON schema (feature 027)
pub mod events;

/// File filtering and sorting logic
pub mod filter;

/// Event hook config: validation and secret masking (feature 027)
pub mod hooks;

/// Lua plugin settings: validation and secret masking (feature 028)
pub mod plugins;

/// Listen addresses: `ip:port`, `[v6]:port`, `eth0:port`, `tailnet:port` (feature 037)
pub mod listen;

/// Structured logging infrastructure
pub mod logging;

/// Masking secrets in captured log lines (feature 047)
pub mod log_redact;

/// Recent log lines kept for the web UI's Logs page (feature 047)
pub mod log_capture;

/// Notification system for download events
pub mod notification;

/// Path handling utilities
pub mod path_utils;

/// Saved file patterns with AND, OR and NOT (feature 020)
pub mod pattern;

/// Shortened forms of values that may carry secrets (feature 027)
pub mod redact;

/// Per-folder file rules: presets, inheritance and decisions
pub mod rules;

pub mod setup;

/// Stalled Put.io transfer detection
pub mod stall;

/// Re-export commonly used types
pub mod prelude {
    pub use crate::config::{
        AuthConfig, AuthMethod, CleanupConfig, Config, ConfigBuilder, DownloadConfig,
        NotificationBackend, NotificationConfig, ProgressStyle, SortOrder, SyncConfig, WatchConfig,
    };
    pub use crate::error::{CoreError, CoreResult};
    pub use crate::filter::{parse_size, sort_files, FileFilter, FileTypeFilter, SortField};
    pub use crate::logging;
    pub use crate::notification::{DownloadCompletionEvent, NotificationService};
    pub use crate::path_utils::{
        ensure_directory_exists, expand_path, normalize_path, validate_directory_path,
    };
}
