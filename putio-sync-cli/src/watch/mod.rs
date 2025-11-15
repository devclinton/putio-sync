//! Filesystem watching for .torrent files

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use putio_sync_core::api::ApiClient;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// File watcher for .torrent files
pub struct TorrentWatcher {
    /// Directories being watched
    watch_dirs: Vec<PathBuf>,
    /// Files that have been processed
    processed_files: Arc<Mutex<HashSet<PathBuf>>>,
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
            processed_files: Arc::new(Mutex::new(HashSet::new())),
            parent_id,
            interval: Duration::from_secs(interval_seconds),
        }
    }

    /// Run the watcher in daemon mode
    ///
    /// Watches directories for new .torrent files and automatically uploads them.
    /// Runs until interrupted with Ctrl+C.
    pub async fn run_daemon(&self, client: &ApiClient) -> Result<()> {
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
        info!("Processing existing .torrent files...");
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
        let watcher = notify::recommended_watcher(tx)
            .context("Failed to create filesystem watcher")?;
        Ok(watcher)
    }

    /// Process filesystem events
    async fn process_events(
        &self,
        rx: &Receiver<notify::Result<Event>>,
        client: &ApiClient,
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
    async fn handle_event(&self, event: Event, client: &ApiClient) -> Result<()> {
        debug!("Filesystem event: {:?}", event.kind);

        // Only process Create and Modify events
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                for path in event.paths {
                    if self.is_torrent_file(&path) {
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

    /// Process existing .torrent files in watch directories
    async fn process_existing_files(&self, client: &ApiClient) -> Result<()> {
        for dir in &self.watch_dirs {
            if !dir.exists() || !dir.is_dir() {
                continue;
            }

            debug!("Scanning directory: {}", dir.display());

            let files = self.find_torrent_files(dir)?;
            if !files.is_empty() {
                info!("Found {} existing .torrent file(s) in {}", files.len(), dir.display());

                for file in files {
                    if let Err(e) = self.process_file(&file, client).await {
                        error!("Failed to process {}: {}", file.display(), e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Process a single .torrent file
    async fn process_file(&self, path: &Path, client: &ApiClient) -> Result<()> {
        // Check if already processed
        {
            let processed = self.processed_files.lock().unwrap();
            if processed.contains(path) {
                debug!("File already processed: {}", path.display());
                return Ok(());
            }
        }

        // Verify file still exists and is a torrent
        if !path.exists() {
            debug!("File no longer exists: {}", path.display());
            return Ok(());
        }

        if !self.is_torrent_file(path) {
            debug!("Not a .torrent file: {}", path.display());
            return Ok(());
        }

        info!("Processing new .torrent file: {}", path.display());

        // Upload the file
        match client.add_transfer_file(path, self.parent_id, None).await {
            Ok(transfer) => {
                println!(
                    "✓ Transfer added: {} (ID: {})",
                    transfer.name, transfer.id
                );
                println!("  File: {}", path.display());
                println!("  Status: {}", transfer.status);
                println!("  Size: {}", format_bytes(transfer.size));
                if let Some(parent) = self.parent_id {
                    println!("  Parent folder: {}", parent);
                }

                // Mark as processed
                let mut processed = self.processed_files.lock().unwrap();
                processed.insert(path.to_path_buf());

                Ok(())
            }
            Err(e) => {
                error!("Failed to upload {}: {}", path.display(), e);
                Err(e.into())
            }
        }
    }

    /// Find all .torrent files in a directory
    fn find_torrent_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let mut torrent_files = Vec::new();

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && self.is_torrent_file(&path) {
                torrent_files.push(path);
            }
        }

        Ok(torrent_files)
    }

    /// Check if a path is a .torrent file
    fn is_torrent_file(&self, path: &Path) -> bool {
        path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("torrent")
    }
}

/// Format bytes as human-readable string
fn format_bytes(bytes: i64) -> String {
    const KB: i64 = 1024;
    const MB: i64 = KB * 1024;
    const GB: i64 = MB * 1024;
    const TB: i64 = GB * 1024;

    if bytes == 0 {
        return "Unknown".to_string();
    } else if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_torrent_file() {
        let watcher = TorrentWatcher::new(vec![], None, 60);

        // Create temp files for testing
        let temp_dir = std::env::temp_dir();

        // Test with actual torrent file path
        let torrent_path = temp_dir.join("test.torrent");
        std::fs::write(&torrent_path, b"test content").unwrap();
        assert!(watcher.is_torrent_file(&torrent_path));
        std::fs::remove_file(&torrent_path).unwrap();

        // Test with non-torrent file
        let txt_path = temp_dir.join("test.txt");
        std::fs::write(&txt_path, b"test content").unwrap();
        assert!(!watcher.is_torrent_file(&txt_path));
        std::fs::remove_file(&txt_path).unwrap();

        // Test with directory
        let dir_path = temp_dir.join("test_dir");
        std::fs::create_dir_all(&dir_path).unwrap();
        assert!(!watcher.is_torrent_file(&dir_path));
        std::fs::remove_dir(&dir_path).unwrap();
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "Unknown");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048576), "1.00 MB");
        assert_eq!(format_bytes(1073741824), "1.00 GB");
        assert_eq!(format_bytes(1099511627776), "1.00 TB");
    }
}
