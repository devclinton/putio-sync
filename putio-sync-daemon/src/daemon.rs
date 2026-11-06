//! Daemon orchestrator for Put.io sync operations
//!
//! The PutioDaemon coordinates all sync operations including:
//! - File watching for torrent/magnet files
//! - Download management and synchronization
//! - API server (when enabled)

use crate::error::DaemonResult;
use putio_sync_client::PutioApiClient;
use putio_sync_core::cleanup::EmptyFolderCleanup;
use putio_sync_core::config::PrecheckConfig;
use putio_sync_core::stall::StallTracker;
use putio_sync_db::Database;
use putio_sync_doctor::Doctor;
use putio_sync_download::pipeline::Counts;
use putio_sync_download::{DownloadConfig, DownloadManager, DownloadResult};
use putio_sync_file_manager::watcher::WatcherConfig;
use putio_sync_file_manager::TorrentWatcher;
use putio_sync_precheck::Prechecker;
use putio_sync_storage::StorageService;
use putio_sync_telemetry::{PipelineStage, PipelineState};
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
    /// Hours a finished transfer stays in the Active page's Recently finished list
    pub finished_retention_hours: u64,
    /// The release check for watch files (042), from `Config::resolve_precheck`. `None`
    /// uploads them unchecked.
    pub precheck: Option<PrecheckConfig>,
}

/// Days the release checks and reports are kept (042 research R10)
const RELEASE_KEEP_DAYS: i64 = 400;

