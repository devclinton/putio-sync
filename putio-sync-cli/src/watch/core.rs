//! Core torrent watching functionality
//!
//! This module contains the core logic for watching directories and processing
//! .torrent files. It's designed to be reusable between different watch modes:
//! - Continuous daemon mode (TorrentWatcher)
//! - Periodic check mode (used by the unified daemon)

use anyhow::Result;
use putio_sync_core::api::ApiClient;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info};

/// Statistics for a watch operation
#[derive(Debug, Clone, Default)]
pub struct WatchStats {
    /// Number of directories scanned
    pub directories_scanned: usize,
    /// Number of .torrent files found
    pub files_found: usize,
    /// Number of files successfully processed
    pub files_processed: usize,
    /// Number of files that failed to process
    pub files_failed: usize,
    /// Number of files skipped (already processed)
    pub files_skipped: usize,
}

impl WatchStats {
    /// Create new empty stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Get total files encountered
    pub fn total_files(&self) -> usize {
        self.files_processed + self.files_failed + self.files_skipped
    }

    /// Get success rate as percentage
    pub fn success_rate(&self) -> f64 {
        if self.files_found == 0 {
            return 100.0;
        }
        (self.files_processed as f64 / self.files_found as f64) * 100.0
    }
}

/// Core torrent watcher that can be used in different modes
pub struct TorrentWatchCore {
    /// Files that have been processed to avoid duplicates
    processed_files: Arc<Mutex<HashSet<PathBuf>>>,
}

impl TorrentWatchCore {
    /// Create a new torrent watch core
    pub fn new() -> Self {
        Self {
            processed_files: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Create a new torrent watch core with existing processed files
    pub fn with_processed_files(processed_files: Arc<Mutex<HashSet<PathBuf>>>) -> Self {
        Self { processed_files }
    }

    /// Perform a single scan of the specified directories
    ///
    /// This method scans all directories once, processes any .torrent files found,
    /// and returns statistics about the operation. It's designed for periodic checks.
    pub async fn scan_directories(
        &self,
        directories: &[PathBuf],
        client: &ApiClient,
        parent_id: Option<i64>,
    ) -> Result<WatchStats> {
        let mut stats = WatchStats::new();

        debug!("Starting scan of {} directories", directories.len());

        for dir in directories {
            if !dir.exists() {
                debug!("Directory does not exist: {}", dir.display());
                continue;
            }

            if !dir.is_dir() {
                debug!("Path is not a directory: {}", dir.display());
                continue;
            }

            debug!("Scanning directory: {}", dir.display());
            stats.directories_scanned += 1;

            // Find torrent files in this directory
            let torrent_files = self.find_torrent_files(dir)?;
            stats.files_found += torrent_files.len();

            if !torrent_files.is_empty() {
                info!("Found {} .torrent file(s) in {}", torrent_files.len(), dir.display());
            }

            // Process each torrent file
            for file_path in torrent_files {
                match self.process_torrent_file(&file_path, client, parent_id).await {
                    Ok(true) => {
                        stats.files_processed += 1;
                        info!("Successfully processed: {}", file_path.display());
                    }
                    Ok(false) => {
                        stats.files_skipped += 1;
                        debug!("Skipped (already processed): {}", file_path.display());
                    }
                    Err(e) => {
                        stats.files_failed += 1;
                        error!("Failed to process {}: {}", file_path.display(), e);
                    }
                }
            }
        }

        info!(
            "Scan completed - directories: {}, files found: {}, processed: {}, skipped: {}, failed: {}",
            stats.directories_scanned,
            stats.files_found,
            stats.files_processed,
            stats.files_skipped,
            stats.files_failed
        );

        Ok(stats)
    }

    /// Process a single .torrent file
    ///
    /// Returns Ok(true) if the file was processed, Ok(false) if skipped (already processed),
    /// or Err if there was an error.
    pub async fn process_torrent_file(
        &self,
        path: &Path,
        client: &ApiClient,
        parent_id: Option<i64>,
    ) -> Result<bool> {
        // Check if already processed
        {
            let processed = self.processed_files.lock().unwrap();
            if processed.contains(path) {
                debug!("File already processed: {}", path.display());
                return Ok(false);
            }
        }

        // Verify file still exists and is a torrent
        if !path.exists() {
            debug!("File no longer exists: {}", path.display());
            return Ok(false);
        }

        if !self.is_torrent_file(path) {
            debug!("Not a .torrent file: {}", path.display());
            return Ok(false);
        }

        info!("Processing new .torrent file: {}", path.display());

        // Upload the file
        match client.add_transfer_file(path, parent_id, None).await {
            Ok(transfer) => {
                println!(
                    "✓ Transfer added: {} (ID: {})",
                    transfer.name, transfer.id
                );
                println!("  File: {}", path.display());
                println!("  Status: {}", transfer.status);
                println!("  Size: {}", self.format_bytes(transfer.size));
                if let Some(parent) = parent_id {
                    println!("  Parent folder: {}", parent);
                }

                // Mark as processed
                {
                    let mut processed = self.processed_files.lock().unwrap();
                    processed.insert(path.to_path_buf());
                }

                Ok(true)
            }
            Err(e) => {
                error!("Failed to upload {}: {}", path.display(), e);
                Err(e.into())
            }
        }
    }

    /// Find all .torrent files in a directory (non-recursive)
    pub fn find_torrent_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let mut torrent_files = Vec::new();

        debug!("Searching for .torrent files in: {}", dir.display());

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && self.is_torrent_file(&path) {
                debug!("Found torrent file: {}", path.display());
                torrent_files.push(path);
            }
        }

        debug!("Found {} torrent files in {}", torrent_files.len(), dir.display());
        Ok(torrent_files)
    }

