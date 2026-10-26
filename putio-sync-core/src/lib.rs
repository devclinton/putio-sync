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

/// Configuration management
pub mod config;

/// Error types and result aliases
pub mod error;

/// File filtering and sorting logic
pub mod filter;

/// Structured logging infrastructure
pub mod logging;

/// Notification system for download events
pub mod notification;

/// Path handling utilities
pub mod path_utils;

/// Stalled Put.io transfer detection
pub mod stall;

/// Re-export commonly used types
pub mod prelude {
    pub use crate::config::{
        AuthConfig, AuthMethod, Config, ConfigBuilder, DownloadConfig, NotificationBackend,
        NotificationConfig, ProgressStyle, SortOrder, SyncConfig, WatchConfig,
    };
    pub use crate::error::{CoreError, CoreResult};
    pub use crate::filter::{parse_size, sort_files, FileFilter, FileTypeFilter, SortField};
    pub use crate::logging;
    pub use crate::notification::{DownloadCompletionEvent, NotificationService};
    pub use crate::path_utils::{
        ensure_directory_exists, expand_path, normalize_path, validate_directory_path,
    };
}
