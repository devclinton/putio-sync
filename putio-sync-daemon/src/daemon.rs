//! Daemon orchestrator for Put.io sync operations
//!
//! The PutioDaemon coordinates all sync operations including:
//! - File watching for torrent/magnet files
//! - Download management and synchronization
//! - Configuration hot-reloading
//! - API server (when enabled)

use crate::config_watcher::ConfigWatcher;
use crate::error::DaemonResult;
use putio_sync_client::PutioApiClient;
use putio_sync_db::Database;
use putio_sync_download::DownloadManager;
use putio_sync_file_manager::TorrentWatcher;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Configuration for the daemon
///
/// This is a simplified config structure. In practice, this would
/// be loaded from the main application's config.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Put.io OAuth token
    pub oauth_token: String,
    /// Database path
    pub database_path: Option<PathBuf>,
    /// Download target directory
    pub download_dir: PathBuf,
    /// Directories to watch for torrents/magnets
    pub watch_dirs: Vec<PathBuf>,
    /// Watch interval in seconds
    pub watch_interval: u64,
    /// Parent folder ID for transfers
    pub parent_id: Option<i64>,
    /// Delete torrent/magnet files after upload
    pub delete_after_upload: bool,
    /// Enable config hot-reloading
    pub enable_hot_reload: bool,
    /// Config file path (for hot-reload)
    pub config_path: Option<PathBuf>,
    /// Number of concurrent download workers
    pub workers: usize,
    /// Buffer size for streaming downloads in megabytes
    pub buffer_size_mb: usize,
    /// Delete files from Put.io after successful download
    pub delete_after_download: bool,
    /// Enable API mode (use API server for downloads instead of local manager)
    pub api_enabled: bool,
    /// API server URL (required if api_enabled is true)
    pub api_url: Option<String>,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            oauth_token: String::new(),
            database_path: None,
            download_dir: PathBuf::from("."),
            watch_dirs: Vec::new(),
            watch_interval: 60,
            parent_id: None,
            delete_after_upload: false,
            enable_hot_reload: false,
            config_path: None,
            workers: 1,
            buffer_size_mb: 64,
            delete_after_download: false,
            api_enabled: false,
            api_url: None,
        }
    }
}

/// Download mode for the daemon
///
/// Determines whether downloads are handled locally or through an API server.
pub enum DownloadMode {
    /// Local mode - daemon manages downloads directly (shared with API for hot reload)
    Local(Arc<DownloadManager>),
    /// Remote mode - daemon uses API server for downloads
    Remote {
        /// API server base URL
        api_url: String,
        /// HTTP client for API requests
        client: reqwest::Client,
    },
}

/// Main daemon orchestrator
///
/// Coordinates all sync operations and manages the lifecycle of:
/// - API client
/// - Database
/// - Download manager (local or remote)
/// - File watcher
/// - Config watcher
pub struct PutioDaemon {
    /// Current configuration (thread-safe)
    config: Arc<RwLock<DaemonConfig>>,
    /// Put.io API client
    client: Option<Arc<PutioApiClient>>,
    /// Database connection
    database: Option<Arc<Database>>,
    /// Download mode (local or remote API)
    download_mode: Option<DownloadMode>,
    /// File watcher for torrents/magnets (shared with API for hot reload)
    file_watcher: Option<Arc<TorrentWatcher>>,
    /// Config file watcher (for hot-reload)
    config_watcher: Option<ConfigWatcher>,
}