    /// Check if a path is a .torrent file
    pub fn is_torrent_file(&self, path: &Path) -> bool {
        path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("torrent")
    }

    /// Get access to processed files set (for compatibility with existing TorrentWatcher)
    pub fn processed_files(&self) -> Arc<Mutex<HashSet<PathBuf>>> {
        self.processed_files.clone()
    }

    /// Clear the processed files set
    pub fn clear_processed_files(&self) {
        let mut processed = self.processed_files.lock().unwrap();
        processed.clear();
    }

    /// Get count of processed files
    pub fn processed_files_count(&self) -> usize {
        let processed = self.processed_files.lock().unwrap();
        processed.len()
    }

    /// Format bytes in human readable format
    fn format_bytes(&self, bytes: i64) -> String {
        if bytes < 0 {
            return "Unknown".to_string();
        }

        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut unit_index = 0;

        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }

        if unit_index == 0 {
            format!("{} {}", bytes, UNITS[unit_index])
        } else {
            format!("{:.2} {}", size, UNITS[unit_index])
        }
    }
}

impl Default for TorrentWatchCore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_is_torrent_file() {
        let core = TorrentWatchCore::new();
        let temp_dir = TempDir::new().unwrap();
        
        // Create test files
        let torrent_file = temp_dir.path().join("test.torrent");
        let other_file = temp_dir.path().join("test.txt");
        
        fs::write(&torrent_file, b"test torrent content").unwrap();
        fs::write(&other_file, b"test text content").unwrap();
        
        assert!(core.is_torrent_file(&torrent_file));
        assert!(!core.is_torrent_file(&other_file));
    }

    #[test]
    fn test_find_torrent_files() {
        let core = TorrentWatchCore::new();
        let temp_dir = TempDir::new().unwrap();
        
        // Create test files
        let torrent1 = temp_dir.path().join("test1.torrent");
        let torrent2 = temp_dir.path().join("test2.torrent");
        let other_file = temp_dir.path().join("test.txt");
        
        fs::write(&torrent1, b"test torrent 1").unwrap();
        fs::write(&torrent2, b"test torrent 2").unwrap();
        fs::write(&other_file, b"test text").unwrap();
        
        let torrent_files = core.find_torrent_files(temp_dir.path()).unwrap();
        
        assert_eq!(torrent_files.len(), 2);
        assert!(torrent_files.contains(&torrent1));
        assert!(torrent_files.contains(&torrent2));
        assert!(!torrent_files.contains(&other_file));
    }

    #[test]
    fn test_watch_stats() {
        let mut stats = WatchStats::new();
        
        stats.directories_scanned = 3;
        stats.files_found = 10;
        stats.files_processed = 8;
        stats.files_failed = 1;
        stats.files_skipped = 1;
        
        assert_eq!(stats.total_files(), 10);
        assert_eq!(stats.success_rate(), 80.0);
    }

    #[test]
    fn test_format_bytes() {
        let core = TorrentWatchCore::new();
        
        assert_eq!(core.format_bytes(0), "0 B");
        assert_eq!(core.format_bytes(1024), "1.00 KB");
        assert_eq!(core.format_bytes(1048576), "1.00 MB");
        assert_eq!(core.format_bytes(1073741824), "1.00 GB");
        assert_eq!(core.format_bytes(-1), "Unknown");
    }
}