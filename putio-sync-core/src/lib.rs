//! # putio-sync-core
//!
//! Core library for putio-sync containing all domain logic, API client,
//! sync planning, and local state management.
//!
//! This crate provides the foundational components for interacting with
//! the Put.io API and managing local sync state, without any UI or CLI
//! concerns.

#![warn(missing_docs)]

/// Put.io API client
pub mod api;

/// Configuration management
pub mod config;

/// Database for local state management
pub mod db;

/// Download management
pub mod download;

/// Error types and result aliases
pub mod error;

/// File filtering and sorting logic
pub mod filter;

/// Filesystem utilities
pub mod fs;

/// Structured logging infrastructure
pub mod logging;

/// Notification system for download events
pub mod notification;

/// Re-export commonly used types
pub mod prelude {
    pub use crate::api::ApiClient;
    pub use crate::config::{
        AuthConfig, Config, ConfigBuilder, DownloadConfig, NotificationBackend, NotificationConfig,
        ProgressStyle, SortOrder, SyncConfig, WatchConfig,
    };
    pub use crate::db::{
        models::{ChunkStatus, Download, DownloadChunk, DownloadHistory, DownloadStats, DownloadStatus, HistoryAction},
        Database, DownloadRepository, ExtensionStats, HistoryRepository, StatsRepository,
    };
    pub use crate::error::{CoreError, CoreResult};
    pub use crate::filter::{parse_size, sort_files, FileFilter, FileTypeFilter, SortField};
    pub use crate::logging;
    pub use crate::notification::{DownloadCompletionEvent, NotificationService};
}
