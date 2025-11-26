//! Filesystem watching for .torrent files

pub mod core;

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use putio_sync_core::api::ApiClient;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{debug, error, info, warn};

pub use core::{TorrentWatchCore, WatchStats};

/// File watcher for .torrent files (continuous daemon mode)
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
    /// and processes any new .torrent files found.
    pub async fn scan_once(&self, client: &ApiClient) -> Result<WatchStats> {
        self.core.scan_directories(&self.watch_dirs, client, self.parent_id).await
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
        let stats = self.core.scan_directories(&self.watch_dirs, client, self.parent_id).await?;
        
        info!("Initial scan completed - {} files processed, {} skipped, {} failed", 
              stats.files_processed, stats.files_skipped, stats.files_failed);
        
        Ok(())
    }

    /// Process a single .torrent file
    async fn process_file(&self, path: &Path, client: &ApiClient) -> Result<()> {
        match self.core.process_torrent_file(path, client, self.parent_id).await {
            Ok(true) => Ok(()),  // Successfully processed
            Ok(false) => Ok(()), // Skipped (already processed)
            Err(e) => Err(e),    // Error occurred
        }
    }


    /// Check if a path is a .torrent file
    fn is_torrent_file(&self, path: &Path) -> bool {
        self.core.is_torrent_file(path)
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

}
