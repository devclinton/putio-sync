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

/// Error types and result aliases
pub mod error;

/// Structured logging infrastructure
pub mod logging;

/// Re-export commonly used types
pub mod prelude {
    pub use crate::api::ApiClient;
    pub use crate::config::{
        AuthConfig, Config, ConfigBuilder, DownloadConfig, SyncConfig, WatchConfig,
    };
    pub use crate::error::{CoreError, CoreResult};
    pub use crate::logging;
}