/// How often the pipeline loop sets `putio_sync_pipeline_items` (feature 039)
pub const PIPELINE_TICK: Duration = Duration::from_secs(10);
/// How old the Put.io list may be for the pipeline loop (R3): with no page open,
/// Put.io is asked at most once per this; open pages ask every 10 s and the loop reuses
/// their list
pub const PIPELINE_MAX_AGE: Duration = Duration::from_secs(60);

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
            finished_retention_hours: 24,
            precheck: None,
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
    /// Put.io storage gauges and the opt-in daily free-up (shared with the API)
    storage: Option<Arc<StorageService>>,
    /// Stalled-transfer verdicts, fed by the stall check task and read by the API
    stall_tracker: Arc<StallTracker>,
    /// Scheduled health checks (shared with the API)
    doctor: Option<Arc<Doctor>>,
    /// The processing pipeline (feature 038), fed by the pipeline loop (shared with the API)
    pipeline: Option<Arc<putio_sync_pipeline::Pipeline>>,
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
            storage: None,
            doctor: None,
            pipeline: None,
        }
    }

    /// Run `cleanup` once a day while its `[cleanup] enabled` is on. The same service
    /// should be given to the API so manual and daily runs never overlap.
    pub fn with_cleanup(mut self, cleanup: Arc<EmptyFolderCleanup>) -> Self {
        self.cleanup = Some(cleanup);
        self
    }

    /// Refresh the Put.io storage gauges and run `storage`'s automatic free-up while
    /// `[storage] auto_free` is on. The same service should be given to the API.
    pub fn with_storage(mut self, storage: Arc<StorageService>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Run `doctor`'s checks every `[doctor] interval_minutes` while enabled. The same
    /// doctor should be given to the API so its page shows the scheduled runs.
    pub fn with_doctor(mut self, doctor: Arc<Doctor>) -> Self {
        self.doctor = Some(doctor);
        self
    }

    /// Advance `pipeline` (feature 038) on every pipeline tick. The same service should be
    /// given to the API. Call it before [`Self::start`].
    pub fn with_pipeline(mut self, pipeline: Arc<putio_sync_pipeline::Pipeline>) -> Self {
        self.pipeline = Some(pipeline);
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
            // A pause or slow-down set before a restart keeps its end time
            if let Err(e) = download_manager.restore_bandwidth_override().await {
                warn!("Failed to restore the bandwidth override: {}", e);
            }

            // Wrap in Arc for sharing with API (hot reload support via internal config RwLock)
            self.download_mode = Some(DownloadMode::Local(Arc::new(download_manager)));
            info!("Download mode: Local");
        }

        // Initialize file watcher (shared Arc for hot reload via API)
        if !config.watch_dirs.is_empty() {
            let file_watcher = TorrentWatcher::from_config(WatcherConfig {
                archive_dir: config.archive_dir.clone(),
                delete_after_upload: config.delete_after_upload,
                ..WatcherConfig::new(
                    config.watch_dirs.clone(),
                    config.parent_id,
                    config.watch_interval,
                )
            });
            let file_watcher = Arc::new(match &config.precheck {
                Some(precheck) => file_watcher.with_precheck(Arc::new(Prechecker::new(
                    precheck.clone(),
                    config.download.rules.clone(),
                ))),
                None => file_watcher,
            });
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
        let (sync_task, download_task, pipeline_task) = match &self.download_mode {
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

                    // Spawn the pipeline task (feature 039): the Active pipeline gauge
                    let dm_pipeline = dm.clone();
                    let tracker = self.stall_tracker.clone();
                    let retention = self.config.read().await.finished_retention_hours;
                    let processing = self.pipeline.clone();
                    let pipeline_handle = tokio::spawn(async move {
                        Self::run_pipeline_loop(dm_pipeline, tracker, retention, processing).await;
                    });

                    (
                        Some(sync_handle),
                        Some(worker_handle),
                        Some(pipeline_handle),
                    )
                } else {
                    unreachable!()
                }
            }
            Some(DownloadMode::Remote { api_url, .. }) => {
                info!(
                    "Running in API mode - downloads handled by API server at {}",
                    api_url
                );
                (None, None, None)
            }
            None => {
                info!("Download mode not configured, skipping download tasks");
                (None, None, None)
            }
        };

        let cleanup_task = self
            .cleanup
            .clone()
            .map(|cleanup| tokio::spawn(crate::schedule::run_daily(cleanup)));
        let storage_task = self
            .storage
            .clone()
            .map(|storage| tokio::spawn(crate::schedule::run_storage(storage)));
        let doctor_task = self
            .doctor
            .clone()
            .map(|doctor| tokio::spawn(putio_sync_doctor::run_scheduled(doctor)));
        let prune_task = self
            .database
            .clone()
            .map(|db| tokio::spawn(Self::run_release_prune(db)));
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
        match (&storage_task, &self.storage) {
            (Some(_), Some(storage)) => {
                let config = storage.config();
                info!(
                    "  - Put.io storage task: running (automatic free-up {})",
                    if config.auto_free {
                        format!(
                            "daily at {} below {}% free",
                            config.run_at, config.warn_free_percent
                        )
                    } else {
                        "off".to_string()
                    }
                );
            }
            _ => info!("  - Put.io storage task: disabled"),
        }
        match (&doctor_task, &self.doctor) {
            (Some(_), Some(doctor)) => {
                let settings = doctor.settings();
                info!(
                    "  - Doctor task: running (every {} min after a {}s delay, {})",
                    settings.interval().as_secs() / 60,
                    putio_sync_doctor::STARTUP_DELAY.as_secs(),
                    if settings.enabled {
                        "enabled"
                    } else {
                        "currently disabled"
                    }
                );
            }
            _ => info!("  - Doctor task: disabled"),
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
        if let Some(task) = pipeline_task {
            task.abort();
        }
        if let Some(task) = cleanup_task {
            task.abort();
        }
        if let Some(task) = storage_task {
            task.abort();
        }
        if let Some(task) = stall_task {
            task.abort();
        }
        if let Some(task) = doctor_task {
            task.abort();
        }
        if let Some(task) = prune_task {
            task.abort();
        }

        info!("Daemon shutting down...");
        Ok(())
    }

    /// Set the Active pipeline gauge every [`PIPELINE_TICK`]
    async fn run_pipeline_loop(
        download_manager: Arc<DownloadManager>,
        tracker: Arc<StallTracker>,
        retention_hours: u64,
        processing: Option<Arc<putio_sync_pipeline::Pipeline>>,
    ) {
        info!(
            tick_secs = PIPELINE_TICK.as_secs(),
            "Active pipeline task started"
        );
        let mut tick = tokio::time::interval(PIPELINE_TICK);
        loop {
            tick.tick().await;
            if let Err(e) = pipeline_tick_with(
                &download_manager,
                &tracker,
                retention_hours,
                processing.as_deref(),
            )
            .await
            {
                debug!(error = %e, "Active pipeline update failed; keeping the gauge");
            }
        }
    }

    /// Delete release checks and reports older than `RELEASE_KEEP_DAYS` (feature 042), at
    /// startup and once a day
    async fn run_release_prune(database: Arc<Database>) {
        let mut tick = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
        loop {
            tick.tick().await;
            match database.releases().prune(RELEASE_KEEP_DAYS).await {
                Ok(0) => {}
                Ok(rows) => info!(rows, "Pruned old release checks and reports"),
                Err(e) => warn!(error = %e, "Failed to prune release checks and reports"),
            }
        }
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

/// One pipeline update: build the Active pipeline from a Put.io list at most
/// [`PIPELINE_MAX_AGE`] old, set `putio_sync_pipeline_items`, and record Recently finished
pub async fn pipeline_tick(
    download_manager: &DownloadManager,
    tracker: &StallTracker,
    retention_hours: u64,
) -> DownloadResult<Counts> {
    pipeline_tick_with(download_manager, tracker, retention_hours, None).await
}

/// [`pipeline_tick`], then hand the same list to the processing pipeline (feature 038),
/// which follows items through their steps
pub async fn pipeline_tick_with(
    download_manager: &DownloadManager,
    tracker: &StallTracker,
    retention_hours: u64,
    processing: Option<&putio_sync_pipeline::Pipeline>,
) -> DownloadResult<Counts> {
    let pipeline = download_manager
        .pipeline(PIPELINE_MAX_AGE, tracker, retention_hours)
        .await?;
    for (stage, state, n) in pipeline_gauges(&pipeline.counts) {
        putio_sync_telemetry::pipeline::set_items(stage, state, n);
    }
    putio_sync_telemetry::pipeline::set_nas_paused(download_manager.nas_paused().await);
    // Same list as above: it is inside the cache age, so this asks Put.io for nothing
    download_manager
        .release_stale_holds(PIPELINE_MAX_AGE)
        .await?;
    // Items that left the list since the last tick go to Recently finished (R6, R12)
    download_manager
        .record_finished(&pipeline, retention_hours)
        .await?;
    if let Some(processing) = processing {
        processing.on_tick(&pipeline).await;
        // Put.io deletes held back for the Subtitles stage go once it has ended (feature 025)
        let ready = processing.releasable_deletes().await;
        if !ready.is_empty() {
            let done = download_manager.delete_remote(&ready).await;
            processing.mark_deleted(&done.deleted).await;
            processing.mark_deleted(&done.refused).await;
        }
    }
    Ok(pipeline.counts)
}

/// Every `(stage, state)` gauge value of `counts`, zeros included
fn pipeline_gauges(counts: &Counts) -> Vec<(PipelineStage, PipelineState, usize)> {
    let (p, n) = (&counts.putio, &counts.nas);
    vec![
        (PipelineStage::Putio, PipelineState::Queued, p.queued),
        (
            PipelineStage::Putio,
            PipelineState::Downloading,
            p.downloading,
        ),
        (PipelineStage::Putio, PipelineState::Stalled, p.stalled),
        (PipelineStage::Putio, PipelineState::Finishing, p.finishing),
        (PipelineStage::Putio, PipelineState::Error, p.error),
        (
            PipelineStage::Nas,
            PipelineState::WaitingForSync,
            n.waiting_for_sync,
        ),
        (PipelineStage::Nas, PipelineState::Queued, n.queued),
        (
            PipelineStage::Nas,
            PipelineState::Downloading,
            n.downloading,
        ),
        (PipelineStage::Nas, PipelineState::Paused, n.paused),
        (PipelineStage::Nas, PipelineState::Failed, n.failed),
    ]
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

    async fn pipeline_manager(
        server: &mockito::Server,
    ) -> (tempfile::TempDir, DownloadManager, Arc<Database>) {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            Database::new(dir.path().join(".putiosync.db"))
                .await
                .unwrap(),
        );
        let client = Arc::new(PutioApiClient::with_base_url("t".to_string(), server.url()));
        let config = DownloadConfig {
            target_directory: dir.path().to_path_buf(),
            ..DownloadConfig::default()
        };
        (dir, DownloadManager::new(client, db.clone(), config), db)
    }

    /// FR-028, SC-006: with no page open, a minute of loop ticks asks Put.io once
    #[tokio::test]
    async fn pipeline_ticks_ask_putio_once_per_minute_with_no_viewer() {
        let mut server = mockito::Server::new_async().await;
        let list = server
            .mock("GET", "/transfers/list")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({ "status": "OK", "transfers": [downloading(5, 970)] })
                    .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let (_dir, manager, _db) = pipeline_manager(&server).await;
        let tracker = StallTracker::new(Duration::from_secs(3600));

        let ticks = PIPELINE_MAX_AGE.as_secs() / PIPELINE_TICK.as_secs();
        assert_eq!(ticks, 6);
        for _ in 0..ticks {
            let counts = pipeline_tick(&manager, &tracker, 24).await.unwrap();
            assert_eq!(counts.putio.downloading, 1);
        }
        list.assert_async().await;
    }

    /// FR-028, SC-006: three tabs polling every 5 s beside the loop share one Put.io
    /// call per page window
    #[tokio::test]
    async fn open_pages_and_the_loop_share_one_putio_call() {
        let mut server = mockito::Server::new_async().await;
        let list = server
            .mock("GET", "/transfers/list")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({ "status": "OK", "transfers": [downloading(5, 970)] })
                    .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let (_dir, manager, _db) = pipeline_manager(&server).await;
        let tracker = StallTracker::new(Duration::from_secs(3600));

        // One 10 s window: a loop tick, and each of three tabs polls twice
        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        for _poll in 0..2 {
            for _tab in 0..3 {
                manager
                    .pipeline(Duration::from_secs(10), &tracker, 24)
                    .await
                    .unwrap();
            }
        }
        list.assert_async().await;
    }

    /// R9: a hold is cleared when its transfer is in error or gone from Put.io; a hold on
    /// a live transfer stays
    #[tokio::test]
    async fn pipeline_tick_releases_holds_of_failed_and_gone_transfers() {
        let mut server = mockito::Server::new_async().await;
        let mut failed = downloading(6, 0);
        failed["status"] = "ERROR".into();
        failed["error_message"] = "tracker gone".into();
        mock_list(
            &mut server,
            serde_json::json!([downloading(5, 970), failed]),
        )
        .await;
        let (_dir, manager, db) = pipeline_manager(&server).await;
        for id in [5, 6, 7] {
            db.holds().add(id, "Held").await.unwrap();
        }
        let tracker = StallTracker::new(Duration::from_secs(3600));

        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        let left: Vec<i64> = db
            .holds()
            .list()
            .await
            .unwrap()
            .into_iter()
            .map(|h| h.transfer_id)
            .collect();
        assert_eq!(left, vec![5]);
    }

    /// With Put.io down the list is the last known one, so no hold is judged gone
    #[tokio::test]
    async fn pipeline_tick_keeps_holds_while_putio_is_unreachable() {
        let mut server = mockito::Server::new_async().await;
        let list = mock_list(&mut server, serde_json::json!([downloading(5, 970)])).await;
        let (_dir, manager, db) = pipeline_manager(&server).await;
        db.holds().add(5, "Held").await.unwrap();
        db.holds().add(7, "Held").await.unwrap();
        let tracker = StallTracker::new(Duration::from_secs(3600));
        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        // Put.io fails from now on; the next snapshot is stale
        list.remove_async().await;
        let _down = server
            .mock("GET", "/transfers/list")
            .with_status(400)
            .with_body(r#"{"status":"ERROR","error_type":"Down","error_message":"down"}"#)
            .create_async()
            .await;
        manager.transfers_snapshot(Duration::ZERO).await.unwrap();
        db.holds().add(8, "Held").await.unwrap();

        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        assert_eq!(db.holds().list().await.unwrap().len(), 2);
    }

    // --- Recently finished (feature 039, US6, R6, R12) ---

    fn transfer(id: i64, status: &str, finished: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "id": id, "name": format!("Show {id}"), "status": status, "size": 300,
            "downloaded": 300, "percent_done": 100, "created_at": "2026-10-26T10:00:00",
            "finished_at": finished, "save_parent_id": 0, "file_id": 50 + id,
            "error_message": if status == "ERROR" { Some("tracker gone") } else { None }
        })
    }

    /// Replace the mocked Put.io list
    async fn set_list(
        server: &mut mockito::Server,
        old: Option<mockito::Mock>,
        transfers: serde_json::Value,
    ) -> mockito::Mock {
        if let Some(old) = old {
            old.remove_async().await;
        }
        mock_list(server, transfers).await
    }

    fn recent(hours: i64) -> String {
        (chrono::Utc::now() - chrono::Duration::hours(hours))
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string()
    }

    /// Queue a file of `transfer_id`, as the sync does
    async fn queue_file(db: &Database, file_id: i64, transfer_id: i64) {
        db.queue()
            .add(
                file_id,
                &format!("e{file_id}.mkv"),
                100,
                "http://x",
                &format!("Show {transfer_id}"),
                0,
                putio_sync_db::models::QueueStatus::Pending,
            )
            .await
            .unwrap();
        db.sources()
            .record(
                file_id,
                &putio_sync_db::models::DownloadSource {
                    transfer_id: Some(transfer_id),
                    source_type: None,
                    tracker_host: None,
                    is_private: false,
                    origin: putio_sync_db::models::Origin::Putio,
                    watch_folder: None,
                    putio_folder: None,
                },
            )
            .await
            .unwrap();
    }

    /// The file finishes: its queue row goes and a completed download is recorded
    async fn finish_file(db: &Database, file_id: i64) {
        db.queue().remove(file_id).await.unwrap();
        db.downloads()
            .insert(
                file_id,
                &format!("e{file_id}.mkv"),
                100,
                "/x",
                putio_sync_db::models::DownloadStatus::Completed,
            )
            .await
            .unwrap();
    }

    async fn rows(db: &Database) -> Vec<putio_sync_db::pipeline::FinishedRow> {
        db.finished().list().await.unwrap()
    }

    #[tokio::test]
    async fn the_first_transition_to_done_writes_one_done_row() {
        let mut server = mockito::Server::new_async().await;
        let finished = recent(2);
        let list = set_list(
            &mut server,
            None,
            serde_json::json!([transfer(1, "COMPLETED", Some(&finished))]),
        )
        .await;
        let (_dir, manager, db) = pipeline_manager(&server).await;
        let tracker = StallTracker::new(Duration::from_secs(3600));
        queue_file(&db, 101, 1).await;
        queue_file(&db, 102, 1).await;

        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        assert!(rows(&db).await.is_empty(), "still downloading");
        finish_file(&db, 101).await;
        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        assert!(rows(&db).await.is_empty(), "one file is left");
        finish_file(&db, 102).await;
        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        drop(list);

        let rows = rows(&db).await;
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.key, "transfer:1");
        assert_eq!(row.name, "Show 1");
        assert_eq!(row.outcome, putio_sync_db::pipeline::FinishedOutcome::Done);
        assert_eq!(row.reason, None);
        assert_eq!(row.size, 200);
        assert_eq!(row.added_at.as_deref(), Some("2026-10-26 10:00:00"));
        assert_eq!(row.putio_done_at, Some(finished.replace('T', " ")));
        assert!(row.queued_at.is_some());
        assert!(row.done_at.len() == 19 && !row.done_at.contains('T'));
    }

    #[tokio::test]
    async fn a_retryable_failure_writes_nothing_and_a_gone_error_writes_failed() {
        let mut server = mockito::Server::new_async().await;
        let list = set_list(
            &mut server,
            None,
            serde_json::json!([
                transfer(1, "ERROR", None),
                transfer(2, "COMPLETED", Some(&recent(1)))
            ]),
        )
        .await;
        let (_dir, manager, db) = pipeline_manager(&server).await;
        let tracker = StallTracker::new(Duration::from_secs(3600));
        // Transfer 2 has a failed file: it can be retried from the Queue page
        queue_file(&db, 201, 2).await;
        db.queue()
            .update_status(201, putio_sync_db::models::QueueStatus::Failed)
            .await
            .unwrap();

        for _ in 0..3 {
            pipeline_tick(&manager, &tracker, 24).await.unwrap();
        }
        assert!(rows(&db).await.is_empty(), "both can still be retried");

        // The error transfer is cleared from Put.io
        let list = set_list(
            &mut server,
            Some(list),
            serde_json::json!([transfer(2, "COMPLETED", Some(&recent(1)))]),
        )
        .await;
        manager.transfers_snapshot(Duration::ZERO).await.unwrap();
        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        drop(list);

        let rows = rows(&db).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, "transfer:1");
        assert_eq!(
            rows[0].outcome,
            putio_sync_db::pipeline::FinishedOutcome::Failed
        );
        assert_eq!(rows[0].reason.as_deref(), Some("tracker gone"));
        assert!(rows[0].putio_done_at.is_none(), "the transfer is gone");
    }

    /// FR-002, R5: failed files the operator skipped end the item
    #[tokio::test]
    async fn skipped_failed_files_end_an_item_as_done_or_failed() {
        let mut server = mockito::Server::new_async().await;
        let _list = set_list(
            &mut server,
            None,
            serde_json::json!([
                transfer(1, "COMPLETED", Some(&recent(1))),
                transfer(2, "COMPLETED", Some(&recent(1)))
            ]),
        )
        .await;
        let (_dir, manager, db) = pipeline_manager(&server).await;
        let tracker = StallTracker::new(Duration::from_secs(3600));
        // Transfer 1: one file done and one failed; transfer 2: its only file failed
        queue_file(&db, 101, 1).await;
        queue_file(&db, 102, 1).await;
        queue_file(&db, 201, 2).await;
        finish_file(&db, 101).await;
        for id in [102, 201] {
            db.queue()
                .update_status(id, putio_sync_db::models::QueueStatus::Failed)
                .await
                .unwrap();
        }
        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        assert!(rows(&db).await.is_empty());

        // The operator skips the failed files: the queue rows go, no download exists
        db.queue().remove(102).await.unwrap();
        db.queue().remove(201).await.unwrap();
        pipeline_tick(&manager, &tracker, 24).await.unwrap();

        let rows = rows(&db).await;
        let by_key = |key: &str| rows.iter().find(|r| r.key == key).unwrap();
        let one = by_key("transfer:1");
        assert_eq!(one.outcome, putio_sync_db::pipeline::FinishedOutcome::Done);
        assert_eq!(one.reason.as_deref(), Some("1 file skipped"));
        let two = by_key("transfer:2");
        assert_eq!(
            two.outcome,
            putio_sync_db::pipeline::FinishedOutcome::Failed
        );
        assert_eq!(two.reason.as_deref(), Some("nothing downloaded"));
    }

    #[tokio::test]
    async fn a_later_write_replaces_the_row_and_old_rows_are_pruned() {
        let mut server = mockito::Server::new_async().await;
        let _list = set_list(&mut server, None, serde_json::json!([])).await;
        let (_dir, manager, db) = pipeline_manager(&server).await;
        let tracker = StallTracker::new(Duration::from_secs(3600));
        let row = |key: &str, name: &str, done_at: &str| putio_sync_db::pipeline::FinishedRow {
            key: key.to_string(),
            name: name.to_string(),
            size: 1,
            outcome: putio_sync_db::pipeline::FinishedOutcome::Done,
            reason: None,
            added_at: None,
            putio_done_at: None,
            queued_at: None,
            done_at: done_at.to_string(),
        };
        let fresh = recent(1).replace('T', " ");
        let old = recent(30).replace('T', " ");
        db.finished()
            .upsert(&row("transfer:1", "First", &fresh))
            .await
            .unwrap();
        db.finished()
            .upsert(&row("transfer:1", "Second", &fresh))
            .await
            .unwrap();
        db.finished()
            .upsert(&row("transfer:2", "Old", &old))
            .await
            .unwrap();

        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        let rows = rows(&db).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Second");
        // The retention is the setting, not a constant
        pipeline_tick(&manager, &tracker, 0).await.unwrap();
        assert!(self::rows(&db).await.is_empty());
    }

    /// R6: the first tick after a restart writes only what finished within the retention
    /// period, and a pruned row does not come back
    #[tokio::test]
    async fn the_first_tick_after_a_restart_writes_only_recent_completions() {
        let mut server = mockito::Server::new_async().await;
        let _list = set_list(
            &mut server,
            None,
            serde_json::json!([
                transfer(1, "COMPLETED", Some(&recent(80))),
                transfer(2, "COMPLETED", Some(&recent(2))),
                transfer(3, "COMPLETED", Some(&recent(1)))
            ]),
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".putiosync.db");
        let open = || async {
            let db = Arc::new(Database::new(path.clone()).await.unwrap());
            let client = Arc::new(PutioApiClient::with_base_url("t".to_string(), server.url()));
            let config = DownloadConfig {
                target_directory: dir.path().to_path_buf(),
                ..DownloadConfig::default()
            };
            (DownloadManager::new(client, db.clone(), config), db)
        };
        let tracker = StallTracker::new(Duration::from_secs(3600));
        let (manager, db) = open().await;
        for (file, transfer) in [(101, 1), (201, 2), (301, 3)] {
            queue_file(&db, file, transfer).await;
            finish_file(&db, file).await;
        }
        // Transfer 1 finished three days ago; transfer 3 already has a row
        sqlx::query(
            "UPDATE downloads SET downloaded_at = datetime('now', '-72 hours') WHERE file_id = 101",
        )
        .execute(db.pool())
        .await
        .unwrap();
        sqlx::query(
            "UPDATE downloads SET downloaded_at = datetime('now', '-1 hours') WHERE file_id = 201",
        )
        .execute(db.pool())
        .await
        .unwrap();
        let existing = putio_sync_db::pipeline::FinishedRow {
            key: "transfer:3".to_string(),
            name: "Already".to_string(),
            size: 1,
            outcome: putio_sync_db::pipeline::FinishedOutcome::Done,
            reason: None,
            added_at: None,
            putio_done_at: None,
            queued_at: None,
            done_at: recent(1).replace('T', " "),
        };
        db.finished().upsert(&existing).await.unwrap();

        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        let keys = |rows: &[putio_sync_db::pipeline::FinishedRow]| {
            let mut keys: Vec<String> = rows.iter().map(|r| r.key.clone()).collect();
            keys.sort();
            keys
        };
        assert_eq!(keys(&rows(&db).await), vec!["transfer:2", "transfer:3"]);
        assert_eq!(
            rows(&db)
                .await
                .iter()
                .find(|r| r.key == "transfer:3")
                .unwrap()
                .name,
            "Already"
        );

        // FR-026: the rows survive a restart, which writes nothing more
        drop((manager, db));
        let (manager, db) = open().await;
        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        assert_eq!(keys(&rows(&db).await), vec!["transfer:2", "transfer:3"]);

        // Pruned (retention 0 here), a row is not written again by later ticks
        sqlx::query("UPDATE pipeline_finished SET done_at = datetime('now', '-30 hours')")
            .execute(db.pool())
            .await
            .unwrap();
        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        assert!(rows(&db).await.is_empty());
    }

    /// R6: a finished transfer whose files a rule blocked is recorded once
    #[tokio::test]
    async fn a_transfer_with_every_file_blocked_is_recorded_once() {
        let mut server = mockito::Server::new_async().await;
        let finished = recent(2);
        let _list = set_list(
            &mut server,
            None,
            serde_json::json!([
                transfer(1, "COMPLETED", Some(&finished)),
                transfer(2, "COMPLETED", Some(&recent(90)))
            ]),
        )
        .await;
        let (_dir, manager, db) = pipeline_manager(&server).await;
        let tracker = StallTracker::new(Duration::from_secs(3600));
        for (file_id, name, folder) in [
            (11, "a.exe", "Show 1"),
            (12, "b.EXE", "Show 1/extras"),
            (13, "c.scr", "Show 1"),
            (21, "d.exe", "Show 2"),
        ] {
            db.rule_decisions()
                .record(
                    file_id,
                    name,
                    folder,
                    putio_sync_telemetry::RuleDecision::Blocked,
                    "blocked extension",
                    None,
                )
                .await
                .unwrap();
        }
        // A sync ran after both transfers finished, so neither is waiting for the next
        // one (it finds nothing). Past the first tick, so this is not the restart case.
        let _files = server
            .mock("GET", "/files/list?parent_id=0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"files":[],"parent":{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false},"status":"OK"}"#,
            )
            .create_async()
            .await;
        manager.sync_downloads().await.unwrap();
        pipeline_tick(&manager, &tracker, 24).await.unwrap();
        pipeline_tick(&manager, &tracker, 24).await.unwrap();

        let rows = rows(&db).await;
        assert_eq!(
            rows.len(),
            1,
            "transfer 2 finished outside the retention period"
        );
        let row = &rows[0];
        assert_eq!(row.key, "transfer:1");
        assert_eq!(
            row.outcome,
            putio_sync_db::pipeline::FinishedOutcome::Failed
        );
        assert_eq!(
            row.reason.as_deref(),
            Some("nothing to download (blocked: .exe, .scr)")
        );
        assert_eq!(row.done_at, finished.replace('T', " "));
    }

    #[test]
    fn pipeline_gauges_cover_every_stage_and_state() {
        let mut counts = Counts::default();
        counts.putio.stalled = 2;
        counts.nas.waiting_for_sync = 3;
        let gauges = pipeline_gauges(&counts);
        let expected: usize = putio_sync_telemetry::labels::PIPELINE_STATES
            .iter()
            .map(|(_, states)| states.len())
            .sum();
        assert_eq!(gauges.len(), expected);
        assert!(gauges.contains(&(PipelineStage::Putio, PipelineState::Stalled, 2)));
        assert!(gauges.contains(&(PipelineStage::Nas, PipelineState::WaitingForSync, 3)));
        assert!(gauges.contains(&(PipelineStage::Nas, PipelineState::Queued, 0)));
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
            on_existing: putio_sync_core::config::ExistingFile::Overwrite,
            resume_enabled: false,
            buffer_size_mb: 48,
            bandwidth_limit_bytes_per_sec: Some(1_048_576),
            bandwidth_schedule: Vec::new(),
            slow_limit_bytes_per_sec: 300_000,
            max_retries: 9,
            retry_initial_delay_secs: 11,
            workers: 3,
            delete_after_download: true,
            rules: putio_sync_core::rules::RuleSet::from_config(
                &putio_sync_core::config::DownloadConfig {
                    blocked_extensions: vec!["iso".to_string()],
                    ..Default::default()
                },
            ),
            verify: putio_sync_download::manager::VerifySettings::default(),
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
            db.queue()
                .add(
                    1,
                    "a.mkv",
                    10,
                    "",
                    "",
                    4,
                    putio_sync_db::models::QueueStatus::Pending,
                )
                .await
                .unwrap();
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