impl PutioDaemon {
    /// Create a new daemon with the given configuration
    pub fn new(config: DaemonConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            client: None,
            database: None,
            download_mode: None,
            file_watcher: None,
            config_watcher: None,
        }
    }

    /// Initialize all daemon components
    ///
    /// This sets up:
    /// - API client
    /// - Database connection
    /// - Download manager
    /// - File watcher
    /// - Config watcher (if enabled)
    pub async fn initialize(&mut self) -> DaemonResult<()> {
        let config = self.config.read().await;
        info!("Initializing Put.io sync daemon...");

        // Initialize API client
        let client = Arc::new(PutioApiClient::new(config.oauth_token.clone()));
        self.client = Some(client.clone());
        info!("API client initialized");

        // Initialize database (if path provided)
        if let Some(ref db_path) = config.database_path {
            match Database::new(db_path).await {
                Ok(db) => {
                    let db = Arc::new(db);
                    self.database = Some(db.clone());
                    info!("Database initialized: {}", db_path.display());
                }
                Err(e) => {
                    warn!(
                        "Failed to initialize database at {}: {}",
                        db_path.display(),
                        e
                    );
                    warn!("Continuing without database (download tracking disabled)");
                }
            }
        } else {
            info!("No database path configured, running without persistence");
        }

        // Initialize download mode (local or remote API)
        if config.api_enabled {
            // API mode - use remote API server for downloads
            let api_url = config.api_url.as_ref().ok_or_else(|| {
                crate::error::DaemonError::ConfigError(
                    "API URL is required when api_enabled is true".to_string(),
                )
            })?;

            let http_client = reqwest::Client::new();
            self.download_mode = Some(DownloadMode::Remote {
                api_url: api_url.clone(),
                client: http_client,
            });
            info!("Download mode: Remote API ({})", api_url);
        } else {
            // Local mode - create own download manager
            let download_config = putio_sync_download::DownloadConfig {
                target_directory: config.download_dir.clone(),
                skip_existing: true,
                buffer_size_mb: config.buffer_size_mb,
                bandwidth_limit_bytes_per_sec: None,
                max_retries: 3,
                retry_initial_delay_secs: 2,
                workers: config.workers,
                delete_after_download: config.delete_after_download,
            };

            let download_manager = if let Some(ref db) = self.database {
                DownloadManager::new(client.clone(), db.clone(), download_config)
            } else {
                DownloadManager::new_without_database(client.clone(), download_config)
            };

            // Wrap in Arc for sharing with API (hot reload support via internal config RwLock)
            self.download_mode = Some(DownloadMode::Local(Arc::new(download_manager)));
            info!("Download mode: Local");
        }

        // Initialize file watcher (shared Arc for hot reload via API)
        if !config.watch_dirs.is_empty() {
            let file_watcher = Arc::new(TorrentWatcher::new(
                config.watch_dirs.clone(),
                config.parent_id,
                config.watch_interval,
            ));
            self.file_watcher = Some(file_watcher);
            info!(
                "File watcher initialized for {} directories",
                config.watch_dirs.len()
            );
        } else {
            info!("No watch directories configured, file watching disabled");
        }

        // Initialize config watcher (if enabled)
        if config.enable_hot_reload {
            if let Some(ref config_path) = config.config_path {
                let _daemon_config = self.config.clone();
                let watcher = ConfigWatcher::new_with_path(config_path.clone(), move || {
                    info!("Config file changed, reload would be triggered here");
                    // In a full implementation, this would reload the config
                    // and call handle_config_update
                });
                self.config_watcher = Some(watcher);
                info!("Config watcher initialized: {}", config_path.display());
            } else {
                warn!("Hot-reload enabled but no config path provided");
            }
        }

        info!("Daemon initialization complete");
        Ok(())
    }

    /// Start the daemon
    ///
    /// This spawns independent async tasks for:
    /// - File watch events (torrent/magnet scanner)
    /// - Download synchronization
    /// - Config updates (hot reload)
    ///
    /// Each task manages its own timing and runs independently.
    pub async fn start(&mut self) -> DaemonResult<()> {
        info!("Starting Put.io sync daemon");

        // Ensure daemon is initialized
        if self.client.is_none() {
            self.initialize().await?;
        }

        // Start config watcher in background task if configured
        if let Some(mut config_watcher) = self.config_watcher.take() {
            info!("Starting config watcher...");
            tokio::spawn(async move {
                if let Err(e) = config_watcher.start().await {
                    error!("Config watcher error: {}", e);
                }
            });
            debug!("Config watcher started in background");
        }

        // Spawn watch task if file watcher is configured
        let watch_task = if self.file_watcher.is_some() && self.client.is_some() && self.database.is_some() {
            let config = self.config.read().await;
            let watch_interval = config.watch_interval;
            let delete_after_upload = config.delete_after_upload;
            drop(config);

            // Clone Arc (cheap) - watcher remains accessible for API hot reload
            let watcher = self.file_watcher.clone().unwrap();
            let client = self.client.clone().unwrap();
            let database = self.database.clone().unwrap();

            Some(tokio::spawn(async move {
                Self::run_watch_loop(watcher, client, database, watch_interval, delete_after_upload).await;
            }))
        } else {
            info!("File watcher not configured, skipping watch task");
            None
        };

        // Spawn download tasks if in local mode (not API mode)
        let (sync_task, download_task) = match &self.download_mode {
            Some(DownloadMode::Local(_)) => {
                // Clone Arc (cheap) - download manager remains accessible for API hot reload
                if let Some(DownloadMode::Local(dm)) = &self.download_mode {
                    let config = self.config.read().await;
                    let sync_interval = config.watch_interval; // Interval for syncing from Put.io
                    let worker_interval = config.watch_interval; // Interval for processing queue
                    drop(config);

                    // Clone Arc for the two separate tasks (cheap operation)
                    let dm_sync = dm.clone();
                    let dm_workers = dm.clone();

                    // Spawn sync task (refreshes queue from Put.io)
                    let sync_handle = tokio::spawn(async move {
                        Self::run_sync_loop(dm_sync, sync_interval).await;
                    });

                    // Spawn worker task (processes queue)
                    let worker_handle = tokio::spawn(async move {
                        Self::run_download_loop(dm_workers, worker_interval).await;
                    });

                    (Some(sync_handle), Some(worker_handle))
                } else {
                    unreachable!()
                }
            }
            Some(DownloadMode::Remote { api_url, .. }) => {
                info!("Running in API mode - downloads handled by API server at {}", api_url);
                (None, None)
            }
            None => {
                info!("Download mode not configured, skipping download tasks");
                (None, None)
            }
        };

        info!("Daemon started successfully");
        info!("  - Watch task: {}", if watch_task.is_some() { "running" } else { "disabled" });
        info!("  - Sync task: {}", if sync_task.is_some() { "running" } else { "disabled" });
        info!("  - Download workers task: {}", if download_task.is_some() { "running" } else { "disabled" });
        info!("Press Ctrl+C to stop");

        // Wait for shutdown signal
        tokio::signal::ctrl_c().await
            .map_err(|e| crate::error::DaemonError::InternalError(format!("Failed to listen for Ctrl+C: {}", e)))?;

        info!("Received shutdown signal");

        // Tasks will be automatically cancelled when dropped
        if let Some(task) = watch_task {
            task.abort();
        }
        if let Some(task) = sync_task {
            task.abort();
        }
        if let Some(task) = download_task {
            task.abort();
        }

        info!("Daemon shutting down...");
        Ok(())
    }

    /// Run the watch loop as an independent async task
    async fn run_watch_loop(
        watcher: Arc<TorrentWatcher>,
        client: Arc<PutioApiClient>,
        database: Arc<Database>,
        interval_secs: u64,
        delete_after_upload: bool,
    ) {
        info!("Watch task started with interval: {}s", interval_secs);
        let interval = tokio::time::Duration::from_secs(interval_secs);
        let mut cycle = 0;

        loop {
            cycle += 1;
            debug!("Watch task cycle #{}", cycle);

            // Scan watch directories for new torrent/magnet files
            match watcher.scan_once(&client, delete_after_upload, Some(&database)).await {
                Ok(stats) => {
                    if stats.files_found > 0 {
                        info!(
                            "Watch scan - found: {}, processed: {}, skipped: {}, failed: {}",
                            stats.files_found,
                            stats.files_processed,
                            stats.files_skipped,
                            stats.files_failed
                        );
                    } else {
                        debug!("Watch scan - no new files");
                    }
                }
                Err(e) => {
                    error!("Error scanning watch directories: {}", e);
                }
            }

            // Sleep for the interval
            tokio::time::sleep(interval).await;
        }
    }

    /// Run the sync loop as an independent async task
    ///
    /// Periodically syncs the file list from Put.io to the download queue
    async fn run_sync_loop(download_manager: Arc<DownloadManager>, interval_secs: u64) {
        info!("Put.io sync task started with interval: {}s", interval_secs);
        download_manager.run_sync_loop(interval_secs).await;
    }

    /// Run the download worker loop as an independent async task
    ///
    /// Starts persistent workers that continuously process the download queue.
    /// Workers are dynamically scaled via hot reload when config changes.
    async fn run_download_loop(download_manager: Arc<DownloadManager>, _interval_secs: u64) {
        info!("Starting persistent download workers");

        // Start initial workers based on configuration
        download_manager.start_workers().await;

        // Workers are now persistent and run continuously
        // They will automatically scale up/down when config changes via update_config()
        // Just keep the task alive
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;

            // Log worker status periodically
            let active_workers = download_manager.active_workers().await;
            debug!("Download workers: {} active", active_workers);
        }
    }

    /// Handle a configuration update
    ///
    /// This is called when the config file changes (hot-reload)
    #[allow(dead_code)]
    async fn handle_config_update(&mut self, new_config: DaemonConfig) -> DaemonResult<()> {
        info!("Handling configuration update");

        let mut config = self.config.write().await;
        let _old_config = config.clone();

        // Update the config
        *config = new_config.clone();
        drop(config); // Release the write lock

        // Reinitialize components that depend on changed config
        // This is a simplified version - a full implementation would
        // selectively reinitialize only what's needed

        info!("Configuration update applied");
        Ok(())
    }

    /// Get the current configuration
    pub async fn get_config(&self) -> DaemonConfig {
        self.config.read().await.clone()
    }

    /// Get the API client (for use by API server)
    pub fn get_client(&self) -> Option<Arc<PutioApiClient>> {
        self.client.clone()
    }

    /// Get the database (for use by API server)
    pub fn get_database(&self) -> Option<Arc<Database>> {
        self.database.clone()
    }

    /// Get the download manager if in local mode (for use by API server)
    pub fn get_download_manager(&self) -> Option<Arc<DownloadManager>> {
        match &self.download_mode {
            Some(DownloadMode::Local(dm)) => Some(dm.clone()),
            _ => None,
        }
    }

    /// Get the file watcher (for use by API server hot reload)
    pub fn get_file_watcher(&self) -> Option<Arc<TorrentWatcher>> {
        self.file_watcher.clone()
    }

    /// Shutdown the daemon gracefully
    pub async fn shutdown(&mut self) -> DaemonResult<()> {
        info!("Shutting down daemon");

        // In a full implementation, this would:
        // - Stop file watcher
        // - Stop config watcher
        // - Wait for in-progress downloads
        // - Close database connections
        // - Save state

        info!("Daemon shutdown complete");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_config_default() {
        let config = DaemonConfig::default();
        assert_eq!(config.watch_interval, 60);
        assert!(!config.enable_hot_reload);
        assert!(config.watch_dirs.is_empty());
    }

    #[test]
    fn test_daemon_creation() {
        let config = DaemonConfig::default();
        let daemon = PutioDaemon::new(config);
        assert!(daemon.client.is_none());
        assert!(daemon.database.is_none());
        assert!(daemon.download_mode.is_none());
    }
}
