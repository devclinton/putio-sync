//! File watching and torrent management for Put.io sync
//!
//! This crate provides filesystem watching functionality for automatically
//! detecting and uploading .torrent and .magnet files to Put.io.

pub mod error;
pub mod watcher;

// Re-export commonly used types
pub use error::{FileManagerError, FileManagerResult};
pub use watcher::{
    calculate_file_checksum, calculate_string_checksum, ProcessOutcome, TorrentWatchCore,
    TorrentWatcher, WatchStats,
};
