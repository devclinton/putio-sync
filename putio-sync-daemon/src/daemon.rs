//! Daemon orchestrator for Put.io sync operations
//!
//! The PutioDaemon coordinates all sync operations including:
//! - File watching for torrent/magnet files
//! - Download management and synchronization
//! - API server (when enabled)

use crate::error::DaemonResult;
use putio_sync_client::PutioApiClient;
use putio_sync_core::cleanup::EmptyFolderCleanup;
use putio_sync_core::stall::StallTracker;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager};
use putio_sync_file_manager::watcher::WatcherConfig;
use putio_sync_file_manager::TorrentWatcher;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
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
    /// Download settings for the local download manager, from `[download]` via
    /// `DownloadConfig::from(&config.download)` (target directory, workers, retries,
    /// bandwidth limit, `skip_existing`, ...)
    pub download: DownloadConfig,
    /// Directories to watch for torrents/magnets
    pub watch_dirs: Vec<PathBuf>,
    /// Watch interval in seconds
    pub watch_interval: u64,
    /// Put.io sync interval in seconds
    pub download_interval: u64,
    /// Parent folder ID for transfers
    pub parent_id: Option<i64>,
    /// Delete torrent/magnet files after upload
    pub delete_after_upload: bool,
    /// Move uploaded torrent/magnet files here (`watch.archive_directory`); wins over
    /// `delete_after_upload`
    pub archive_dir: Option<PathBuf>,
    /// Enable file watcher (can be toggled at runtime via API)
    pub watch_enabled: bool,
    /// Enable API mode (use API server for downloads instead of local manager)
    pub api_enabled: bool,
    /// API server URL (required if api_enabled is true)
    pub api_url: Option<String>,
    /// Time without new bytes before a downloading Put.io transfer is stalled
    pub stall_threshold: Duration,
    /// Seconds between checks of the Put.io transfer list for stalls
    pub stall_check_interval: u64,
}

/// (Put.io sync interval, queue worker interval) in seconds.
/// The sync loop used to run on `watch_interval`, which made `--download-interval` a no-op.
fn loop_intervals(config: &DaemonConfig) -> (u64, u64) {
    (config.download_interval, config.watch_interval)
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            oauth_token: String::new(),
            database_path: None,
            download: DownloadConfig::default(),
            watch_dirs: Vec::new(),
            watch_interval: 60,
            download_interval: 300,
            parent_id: None,
            delete_after_upload: false,
            archive_dir: None,
            watch_enabled: true,
            api_enabled: false,
            api_url: None,
            stall_threshold: Duration::from_secs(6 * 3600),
            stall_check_interval: 600,
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
    /// Empty-folder cleanup run daily (shared with the API)
    cleanup: Option<Arc<EmptyFolderCleanup>>,
    /// Stalled-transfer verdicts, fed by the stall check task and read by the API
    stall_tracker: Arc<StallTracker>,
}

impl PutioDaemon {
    /// Create a new daemon with the given configuration
    pub fn new(config: DaemonConfig) -> Self {
        Self {
            stall_tracker: Arc::new(StallTracker::new(config.stall_threshold)),
            config: Arc::new(RwLock::new(config)),
            client: None,
            database: None,
            download_mode: None,
            file_watcher: None,
            cleanup: None,
        }
    }

    /// Run `cleanup` once a day while its `[cleanup] enabled` is on. The same service
    /// should be given to the API so manual and daily runs never overlap.
    pub fn with_cleanup(mut self, cleanup: Arc<EmptyFolderCleanup>) -> Self {
        self.cleanup = Some(cleanup);
        self
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
            let download_config = config.download.clone();

            let download_manager = if let Some(ref db) = self.database {
                DownloadManager::new(client.clone(), db.clone(), download_config)
            } else {
                DownloadManager::new_without_database(client.clone(), download_config)
            };
            // Before any worker starts: a crash leaves rows in `downloading`, which no
            // worker claims again
            if let Err(e) = download_manager.recover_interrupted().await {
                warn!("{}", e);
            }

            // Wrap in Arc for sharing with API (hot reload support via internal config RwLock)
            self.download_mode = Some(DownloadMode::Local(Arc::new(download_manager)));
            info!("Download mode: Local");
        }

        // Initialize file watcher (shared Arc for hot reload via API)
        if !config.watch_dirs.is_empty() {
            let file_watcher = Arc::new(TorrentWatcher::from_config(WatcherConfig {
                archive_dir: config.archive_dir.clone(),
                delete_after_upload: config.delete_after_upload,
                ..WatcherConfig::new(
                    config.watch_dirs.clone(),
                    config.parent_id,
                    config.watch_interval,
                )
            }));
            self.file_watcher = Some(file_watcher);
            info!(
                "File watcher initialized for {} directories",
                config.watch_dirs.len()
            );
        } else {
            info!("No watch directories configured, file watching disabled");
        }

