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

/// Where the state database lives, and its move out of the download folder
pub mod database;

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

/// `[notifications.smtp]`, the email channel: validation and password masking (feature 030)
pub mod email;

/// Event hook config: validation and secret masking (feature 027)
pub mod hooks;

/// `[sign_in]`: password and OAuth sign-in settings and the allow-list (feature 019)
pub mod sign_in;

/// Lua plugin settings: validation and secret masking (feature 028)
pub mod plugins;

/// Language codes for subtitle names (feature 025)
pub mod lang;

/// What the Radarr/Sonarr side knows about a download, as a trait (feature 050)
pub mod media;

/// `[subtitles]` settings: validation and key masking (feature 025)
pub mod subtitles;

/// `[translation]` settings: validation and secret masking (feature 026)
pub mod translation;

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

/// Question the Radarr/Sonarr view asks the processing pipeline (feature 038)
pub mod pipeline_gate;

/// Shortened forms of values that may carry secrets (feature 027)
pub mod redact;

/// Organising files stream mode keeps on Put.io: settings and target names (feature 023)
pub mod organise;

/// Per-folder file rules: presets, inheritance and decisions
pub mod rules;

/// Reading release names: title, year, season, episode and quality (features 050, 023)
pub mod release;

pub mod setup;

/// Stalled Put.io transfer detection
pub mod stall;

/// Sync extension points plugins provide: file filters, rename and folder rules
pub mod sync_hooks;

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
