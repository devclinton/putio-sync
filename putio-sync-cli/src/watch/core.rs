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
use tracing::{debug, error, info, warn};

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
        archive_dir: Option<&PathBuf>,
        delete_after_upload: bool,
        database: Option<&putio_sync_core::db::Database>,
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
                match self.process_torrent_file(&file_path, client, parent_id, database).await {
                    Ok(true) => {
                        stats.files_processed += 1;
                        info!("Successfully processed: {}", file_path.display());
                        
                        // Handle post-upload actions
                        if let Err(e) = self.handle_post_upload_action(&file_path, archive_dir, delete_after_upload).await {
                            error!("Failed to handle post-upload action for {}: {}", file_path.display(), e);
                        }
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
        database: Option<&putio_sync_core::db::Database>,
    ) -> Result<bool> {
        // Check if already processed using checksum if database is available
        if let Some(db) = database {
            use putio_sync_core::db::torrents::{calculate_file_checksum, calculate_string_checksum};
            
            let checksum = if self.is_magnet_file(path) {
                // For magnet files, calculate checksum of the magnet link content
                match tokio::fs::read_to_string(path).await {
                    Ok(magnet_content) => {
                        let trimmed = magnet_content.trim();
                        if trimmed.starts_with("magnet:") {
                            calculate_string_checksum(trimmed)
                        } else {
                            debug!("Invalid magnet file content in {}: doesn't start with 'magnet:'", path.display());
                            return Ok(false);
                        }
                    }
                    Err(e) => {
                        debug!("Failed to read magnet file {}: {}", path.display(), e);
                        return Ok(false);
                    }
                }
            } else {
                // For torrent files, calculate checksum of the binary content
                match calculate_file_checksum(path).await {
                    Ok(checksum) => checksum,
                    Err(e) => {
                        debug!("Failed to calculate checksum for {}: {}", path.display(), e);
                        // Fall back to path-based check
                        String::new()
                    }
                }
            };
            
            if !checksum.is_empty() {
                let repo = db.processed_torrents();
                if repo.is_processed(&checksum).await.unwrap_or(false) {
                    debug!("File already processed (checksum match): {}", path.display());
                    // Also add to in-memory set for consistency
                    let mut processed = self.processed_files.lock().unwrap();
                    processed.insert(path.to_path_buf());
                    return Ok(false);
                }
            }
        }
        
        // Check if already processed (in-memory fallback)
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

        if !self.is_torrent_file(path) && !self.is_magnet_file(path) {
            debug!("Not a .torrent or .magnet file: {}", path.display());
            return Ok(false);
        }

        let file_type = if self.is_magnet_file(path) { "magnet" } else { "torrent" };
        info!("🔼 Processing new .{} file: {}", file_type, path.display());

        // Process based on file type
        let transfer_result = if self.is_magnet_file(path) {
            // Read magnet link from file and add as URL
            match tokio::fs::read_to_string(path).await {
                Ok(magnet_content) => {
                    let magnet_link = magnet_content.trim();
                    if magnet_link.starts_with("magnet:") {
                        client.add_transfer_url(magnet_link, parent_id, None).await
                    } else {
                        return Err(anyhow::anyhow!("Invalid magnet file content: doesn't start with 'magnet:'"));
                    }
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Failed to read magnet file: {}", e));
                }
            }
        } else {
            // Upload torrent file
            client.add_transfer_file(path, parent_id, None).await
        };

        match transfer_result {
            Ok(transfer) => {
                info!(
                    "✅ {} uploaded successfully: {} (ID: {})",
                    if self.is_magnet_file(path) { "Magnet link" } else { "Torrent" },
                    transfer.name, transfer.id
                );
                info!("   File: {}", path.display());
                info!("   Status: {}", transfer.status);
                info!("   Size: {}", self.format_bytes(transfer.size));
                if let Some(parent) = parent_id {
                    info!("   Parent folder ID: {}", parent);
                }

                // Mark as processed
                {
                    let mut processed = self.processed_files.lock().unwrap();
                    processed.insert(path.to_path_buf());
                }

                // Record in database if available
                if let Some(db) = database {
                    use putio_sync_core::db::torrents::{calculate_file_checksum, calculate_string_checksum};
                    
                    let (checksum, magnet_link) = if self.is_magnet_file(path) {
                        // For magnet files, record the magnet link and use content checksum
                        match tokio::fs::read_to_string(path).await {
                            Ok(content) => {
                                let magnet = content.trim().to_string();
                                let checksum = calculate_string_checksum(&magnet);
                                (checksum, Some(magnet))
                            }
                            Err(e) => {
                                debug!("Failed to re-read magnet file for database recording: {}", e);
                                return Ok(true); // Still consider it processed
                            }
                        }
                    } else {
                        // For torrent files, calculate file checksum and no magnet link
                        match calculate_file_checksum(path).await {
                            Ok(checksum) => (checksum, None),
                            Err(e) => {
                                debug!("Failed to calculate checksum for database recording: {}", e);
                                return Ok(true); // Still consider it processed
                            }
                        }
                    };
                    
                    let repo = db.processed_torrents();
                    let file_size = path.metadata().map(|m| m.len()).unwrap_or(0) as i64;
                    
                    if let Err(e) = repo.record_processed(
                        path.to_str().unwrap_or("unknown"),
                        &checksum,
                        file_size,
                        magnet_link.as_deref(),
                    ).await {
                        debug!("Failed to record processed {} in database: {}", file_type, e);
                    } else {
                        debug!("Recorded {} in database: checksum {}", file_type, checksum);
                    }
                }

                Ok(true)
            }
            Err(e) => {
                // Check if this is a "TRANSFER_ALREADY_ADDED" error
                let error_str = format!("{}", e);
                if error_str.contains("TRANSFER_ALREADY_ADDED") {
                    warn!("⚠️  Transfer already exists for {}, treating as success for archive/delete", path.display());
                    
                    // Mark as processed
                    {
                        let mut processed = self.processed_files.lock().unwrap();
                        processed.insert(path.to_path_buf());
                    }

                    // Record in database if available (same logic as success case)
                    if let Some(db) = database {
                        use putio_sync_core::db::torrents::{calculate_file_checksum, calculate_string_checksum};
                        
                        let (checksum, magnet_link) = if self.is_magnet_file(path) {
                            // For magnet files, record the magnet link and use content checksum
                            match tokio::fs::read_to_string(path).await {
                                Ok(content) => {
                                    let magnet = content.trim().to_string();
                                    let checksum = calculate_string_checksum(&magnet);
                                    (checksum, Some(magnet))
                                }
                                Err(e) => {
                                    debug!("Failed to re-read magnet file for database recording: {}", e);
                                    return Ok(true); // Still consider it processed
                                }
                            }
                        } else {
                            // For torrent files, calculate file checksum and no magnet link
                            match calculate_file_checksum(path).await {
                                Ok(checksum) => (checksum, None),
                                Err(e) => {
                                    debug!("Failed to calculate checksum for database recording: {}", e);
                                    return Ok(true); // Still consider it processed
                                }
                            }
                        };
                        
                        let repo = db.processed_torrents();
                        let file_size = path.metadata().map(|m| m.len()).unwrap_or(0) as i64;
                        
                        if let Err(e) = repo.record_processed(
                            path.to_str().unwrap_or("unknown"),
                            &checksum,
                            file_size,
                            magnet_link.as_deref(),
                        ).await {
                            debug!("Failed to record processed {} in database: {}", file_type, e);
                        } else {
                            debug!("Recorded {} in database: checksum {}", file_type, checksum);
                        }
                    }

                    Ok(true) // Treat as success for archive/delete
                } else {
                    error!("❌ Failed to upload {}: {}", path.display(), e);
                    Err(e.into())
                }
            }
        }
    }

    /// Find all .torrent and .magnet files in a directory (non-recursive)
    pub fn find_torrent_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let mut torrent_files = Vec::new();

        debug!("Searching for .torrent and .magnet files in: {}", dir.display());

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && (self.is_torrent_file(&path) || self.is_magnet_file(&path)) {
                debug!("Found torrent/magnet file: {}", path.display());
                torrent_files.push(path);
            }
        }

        debug!("Found {} torrent/magnet files in {}", torrent_files.len(), dir.display());
        Ok(torrent_files)
    }

    /// Check if a path is a .torrent file
    pub fn is_torrent_file(&self, path: &Path) -> bool {
        path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("torrent")
    }

    /// Check if a path is a .magnet file
    pub fn is_magnet_file(&self, path: &Path) -> bool {
        path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("magnet")
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

    /// Handle post-upload actions (archive or delete)
    async fn handle_post_upload_action(
        &self,
        file_path: &PathBuf,
        archive_dir: Option<&PathBuf>,
        delete_after_upload: bool,
    ) -> Result<()> {
        if let Some(archive_path) = archive_dir {
            // Archive the file
            let filename = file_path.file_name()
                .ok_or_else(|| anyhow::anyhow!("Invalid filename"))?;
            let archive_file_path = archive_path.join(filename);
            
            info!("📦 Archiving {} to {}", file_path.display(), archive_file_path.display());
            
            // Ensure archive directory exists
            if let Some(parent) = archive_file_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            
            // Move file to archive directory
            tokio::fs::rename(file_path, &archive_file_path).await?;
            info!("✅ Successfully archived: {}", archive_file_path.display());
            
        } else if delete_after_upload {
            // Delete the file
            info!("🗑️  Deleting processed file: {}", file_path.display());
            tokio::fs::remove_file(file_path).await?;
            info!("✅ Successfully deleted: {}", file_path.display());
        }
        // If neither archive nor delete is enabled, leave file in place
        
        Ok(())
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
        let magnet_file = temp_dir.path().join("test.magnet");
        let other_file = temp_dir.path().join("test.txt");
        
        fs::write(&torrent_file, b"test torrent content").unwrap();
        fs::write(&magnet_file, "magnet:?xt=urn:btih:test").unwrap();
        fs::write(&other_file, b"test text content").unwrap();
        
        assert!(core.is_torrent_file(&torrent_file));
        assert!(!core.is_torrent_file(&magnet_file));
        assert!(!core.is_torrent_file(&other_file));
        
        assert!(core.is_magnet_file(&magnet_file));
        assert!(!core.is_magnet_file(&torrent_file));
        assert!(!core.is_magnet_file(&other_file));
    }

    #[test]
    fn test_find_torrent_files() {
        let core = TorrentWatchCore::new();
        let temp_dir = TempDir::new().unwrap();
        
        // Create test files
        let torrent1 = temp_dir.path().join("test1.torrent");
        let torrent2 = temp_dir.path().join("test2.torrent");
        let magnet1 = temp_dir.path().join("test1.magnet");
        let magnet2 = temp_dir.path().join("test2.magnet");
        let other_file = temp_dir.path().join("test.txt");
        
        fs::write(&torrent1, b"test torrent 1").unwrap();
        fs::write(&torrent2, b"test torrent 2").unwrap();
        fs::write(&magnet1, "magnet:?xt=urn:btih:test1").unwrap();
        fs::write(&magnet2, "magnet:?xt=urn:btih:test2").unwrap();
        fs::write(&other_file, b"test text").unwrap();
        
        let torrent_files = core.find_torrent_files(temp_dir.path()).unwrap();
        
        assert_eq!(torrent_files.len(), 4); // 2 .torrent + 2 .magnet files
        assert!(torrent_files.contains(&torrent1));
        assert!(torrent_files.contains(&torrent2));
        assert!(torrent_files.contains(&magnet1));
        assert!(torrent_files.contains(&magnet2));
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