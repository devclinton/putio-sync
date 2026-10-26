//! Daemon orchestrator for Put.io sync
//!
//! This crate provides the main daemon that coordinates all Put.io sync operations:
//! - File watching for automatic torrent/magnet upload
//! - Download synchronization
//! - API server integration (when enabled)
//!
//! # Example
//!
//! ```no_run
//! use putio_sync_daemon::{DaemonConfig, PutioDaemon};
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = DaemonConfig {
//!         oauth_token: "your-token".to_string(),
//!         download: putio_sync_download::DownloadConfig {
//!             target_directory: PathBuf::from("/downloads"),
//!             ..Default::default()
//!         },
//!         watch_dirs: vec![PathBuf::from("/watch")],
//!         ..Default::default()
//!     };
//!
//!     let mut daemon = PutioDaemon::new(config);
//!     daemon.initialize().await?;
//!     daemon.start().await?;
//!
//!     Ok(())
//! }
//! ```

pub mod daemon;
pub mod error;
pub mod schedule;

// Re-export commonly used types
pub use daemon::{DaemonConfig, PutioDaemon};
pub use error::{DaemonError, DaemonResult};
