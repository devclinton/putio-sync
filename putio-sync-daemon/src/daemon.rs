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
use tracing::{debug, info, warn};

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
    /// Enable config hot-reloading
    pub enable_hot_reload: bool,
    /// Config file path (for hot-reload)
    pub config_path: Option<PathBuf>,
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
            enable_hot_reload: false,
            config_path: None,
        }
    }
}

/// Main daemon orchestrator
///
/// Coordinates all sync operations and manages the lifecycle of:
/// - API client
/// - Database
/// - Download manager
/// - File watcher
/// - Config watcher
pub struct PutioDaemon {
    /// Current configuration (thread-safe)
    config: Arc<RwLock<DaemonConfig>>,
    /// Put.io API client
    client: Option<Arc<PutioApiClient>>,
    /// Database connection
    database: Option<Arc<Database>>,
    /// Download manager
    download_manager: Option<DownloadManager>,
    /// File watcher for torrents/magnets
    file_watcher: Option<TorrentWatcher>,
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
            download_manager: None,
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

        // Initialize download manager
        let download_config = putio_sync_download::DownloadConfig {
            target_directory: config.download_dir.clone(),
            skip_existing: true,
            buffer_size_mb: 64,
            bandwidth_limit_bytes_per_sec: None,
            max_retries: 3,
            retry_initial_delay_secs: 2,
        };

        self.download_manager = if let Some(ref db) = self.database {
            Some(DownloadManager::new(
                client.clone(),
                db.clone(),
                download_config,
            ))
        } else {
            Some(DownloadManager::new_without_database(
                client.clone(),
                download_config,
            ))
        };
        info!("Download manager initialized");

        // Initialize file watcher
        if !config.watch_dirs.is_empty() {
            let file_watcher = TorrentWatcher::new(
                config.watch_dirs.clone(),
                config.parent_id,
                config.watch_interval,
            );
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
                let watcher = ConfigWatcher::new_with_path(
                    config_path.clone(),
                    move || {
                        info!("Config file changed, reload would be triggered here");
                        // In a full implementation, this would reload the config
                        // and call handle_config_update
                    },
                );
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
    /// This runs the main daemon loop, processing:
    /// - File watch events
    /// - Download synchronization
    /// - Config updates
    pub async fn start(&mut self) -> DaemonResult<()> {
        info!("Starting Put.io sync daemon");

        // Ensure daemon is initialized
        if self.client.is_none() {
            self.initialize().await?;
        }

        // Start file watcher if configured
        if let Some(ref _file_watcher) = self.file_watcher {
            if let Some(ref _client) = self.client {
                info!("Starting file watcher daemon...");
                // In a full implementation, this would run in a separate task
                // For now, we just log that it would start
                debug!("File watcher would run in background");

                // Example of how it would be used:
                // tokio::spawn(async move {
                //     if let Err(e) = file_watcher.run_daemon(client).await {
                //         error!("File watcher error: {}", e);
                //     }
                // });
            }
        }

        // Start config watcher if configured
        if let Some(ref mut _config_watcher) = self.config_watcher {
            info!("Starting config watcher...");
            // In a full implementation, this would run in a separate task
            debug!("Config watcher would run in background");
        }

        info!("Daemon started successfully");
        info!("Press Ctrl+C to stop");

        // In a full implementation, this would have a main loop
        // that coordinates all operations and handles shutdown

        Ok(())
    }

    /// Handle a configuration update
    ///
    /// This is called when the config file changes (hot-reload)
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
        assert!(daemon.download_manager.is_none());
    }
}
