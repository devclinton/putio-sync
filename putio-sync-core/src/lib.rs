//! # putio-sync-core
//!
//! Core library for putio-sync containing all domain logic, API client,
//! sync planning, and local state management.
//!
//! This crate provides the foundational components for interacting with
//! the Put.io API and managing local sync state, without any UI or CLI
//! concerns.

#![warn(missing_docs)]

/// Structured logging infrastructure
pub mod logging;

/// Re-export commonly used types
pub mod prelude {
    pub use crate::logging;
}
