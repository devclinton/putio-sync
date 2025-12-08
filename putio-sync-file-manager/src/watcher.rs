//! Filesystem watching for .torrent and .magnet files
//!
//! This module provides functionality for watching directories and processing
//! torrent and magnet files. It supports both:
//! - Continuous daemon mode (TorrentWatcher)
//! - Periodic check mode (scan_once)

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use putio_sync_client::PutioApiClient;
use putio_sync_db::Database;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Statistics for a watch operation
#[derive(Debug, Clone, Default)]
pub struct WatchStats {
    /// Number of directories scanned
    pub directories_scanned: usize,
    /// Number of .torrent/.magnet files found
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
    /// This method scans all directories once, processes any .torrent/.magnet files found,
    /// and returns statistics about the operation. It's designed for periodic checks.
    pub async fn scan_directories(
        &self,
        directories: &[PathBuf],
        client: &PutioApiClient,
        parent_id: Option<i64>,
        archive_dir: Option<&PathBuf>,
        delete_after_upload: bool,
        database: Option<&Database>,
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
                info!(
                    "Found {} .torrent/.magnet file(s) in {}",
                    torrent_files.len(),
                    dir.display()
                );
            }

            // Process each torrent file
            for file_path in torrent_files {
                match self
                    .process_torrent_file(&file_path, client, parent_id, database)
                    .await
                {
                    Ok(true) => {
                        stats.files_processed += 1;
                        info!("Successfully processed: {}", file_path.display());

                        // Handle post-upload actions
                        if let Err(e) = self
                            .handle_post_upload_action(&file_path, archive_dir, delete_after_upload)
                            .await
                        {
                            error!(
                                "Failed to handle post-upload action for {}: {}",
                                file_path.display(),
                                e
                            );
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

    /// Process a single .torrent or .magnet file
    ///
    /// Returns Ok(true) if the file was processed, Ok(false) if skipped (already processed),
    /// or Err if there was an error.
    pub async fn process_torrent_file(
        &self,
        path: &Path,
        client: &PutioApiClient,
        parent_id: Option<i64>,
        database: Option<&Database>,
    ) -> Result<bool> {
        // Check if already processed using checksum if database is available
        if let Some(db) = database {
            let checksum = if self.is_magnet_file(path) {
                // For magnet files, calculate checksum of the magnet link content
                match tokio::fs::read_to_string(path).await {
                    Ok(magnet_content) => {
                        let trimmed = magnet_content.trim();
                        if trimmed.starts_with("magnet:") {
                            calculate_string_checksum(trimmed)
                        } else {
                            debug!(
                                "Invalid magnet file content in {}: doesn't start with 'magnet:'",
                                path.display()
                            );
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
                    debug!(
                        "File already processed (checksum match): {}",
                        path.display()
                    );
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

        // Verify file still exists and is a torrent/magnet
        if !path.exists() {
            debug!("File no longer exists: {}", path.display());
            return Ok(false);
        }

        if !self.is_torrent_file(path) && !self.is_magnet_file(path) {
            debug!("Not a .torrent or .magnet file: {}", path.display());
            return Ok(false);
        }

        let file_type = if self.is_magnet_file(path) {
            "magnet"
        } else {
            "torrent"
        };
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
                        return Err(anyhow::anyhow!(
                            "Invalid magnet file content: doesn't start with 'magnet:'"
                        ));
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
                    if self.is_magnet_file(path) {
                        "Magnet link"
                    } else {
                        "Torrent"
                    },
                    transfer.name,
                    transfer.id
                );
                info!("   File: {}", path.display());
                info!("   Status: {}", transfer.status);
                info!("   Size: {}", self.format_bytes(transfer.size.unwrap_or(0)));
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
                    let (checksum, magnet_link) = if self.is_magnet_file(path) {
                        // For magnet files, record the magnet link and use content checksum
                        match tokio::fs::read_to_string(path).await {
                            Ok(content) => {
                                let magnet = content.trim().to_string();
                                let checksum = calculate_string_checksum(&magnet);
                                (checksum, Some(magnet))
                            }
                            Err(e) => {
                                debug!(
                                    "Failed to re-read magnet file for database recording: {}",
                                    e
                                );
                                return Ok(true); // Still consider it processed
                            }
                        }
                    } else {
                        // For torrent files, calculate file checksum and no magnet link
                        match calculate_file_checksum(path).await {
                            Ok(checksum) => (checksum, None),
                            Err(e) => {
                                debug!(
                                    "Failed to calculate checksum for database recording: {}",
                                    e
                                );
                                return Ok(true); // Still consider it processed
                            }
                        }
                    };

                    let repo = db.processed_torrents();
                    let file_size = path.metadata().map(|m| m.len()).unwrap_or(0) as i64;

                    if let Err(e) = repo
                        .record_processed(
                            path.to_str().unwrap_or("unknown"),
                            &checksum,
                            file_size,
                            magnet_link.as_deref(),
                        )
                        .await
                    {
                        debug!(
                            "Failed to record processed {} in database: {}",
                            file_type, e
                        );
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
                        let (checksum, magnet_link) = if self.is_magnet_file(path) {
                            match tokio::fs::read_to_string(path).await {
                                Ok(content) => {
                                    let magnet = content.trim().to_string();
                                    let checksum = calculate_string_checksum(&magnet);
                                    (checksum, Some(magnet))
                                }
                                Err(e) => {
                                    debug!(
                                        "Failed to re-read magnet file for database recording: {}",
                                        e
                                    );
                                    return Ok(true); // Still consider it processed
                                }
                            }
                        } else {
                            match calculate_file_checksum(path).await {
                                Ok(checksum) => (checksum, None),
                                Err(e) => {
                                    debug!(
                                        "Failed to calculate checksum for database recording: {}",
                                        e
                                    );
                                    return Ok(true); // Still consider it processed
                                }
                            }
                        };

                        let repo = db.processed_torrents();
                        let file_size = path.metadata().map(|m| m.len()).unwrap_or(0) as i64;

                        if let Err(e) = repo
                            .record_processed(
                                path.to_str().unwrap_or("unknown"),
                                &checksum,
                                file_size,
                                magnet_link.as_deref(),
                            )
                            .await
                        {
                            debug!(
                                "Failed to record processed {} in database: {}",
                                file_type, e
                            );
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

        debug!(
            "Searching for .torrent and .magnet files in: {}",
            dir.display()
        );

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && (self.is_torrent_file(&path) || self.is_magnet_file(&path)) {
                debug!("Found torrent/magnet file: {}", path.display());
                torrent_files.push(path);
            }
        }

        debug!(
            "Found {} torrent/magnet files in {}",
            torrent_files.len(),
            dir.display()
        );
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

    /// Get access to processed files set
    pub fn processed_files(&self) -> Arc<Mutex<HashSet<PathBuf>>> {
        self.processed_files.clone()
    }

    /// Clear the processed files set
    pub fn clear_processed_files(&self) {
        let mut processed = self.processed_files.lock().unwrap();
        processed.clear()
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
            let filename = file_path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Invalid filename"))?;
            let archive_file_path = archive_path.join(filename);

            info!(
                "📦 Archiving {} to {}",
                file_path.display(),
                archive_file_path.display()
            );

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

/// Calculate SHA-256 checksum of file content
pub async fn calculate_file_checksum(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};

    let content = tokio::fs::read(path).await?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}

/// Calculate SHA-256 checksum of a string
pub fn calculate_string_checksum(content: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}

/// File watcher for .torrent and .magnet files (continuous daemon mode)
pub struct TorrentWatcher {
    /// Directories being watched
    watch_dirs: Vec<PathBuf>,
    /// Core watch functionality
    core: TorrentWatchCore,
    /// Parent folder ID for transfers
    parent_id: Option<i64>,
    /// Check interval for polling mode
    interval: Duration,
}

impl TorrentWatcher {
    /// Create a new torrent watcher
    ///
    /// # Arguments
    ///
    /// * `watch_dirs` - Directories to watch
    /// * `parent_id` - Optional parent folder ID for transfers
    /// * `interval_seconds` - Check interval in seconds
    pub fn new(watch_dirs: Vec<PathBuf>, parent_id: Option<i64>, interval_seconds: u64) -> Self {
        Self {
            watch_dirs,
            core: TorrentWatchCore::new(),
            parent_id,
            interval: Duration::from_secs(interval_seconds),
        }
    }

    /// Perform a single scan check (for use by unified daemon)
    ///
    /// This is a non-blocking operation that scans all watch directories once
    /// and processes any new .torrent/.magnet files found.
    pub async fn scan_once(&self, client: &PutioApiClient) -> Result<WatchStats> {
        self.core
            .scan_directories(&self.watch_dirs, client, self.parent_id, None, false, None)
            .await
    }

    /// Run the watcher in daemon mode
    ///
    /// Watches directories for new .torrent/.magnet files and automatically uploads them.
    /// Runs until interrupted with Ctrl+C.
    pub async fn run_daemon(&self, client: &PutioApiClient) -> Result<()> {
        info!("Starting torrent watcher daemon");
        info!("Watching directories: {:?}", self.watch_dirs);
        info!("Check interval: {:?}", self.interval);

        // Validate watch directories
        for dir in &self.watch_dirs {
            if !dir.exists() {
                warn!("Watch directory does not exist: {}", dir.display());
            } else if !dir.is_dir() {
                warn!("Path is not a directory: {}", dir.display());
            } else {
                info!("Watching: {}", dir.display());
            }
        }

        // Set up Ctrl+C handler
        let running = Arc::new(Mutex::new(true));
        let r = running.clone();

        ctrlc::set_handler(move || {
            info!("Received Ctrl+C, shutting down gracefully...");
            *r.lock().unwrap() = false;
        })
        .context("Failed to set Ctrl+C handler")?;

        // Create filesystem watcher
        let (tx, rx) = channel();
        let mut watcher = self.create_watcher(tx)?;

        // Watch all directories
        for dir in &self.watch_dirs {
            if dir.exists() && dir.is_dir() {
                watcher
                    .watch(dir, RecursiveMode::NonRecursive)
                    .with_context(|| format!("Failed to watch directory: {}", dir.display()))?;
                debug!("Started watching: {}", dir.display());
            }
        }

        // Process initial files
        info!("Processing existing .torrent/.magnet files...");
        self.process_existing_files(client).await?;

        // Main watch loop
        let mut cycle = 0;
        while *running.lock().unwrap() {
            cycle += 1;
            debug!("Daemon cycle {}", cycle);

            // Process filesystem events
            self.process_events(&rx, client).await?;

            // Sleep for the interval, but check running flag every second
            for _ in 0..self.interval.as_secs() {
                if !*running.lock().unwrap() {
                    break;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }

        info!("Torrent watcher daemon stopped");
        Ok(())
    }

    /// Create a filesystem watcher
    fn create_watcher(
        &self,
        tx: Sender<notify::Result<Event>>,
    ) -> Result<notify::RecommendedWatcher> {
        let watcher =
            notify::recommended_watcher(tx).context("Failed to create filesystem watcher")?;
        Ok(watcher)
    }

    /// Process filesystem events
    async fn process_events(
        &self,
        rx: &Receiver<notify::Result<Event>>,
        client: &PutioApiClient,
    ) -> Result<()> {
        // Process all available events (non-blocking)
        while let Ok(result) = rx.try_recv() {
            match result {
                Ok(event) => {
                    if let Err(e) = self.handle_event(event, client).await {
                        error!("Error handling event: {}", e);
                    }
                }
                Err(e) => {
                    error!("Filesystem watch error: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Handle a filesystem event
    async fn handle_event(&self, event: Event, client: &PutioApiClient) -> Result<()> {
        debug!("Filesystem event: {:?}", event.kind);

        // Only process Create and Modify events
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                for path in event.paths {
                    if self.is_torrent_file(&path) || self.is_magnet_file(&path) {
                        self.process_file(&path, client).await?;
                    }
                }
            }
            _ => {
                // Ignore other events (delete, access, etc.)
            }
        }

        Ok(())
    }

    /// Process existing .torrent and .magnet files in watch directories
    async fn process_existing_files(&self, client: &PutioApiClient) -> Result<()> {
        let stats = self
            .core
            .scan_directories(&self.watch_dirs, client, self.parent_id, None, false, None)
            .await?;

        info!(
            "Initial scan completed - {} files processed, {} skipped, {} failed",
            stats.files_processed, stats.files_skipped, stats.files_failed
        );

        Ok(())
    }

    /// Process a single .torrent or .magnet file
    async fn process_file(&self, path: &Path, client: &PutioApiClient) -> Result<()> {
        match self
            .core
            .process_torrent_file(path, client, self.parent_id, None)
            .await
        {
            Ok(true) => Ok(()),  // Successfully processed
            Ok(false) => Ok(()), // Skipped (already processed)
            Err(e) => Err(e),    // Error occurred
        }
    }

    /// Check if a path is a .torrent file
    fn is_torrent_file(&self, path: &Path) -> bool {
        self.core.is_torrent_file(path)
    }

    /// Check if a path is a .magnet file
    fn is_magnet_file(&self, path: &Path) -> bool {
        self.core.is_magnet_file(path)
    }
}