        info!("Daemon initialization complete");
        Ok(())
    }

    /// Start the daemon
    ///
    /// This spawns independent async tasks for:
    /// - File watch events (torrent/magnet scanner)
    /// - Download synchronization
    ///
    /// Each task manages its own timing and runs independently.
    pub async fn start(&mut self) -> DaemonResult<()> {
        info!("Starting Put.io sync daemon");

        // Ensure daemon is initialized
        if self.client.is_none() {
            self.initialize().await?;
        }

        // Spawn watch task if file watcher is configured
        let watch_task =
            if self.file_watcher.is_some() && self.client.is_some() && self.database.is_some() {
                let config_guard = self.config.read().await;
                let watch_interval = config_guard.watch_interval;
                let delete_after_upload = config_guard.delete_after_upload;
                drop(config_guard);

                // Clone Arc (cheap) - watcher remains accessible for API hot reload
                let watcher = self.file_watcher.clone().unwrap();
                let client = self.client.clone().unwrap();
                let database = self.database.clone().unwrap();
                let config = self.config.clone();

                Some(tokio::spawn(async move {
                    Self::run_watch_loop(
                        watcher,
                        client,
                        database,
                        config,
                        watch_interval,
                        delete_after_upload,
                    )
                    .await;
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
                    let (sync_interval, worker_interval) =
                        loop_intervals(&*self.config.read().await);

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
                info!(
                    "Running in API mode - downloads handled by API server at {}",
                    api_url
                );
                (None, None)
            }
            None => {
                info!("Download mode not configured, skipping download tasks");
                (None, None)
            }
        };

        let cleanup_task = self
            .cleanup
            .clone()
            .map(|cleanup| tokio::spawn(crate::schedule::run_daily(cleanup)));
        // Spawn the stall check task (feature 007): flags Put.io transfers whose bytes stop
        let interval = self.config.read().await.stall_check_interval;
        let stall_task = self.client.clone().map(|client| {
            let tracker = self.stall_tracker.clone();
            tokio::spawn(async move {
                Self::run_stall_loop(client, tracker, interval).await;
            })
        });

        info!("Daemon started successfully");
        info!(
            "  - Watch task: {}",
            if watch_task.is_some() {
                "running"
            } else {
                "disabled"
            }
        );
        info!(
            "  - Sync task: {}",
            if sync_task.is_some() {
                "running"
            } else {
                "disabled"
            }
        );
        info!(
            "  - Download workers task: {}",
            if download_task.is_some() {
                "running"
            } else {
                "disabled"
            }
        );
        match (&cleanup_task, &self.cleanup) {
            (Some(_), Some(cleanup)) => {
                let config = cleanup.config();
                info!(
                    "  - Empty-folder cleanup task: running (daily at {}, {})",
                    config.run_at,
                    if config.enabled {
                        "enabled"
                    } else {
                        "currently disabled"
                    }
                );
            }
            _ => info!("  - Empty-folder cleanup task: disabled"),
        }
        info!("Press Ctrl+C to stop");

        // Wait for shutdown signal
        tokio::signal::ctrl_c().await.map_err(|e| {
            crate::error::DaemonError::InternalError(format!("Failed to listen for Ctrl+C: {}", e))
        })?;

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
        if let Some(task) = cleanup_task {
            task.abort();
        }
        if let Some(task) = stall_task {
            task.abort();
        }

        info!("Daemon shutting down...");
        Ok(())
    }

    /// Check the Put.io transfer list for stalls every `interval_secs`
    async fn run_stall_loop(
        client: Arc<PutioApiClient>,
        tracker: Arc<StallTracker>,
        interval_secs: u64,
    ) {
        info!(interval_secs, "Stall check task started");
        let mut tick = tokio::time::interval(Duration::from_secs(interval_secs.max(1)));
        loop {
            tick.tick().await;
            if let Err(e) = check_stalls(&client, &tracker, SystemTime::now()).await {
                warn!(error = %e, "Stall check failed; keeping previous verdicts");
            }
        }
    }

    /// Run the watch loop as an independent async task
    ///
    /// Checks config.watch_enabled before each scan to support runtime start/stop via API
    async fn run_watch_loop(
        watcher: Arc<TorrentWatcher>,
        client: Arc<PutioApiClient>,
        database: Arc<Database>,
        config: Arc<RwLock<DaemonConfig>>,
        interval_secs: u64,
        delete_after_upload: bool,
    ) {
        info!("Watch task started with interval: {}s", interval_secs);
        let interval = tokio::time::Duration::from_secs(interval_secs);
        let mut cycle = 0;

        loop {
            cycle += 1;

            // Check if watch_enabled in config (supports runtime start/stop via API)
            let watch_enabled = {
                let cfg = config.read().await;
                cfg.watch_enabled
            };

            if !watch_enabled {
                debug!(
                    "Watch task cycle #{} - watcher disabled, skipping scan",
                    cycle
                );
                tokio::time::sleep(interval).await;
                continue;
            }

            debug!("Watch task cycle #{}", cycle);

            // Scan watch directories for new torrent/magnet files
            match watcher
                .scan_once(&client, delete_after_upload, Some(&database))
                .await
            {
                Ok(stats) => {
                    if stats.files_processed + stats.cleaned_up + stats.files_failed > 0 {
                        info!(
                            "Watch scan - found: {}, processed: {}, cleaned up: {}, skipped: {}, failed: {}",
                            stats.files_found,
                            stats.files_processed,
                            stats.cleaned_up,
                            stats.files_skipped,
                            stats.files_failed
                        );
                    } else {
                        debug!("Watch scan - found: {}, nothing to do", stats.files_found);
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
    /// Not called yet: only `[logging]` settings reload at runtime, via
    /// `putio_sync_core::config::spawn_reload_poller`.
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

    /// Update the watch_enabled setting (for use by API server)
    pub async fn set_watch_enabled(&self, enabled: bool) {
        let mut config = self.config.write().await;
        config.watch_enabled = enabled;
        info!("Watch enabled updated to: {}", enabled);
    }

    /// Get the current watch_enabled state
    pub async fn is_watch_enabled(&self) -> bool {
        self.config.read().await.watch_enabled
    }

    /// The stalled-transfer tracker the API reads (feature 007)
    pub fn stall_tracker(&self) -> Arc<StallTracker> {
        self.stall_tracker.clone()
    }

    /// Get the daemon config (for use by API server)
    pub fn get_daemon_config(&self) -> Arc<RwLock<DaemonConfig>> {
        self.config.clone()
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

/// One stall check: fetch the transfer list, update `tracker` as of `now` and the
/// `putio_sync_transfers_stalled` gauge. On a Put.io error nothing changes.
pub async fn check_stalls(
    client: &PutioApiClient,
    tracker: &StallTracker,
    now: SystemTime,
) -> putio_sync_client::error::ClientResult<usize> {
    let list = client.list_transfers().await?;
    let stalled = tracker.observe(&list.transfers, now);
    putio_sync_telemetry::transfers::set_stalled(stalled);
    for transfer in &list.transfers {
        if let Some(stall) = tracker.get(transfer.id) {
            debug!(
                transfer_id = transfer.id,
                name = %transfer.name,
                reason = %stall.reason.label(),
                "Transfer stalled"
            );
        }
    }
    Ok(stalled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn downloading(id: i64, downloaded: i64) -> serde_json::Value {
        serde_json::json!({
            "id": id, "name": "Ubuntu ISO", "status": "DOWNLOADING",
            "downloaded": downloaded, "availability": 92, "peers_sending_to_us": 0,
            "percent_done": 97, "created_at": "2026-10-26T10:00:00Z", "save_parent_id": 0,
            "estimated_time": null, "seconds_seeding": null, "error_message": null,
            "finished_at": null, "file_id": null
        })
    }

    async fn mock_list(
        server: &mut mockito::Server,
        transfers: serde_json::Value,
    ) -> mockito::Mock {
        server
            .mock("GET", "/transfers/list")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({ "status": "OK", "transfers": transfers }).to_string())
            .create_async()
            .await
    }

    /// FR-001..FR-004: a downloading transfer whose bytes do not move is flagged once the
    /// threshold passes, with the availability reason
    #[tokio::test]
    async fn check_stalls_flags_a_frozen_transfer() {
        let mut server = mockito::Server::new_async().await;
        let _list = mock_list(&mut server, serde_json::json!([downloading(5, 970)])).await;
        let client = PutioApiClient::with_base_url("t".to_string(), server.url());
        let tracker = StallTracker::new(Duration::from_secs(6 * 3600));
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);

        assert_eq!(check_stalls(&client, &tracker, t0).await.unwrap(), 0);
        let later = t0 + Duration::from_secs(6 * 3600 + 600);
        assert_eq!(check_stalls(&client, &tracker, later).await.unwrap(), 1);
        let stall = tracker.get(5).unwrap();
        assert_eq!(stall.since, t0);
        assert_eq!(stall.reason.label(), "availability 92%");
    }

    /// Put.io failing keeps the previous verdicts (spec edge case)
    #[tokio::test]
    async fn check_stalls_keeps_state_when_putio_fails() {
        let mut server = mockito::Server::new_async().await;
        let list = mock_list(&mut server, serde_json::json!([downloading(5, 970)])).await;
        let client = PutioApiClient::with_base_url("t".to_string(), server.url());
        let tracker = StallTracker::new(Duration::from_secs(3600));
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        check_stalls(&client, &tracker, t0).await.unwrap();
        check_stalls(&client, &tracker, t0 + Duration::from_secs(7200))
            .await
            .unwrap();
        assert_eq!(tracker.stalled_count(), 1);

        list.remove_async().await;
        let _broken = server
            .mock("GET", "/transfers/list")
            .with_status(400)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"ERROR","error_type":"BadRequest","error_message":"nope"}"#)
            .create_async()
            .await;
        assert!(
            check_stalls(&client, &tracker, t0 + Duration::from_secs(9000))
                .await
                .is_err()
        );
        assert_eq!(tracker.stalled_count(), 1);
    }

    #[test]
    fn sync_loop_uses_download_interval() {
        let config = DaemonConfig {
            download_interval: 30,
            watch_interval: 60,
            ..Default::default()
        };
        assert_eq!(loop_intervals(&config), (30, 60));
    }

    #[test]
    fn test_daemon_config_default() {
        let config = DaemonConfig::default();
        assert_eq!(config.watch_interval, 60);
        assert!(config.watch_dirs.is_empty());
    }

    /// `watch.archive_directory` reaches the watcher at startup, as a live settings save
    /// already does (no network: the client is only constructed)
    #[tokio::test]
    async fn watcher_starts_with_archive_directory() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("processed");
        let config = DaemonConfig {
            watch_dirs: vec![dir.path().to_path_buf()],
            archive_dir: Some(archive.clone()),
            parent_id: Some(9),
            ..Default::default()
        };
        let mut daemon = PutioDaemon::new(config);
        daemon.initialize().await.unwrap();

        let watcher = daemon.get_file_watcher().expect("a watcher");
        let started = watcher.get_config();
        assert_eq!(started.archive_dir, Some(archive));
        assert_eq!(started.parent_id, Some(9));
        assert_eq!(started.watch_dirs, vec![dir.path().to_path_buf()]);
    }

    /// Every download setting reaches the local download manager, not only the few the
    /// daemon used to copy (no network: the client is only constructed)
    #[tokio::test]
    async fn download_settings_reach_the_manager() {
        let dir = tempfile::tempdir().unwrap();
        let download = putio_sync_download::DownloadConfig {
            target_directory: dir.path().to_path_buf(),
            skip_existing: false,
            buffer_size_mb: 48,
            bandwidth_limit_bytes_per_sec: Some(1_048_576),
            max_retries: 9,
            retry_initial_delay_secs: 11,
            workers: 3,
            delete_after_download: true,
            blocked_extensions: vec!["iso".to_string()],
        };
        let config = DaemonConfig {
            database_path: Some(dir.path().join(".putiosync.db")),
            download: download.clone(),
            ..Default::default()
        };
        let mut daemon = PutioDaemon::new(config);
        daemon.initialize().await.unwrap();

        let manager = daemon.get_download_manager().expect("local mode");
        assert_eq!(manager.get_config().await, download);
    }

    /// A row a crash left in `downloading` is pending again once the daemon has
    /// initialised, before any worker starts
    #[tokio::test]
    async fn initialize_requeues_interrupted_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join(".putiosync.db");
        {
            let db = Database::new(&db_path).await.unwrap();
            db.queue().add(1, "a.mkv", 10, "", "", 4).await.unwrap();
            db.queue()
                .update_status(1, putio_sync_db::models::QueueStatus::Downloading)
                .await
                .unwrap();
        }
        let config = DaemonConfig {
            database_path: Some(db_path),
            download: putio_sync_download::DownloadConfig {
                target_directory: dir.path().to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut daemon = PutioDaemon::new(config);
        daemon.initialize().await.unwrap();

        let rows = daemon
            .get_database()
            .unwrap()
            .queue()
            .get_all()
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "pending");
        assert_eq!(rows[0].priority, 4);
        assert_eq!(
            daemon
                .get_download_manager()
                .unwrap()
                .active_workers()
                .await,
            0
        );
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
