//! # putio-sync-archive
//!
//! Long-term copies of putio-sync's resource telemetry (feature 041): `archive metrics`
//! saves the 5-minute Prometheus series of each finished UTC day, `archive db` saves a
//! compact copy of the database, and `archive summary` writes CSV usage reports. The CLI
//! wires the three to `putio-sync archive`.

pub mod db;
pub mod error;
pub mod files;
pub mod metrics;
pub mod openmetrics;
pub mod prom;
pub mod summary;

pub use error::{ArchiveError, ArchiveResult};

/// 5-minute points in a UTC day
pub const POINTS_PER_DAY: u64 = 288;
