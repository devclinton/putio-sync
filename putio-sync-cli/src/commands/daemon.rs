//! Daemon command implementation with hot reloading support
//!
//! Runs both download sync and torrent watching in a single process
//! with intelligent configuration hot reloading and worker scaling

use anyhow::{anyhow, Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use putio_sync_core::api::ApiClient;
use putio_sync_core::config::Config;
use putio_sync_core::download::{DownloadJob, DownloadProgress, WorkerPool};
use std::collections::HashMap;
use std::future;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

use crate::diagnostics::{DaemonEvent, DiagnosticsCollector, DownloadStatus, WatchStatus};
use crate::download_ops::DownloadOps;
use crate::watch::{TorrentWatcher, TorrentWatchCore};

/// Active download tracking for hot reload
#[derive(Debug, Clone)]
pub struct ActiveDownload {
    pub job: DownloadJob,
    pub start_time: Instant,
    pub progress: DownloadProgress,
    pub worker_id: usize,
}

impl ActiveDownload {
    pub fn completion_percentage(&self) -> f64 {
        self.progress.percentage()
    }

    pub fn should_preserve(&self) -> bool {
        self.completion_percentage() >= 75.0
    }
}

/// Hot-reload state for the daemon
pub struct HotReloadState {
    pub config: Arc<RwLock<Config>>,
    pub active_downloads: Arc<RwLock<HashMap<String, ActiveDownload>>>,
    pub download_queue: Arc<RwLock<Vec<DownloadJob>>>,
    pub worker_pool: Arc<RwLock<Option<WorkerPool>>>,
    pub watch_core: Option<TorrentWatchCore>,
    pub download_ops: Arc<RwLock<Option<DownloadOps>>>,
    pub config_change_tx: mpsc::UnboundedSender<Config>,
    pub diagnostics: Arc<RwLock<DiagnosticsCollector>>,
}

/// Execute the daemon command with hot reloading support
///
/// This runs both download sync and torrent watching in the same process,
/// with intelligent configuration reloading and worker scaling.
#[allow(clippy::too_many_arguments)]
pub async fn execute_daemon(
    download_interval: u64,
    watch_interval: u64,
    watch_paths: Vec<PathBuf>,
    filter: Option<String>,
    skip_existing: bool,
    delete_after: bool,
    parent_id: Option<i64>,
    watch_only: bool,
    download_only: bool,
    config_path: Option<PathBuf>,
) -> Result<()> {
    eprintln!("🎯 execute_daemon function started");
    eprintln!("📋 Daemon parameters:");
    eprintln!("   - download_interval: {}s", download_interval);
    eprintln!("   - watch_interval: {}s", watch_interval);
    eprintln!("   - watch_paths: {:?}", watch_paths);
    eprintln!("   - config_path: {:?}", config_path);
    
    // Load configuration
    eprintln!("📂 Determining config file path...");
    let config_file_path = config_path
        .or_else(|| Config::default_path().ok())
        .ok_or_else(|| anyhow!("Could not determine config file path"))?;
    
    eprintln!("📄 Config file path: {}", config_file_path.display());
    eprintln!("🔄 Loading configuration...");
    let config = Config::load(&config_file_path)
        .context("Failed to load configuration. Have you run 'putio-sync login'?")?;
    
    eprintln!("✅ Configuration loaded successfully");

    let auth = config.auth.clone().ok_or_else(|| {
        anyhow!("No authentication credentials found. Please run 'putio-sync login' first.")
    })?;

    // Create API client
    let client = Arc::new(ApiClient::from_auth_config(auth));

    // Determine watch directories - use CLI args if provided, otherwise config
    let watch_dirs = if watch_paths.is_empty() {
        config.watch.paths.clone()
    } else {
        watch_paths
    };

    // Validate modes
    if watch_only && download_only {
        return Err(anyhow!("Cannot specify both --watch-only and --download-only"));
    }

    if watch_only && watch_dirs.is_empty() {
        return Err(anyhow!("Watch mode requires watch directories. Use --watch or configure watch.paths in config.toml"));
    }

    info!("=== Starting putio-sync hot-reload daemon ===");
    info!("Hot reloading: ENABLED - config changes will be applied automatically");
    
    if !download_only {
        info!("Download sync: {} (interval: {}s)", if watch_only { "DISABLED" } else { "ENABLED" }, download_interval);
    }
    
    if !watch_only {
        info!("Torrent watch: {} (interval: {}s)", if download_only { "DISABLED" } else { "ENABLED" }, watch_interval);
        if !watch_dirs.is_empty() {
            info!("Watch directories: {:?}", watch_dirs);
        }
    }

    // Set up hot reload state
    let (config_change_tx, mut config_change_rx) = mpsc::unbounded_channel();
    
    // Create watch core if not download-only
    let watch_core = if !download_only && !watch_dirs.is_empty() {
        Some(TorrentWatchCore::new())
    } else {
        None
    };

    // Initialize download operations if not watch-only
    let download_ops = if !watch_only {
        let sync_parent_id = config.sync.remote_parent_id.unwrap_or(0);
        
        match DownloadOps::new(client.clone(), config.download.clone(), sync_parent_id).await {
            Ok(ops) => Some(ops),
            Err(e) => {
                error!("Failed to initialize download operations: {}", e);
                return Err(e);
            }
        }
    } else {
        None
    };

    let hot_reload_state = HotReloadState {
        config: Arc::new(RwLock::new(config.clone())),
        active_downloads: Arc::new(RwLock::new(HashMap::new())),
        download_queue: Arc::new(RwLock::new(Vec::new())),
        worker_pool: Arc::new(RwLock::new(None)),
        watch_core,
        download_ops: Arc::new(RwLock::new(download_ops)),
        config_change_tx,
        diagnostics: Arc::new(RwLock::new(DiagnosticsCollector::new())),
    };

    // Initialize worker pool
    {
        let mut worker_pool = hot_reload_state.worker_pool.write().unwrap();
        *worker_pool = Some(WorkerPool::new(config.download.workers));
    }

    // Start config file watching
    let _watcher = start_config_watching(&config_file_path, hot_reload_state.config_change_tx.clone()).await?;

    // Create torrent watcher if needed (for legacy mode)
    let watcher = if !download_only && !watch_dirs.is_empty() {
        Some(TorrentWatcher::new(
            watch_dirs.clone(),
            parent_id,
            watch_interval,
        ))
    } else {
        None
    };

    // Create timers with original intervals (these can be updated via config)
    let mut download_timer = if !watch_only {
        Some(interval(Duration::from_secs(download_interval)))
    } else {
        None
    };

    let mut watch_timer = if !download_only && watcher.is_some() {
        Some(interval(Duration::from_secs(watch_interval)))
    } else {
        None
    };

    // Add periodic health reporting every 5 minutes
    let mut health_timer = interval(Duration::from_secs(300));

    info!("Hot-reload daemon started successfully. Press Ctrl+C to stop.");

    // Main daemon loop with hot reload support
    loop {
        tokio::select! {
            // Handle download sync
            _ = async {
                if let Some(ref mut timer) = download_timer {
                    timer.tick().await;
                } else {
                    future::pending::<()>().await;
                }
            } => {
                // Record the event for tracking
                {
                    let mut diagnostics = hot_reload_state.diagnostics.write().unwrap();
                    diagnostics.record_daemon_event(DaemonEvent::DownloadCycle);
                }

                if let Err(e) = run_download_sync_with_hot_reload(&client, &hot_reload_state, filter.as_deref(), skip_existing, delete_after).await {
                    error!("Download sync failed: {}", e);
                }
            }

            // Handle torrent watch
            _ = async {
                if let Some(ref mut timer) = watch_timer {
                    timer.tick().await;
                } else {
                    future::pending::<()>().await;
                }
            } => {
                // Record the event for tracking
                {
                    let mut diagnostics = hot_reload_state.diagnostics.write().unwrap();
                    diagnostics.record_daemon_event(DaemonEvent::WatchCycle);
                }

                if let Err(e) = run_watch_check_with_hot_reload(&client, &hot_reload_state, &watch_dirs, parent_id).await {
                    error!("Watch check failed: {}", e);
                }
            }

            // Handle config changes
            new_config = config_change_rx.recv() => {
                match new_config {
                    Some(config) => {
                        // Record the event for tracking
                        {
                            let mut diagnostics = hot_reload_state.diagnostics.write().unwrap();
                            diagnostics.record_daemon_event(DaemonEvent::ConfigReload);
                        }

                        if let Err(e) = handle_config_change(&hot_reload_state, config, &mut download_timer, &mut watch_timer).await {
                            error!("Failed to handle config change: {}", e);
                        }
                    }
                    None => {
                        warn!("Config change channel closed");
                    }
                }
            }

            // Handle periodic health reporting
            _ = health_timer.tick() => {
                let download_status = generate_current_download_status(&hot_reload_state);
                let watch_status = generate_current_watch_status(&hot_reload_state, &watch_dirs);
                
                let summary = {
                    let diagnostics = hot_reload_state.diagnostics.read().unwrap();
                    diagnostics.generate_health_summary(&download_status, &watch_status)
                };
                
                info!("📊 {}", summary);
            }

            // Handle shutdown signal
            _ = tokio::signal::ctrl_c() => {
                info!("Received Ctrl+C signal, stopping daemon...");
                break;
            }
        }
    }

    // Graceful shutdown
    if let Some(worker_pool) = hot_reload_state.worker_pool.write().unwrap().take() {
        info!("Shutting down worker pool...");
        worker_pool.shutdown().await?;
    }

    info!("Hot-reload daemon stopped successfully");
    Ok(())
}

/// Start watching the configuration file for changes
async fn start_config_watching(
    config_path: &PathBuf,
    config_change_tx: mpsc::UnboundedSender<Config>,
) -> Result<RecommendedWatcher> {
    let config_path_for_closure = config_path.clone();
    let config_path_for_watch = config_path.clone();
    let change_tx = config_change_tx.clone();

    info!("Starting config file watcher for: {}", config_path.display());

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        match res {
            Ok(event) => {
                if let EventKind::Modify(_) = event.kind {
                    if event.paths.iter().any(|p| p == &config_path_for_closure) {
                        info!("Config file changed, reloading...");
                        
                        match Config::load(&config_path_for_closure) {
                            Ok(new_config) => {
                                if let Err(e) = change_tx.send(new_config) {
                                    error!("Failed to send config change notification: {}", e);
                                }
                            }
                            Err(e) => {
                                error!("Failed to reload config: {}", e);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!("Config file watch error: {}", e);
            }
        }
    })?;

    // Watch the config file's directory since file replacements might not trigger direct file watches
    if let Some(parent_dir) = config_path_for_watch.parent() {
        watcher.watch(parent_dir, RecursiveMode::NonRecursive)?;
        debug!("Watching directory: {}", parent_dir.display());
    } else {
        return Err(anyhow!("Config file has no parent directory"));
    }

    Ok(watcher)
}

/// Handle configuration changes with intelligent worker scaling
async fn handle_config_change(
    state: &HotReloadState,
    new_config: Config,
    _download_timer: &mut Option<tokio::time::Interval>,
    _watch_timer: &mut Option<tokio::time::Interval>,
) -> Result<()> {
    info!("Processing configuration change...");

    let old_config = {
        let config = state.config.read().unwrap();
        config.clone()
    };

    // Check what changed
    let workers_changed = old_config.download.workers != new_config.download.workers;
    let download_settings_changed = 
        old_config.download.buffer_size_mb != new_config.download.buffer_size_mb ||
        old_config.download.chunk_size_mb != new_config.download.chunk_size_mb ||
        old_config.download.bandwidth_limit_bytes_per_sec != new_config.download.bandwidth_limit_bytes_per_sec;

    // Update stored configuration
    {
        let mut config = state.config.write().unwrap();
        *config = new_config.clone();
    }

    info!("Configuration updated - workers_changed: {}, download_settings_changed: {}", 
          workers_changed, download_settings_changed);

    // Handle worker pool scaling if needed
    if workers_changed {
        scale_worker_pool(state, old_config.download.workers, new_config.download.workers).await?;
    }

    if download_settings_changed {
        info!("Download settings updated - applying to download operations");
        
        // Update download operations with new config
        {
            let mut ops_guard = state.download_ops.write().unwrap();
            if let Some(ref mut ops) = *ops_guard {
                ops.update_config(new_config.download.clone());
            }
        }
    }

    // Note: Timer intervals are CLI-controlled, not config-controlled in this implementation
    // If you want config-controlled intervals, you would recreate the timers here

    Ok(())
}

/// Scale worker pool intelligently based on active downloads
async fn scale_worker_pool(
    state: &HotReloadState,
    old_workers: usize,
    new_workers: usize,
) -> Result<()> {
    info!("Scaling worker pool from {} to {} workers", old_workers, new_workers);

    if new_workers > old_workers {
        // Scaling up - easier, just create new workers
        scale_up_workers(state, new_workers).await?;
    } else if new_workers < old_workers {
        // Scaling down - need to handle active downloads intelligently
        scale_down_workers(state, new_workers).await?;
    }

    Ok(())
}

/// Scale up workers (add new workers)
async fn scale_up_workers(state: &HotReloadState, new_count: usize) -> Result<()> {
    info!("Scaling up to {} workers", new_count);

    // Replace worker pool
    {
        let mut worker_pool = state.worker_pool.write().unwrap();
        
        // Shutdown existing pool
        if let Some(pool) = worker_pool.take() {
            // Let it shutdown in background to avoid blocking
            tokio::spawn(async move {
                if let Err(e) = pool.shutdown().await {
                    warn!("Error during worker pool shutdown: {}", e);
                }
            });
        }

        // Create new pool with more workers
        *worker_pool = Some(WorkerPool::new(new_count));
    }

    // Record worker restart event
    {
        let mut diagnostics = state.diagnostics.write().unwrap();
        diagnostics.record_daemon_event(DaemonEvent::WorkerRestart);
    }

    info!("Successfully scaled up to {} workers", new_count);
    Ok(())
}

/// Scale down workers with intelligent download preservation
async fn scale_down_workers(state: &HotReloadState, new_count: usize) -> Result<()> {
    info!("Scaling down to {} workers", new_count);

    // Get active downloads and analyze them
    let active_downloads = {
        let downloads = state.active_downloads.read().unwrap();
        downloads.clone()
    };

    let mut downloads_to_interrupt = Vec::new();
    let mut downloads_to_preserve = Vec::new();

    for (job_id, download) in active_downloads {
        if download.should_preserve() {
            info!("Preserving download: {} ({}% complete)", 
                  download.job.filename, download.completion_percentage());
            downloads_to_preserve.push((job_id.clone(), download));
        } else {
            warn!("Will interrupt download: {} ({}% complete)", 
                  download.job.filename, download.completion_percentage());
            downloads_to_interrupt.push((job_id.clone(), download));
        }
    }

    // Add interrupted downloads to the front of the queue with higher priority
    let interrupt_count = downloads_to_interrupt.len();
    {
        let mut queue = state.download_queue.write().unwrap();
        for (_, download) in downloads_to_interrupt {
            let filename = download.job.filename.clone(); // Save filename before moving
            let mut job = download.job;
            job.priority += 1000; // Boost priority for interrupted downloads
            queue.insert(0, job);
            info!("Added interrupted download to priority queue: {}", filename);
        }

        // Sort by priority (highest first)
        queue.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    // Replace worker pool
    {
        let mut worker_pool = state.worker_pool.write().unwrap();
        
        // Shutdown existing pool
        if let Some(pool) = worker_pool.take() {
            // Shutdown in background to avoid blocking  
            tokio::spawn(async move {
                if let Err(e) = pool.shutdown().await {
                    warn!("Error during worker pool shutdown: {}", e);
                }
            });
        }

        // Create new pool with fewer workers
        *worker_pool = Some(WorkerPool::new(new_count));
    }

    // Clear active downloads tracking (they'll be re-added as they restart)
    {
        let mut active = state.active_downloads.write().unwrap();
        active.clear();
    }

    // Record worker restart event
    {
        let mut diagnostics = state.diagnostics.write().unwrap();
        diagnostics.record_daemon_event(DaemonEvent::WorkerRestart);
    }

    info!("Successfully scaled down to {} workers, {} downloads interrupted and prioritized", 
          new_count, interrupt_count);

    Ok(())
}

/// Run download sync with hot reload support
async fn run_download_sync_with_hot_reload(
    _client: &ApiClient,
    state: &HotReloadState,
    filter: Option<&str>,
    _skip_existing: bool,
    _delete_after: bool,
) -> Result<()> {
    let start_time = Instant::now();
    debug!("Starting download sync check with hot reload...");

    // Get current download operations
    let download_ops = {
        let ops_guard = state.download_ops.read().unwrap();
        if let Some(ref _ops) = *ops_guard {
            // We need to clone or share the DownloadOps safely
            // For now, let's implement the sync logic here directly
            drop(ops_guard); // Release the lock
            Some(())
        } else {
            None
        }
    };

    if download_ops.is_none() {
        debug!("Download operations not initialized (watch-only mode)");
        return Ok(());
    }

    let _config = {
        let config = state.config.read().unwrap();
        config.clone()
    };

    let active_count = state.active_downloads.read().unwrap().len();
    let queued_count = state.download_queue.read().unwrap().len();
    let worker_count = {
        let pool = state.worker_pool.read().unwrap();
        pool.as_ref().map(|p| p.worker_count()).unwrap_or(0)
    };

    info!("Download sync check - workers: {}, active: {}, queued: {}", 
          worker_count, active_count, queued_count);

    // Run download sync cycle using the shared DownloadOps
    let result = {
        let mut ops_guard = state.download_ops.write().unwrap();
        if let Some(ref mut ops) = *ops_guard {
            // Use daemon-specific parameters - get all files from configured parent
            let all = true; // For daemon mode, scan all files
            let specific_files = &[]; // No specific files in daemon mode
            let show_progress = false; // Don't show progress bars in daemon mode
            
            ops.run_sync_cycle(filter, all, specific_files, show_progress).await
        } else {
            return Ok(()); // No operations available
        }
    };

    match result {
        Ok(cycle_result) => {
            let elapsed = start_time.elapsed().as_secs_f64();
            
            if cycle_result.is_empty() {
                debug!("Download sync completed in {:.1}s - no new files found", elapsed);
            } else {
                info!(
                    "📊 Download sync completed in {:.1}s - found: {}, downloaded: {}, failed: {}, total: {} MB",
                    elapsed,
                    cycle_result.files_found,
                    cycle_result.files_downloaded,
                    cycle_result.files_failed,
                    cycle_result.total_bytes / 1024 / 1024
                );
            }
        }
        Err(e) => {
            error!("Download sync failed: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

/// Run watch check with hot reload support
async fn run_watch_check_with_hot_reload(
    client: &ApiClient,
    state: &HotReloadState,
    watch_dirs: &[PathBuf],
    parent_id: Option<i64>,
) -> Result<()> {
    let start_time = Instant::now();
    debug!("Starting torrent watch check with hot reload...");

    // Use the watch core if available
    if let Some(ref watch_core) = state.watch_core {
        let stats = watch_core.scan_directories(watch_dirs, client, parent_id).await?;
        
        let elapsed = start_time.elapsed();
        
        if stats.files_found > 0 {
            info!(
                "Torrent watch completed in {:.1}s - scanned: {}, found: {}, processed: {}, skipped: {}, failed: {}",
                elapsed.as_secs_f64(),
                stats.directories_scanned,
                stats.files_found,
                stats.files_processed,
                stats.files_skipped,
                stats.files_failed
            );
        } else {
            debug!("Torrent watch completed in {:.1}s - no new files found", elapsed.as_secs_f64());
        }
    } else {
        debug!("Watch core not available - skipping watch check");
    }

    Ok(())
}


/// Generate current download status for health reporting
fn generate_current_download_status(state: &HotReloadState) -> DownloadStatus {
    let mut status = DownloadStatus::default();

    let config = state.config.read().unwrap();
    status.workers_configured = config.download.workers;
    drop(config);

    status.downloads_active = state.active_downloads.read().unwrap().len();
    status.downloads_queued = state.download_queue.read().unwrap().len();

    if let Some(ref worker_pool) = *state.worker_pool.read().unwrap() {
        status.workers_active = worker_pool.worker_count() - worker_pool.available_workers();
    }

    // TODO: Add more detailed status tracking (total downloaded, speeds, etc.)
    
    status
}

/// Generate current watch status for health reporting
fn generate_current_watch_status(_state: &HotReloadState, watch_dirs: &[PathBuf]) -> WatchStatus {
    let mut status = WatchStatus::default();

    for dir in watch_dirs {
        status.directories_watched.push(dir.display().to_string());
        if dir.exists() && dir.is_dir() {
            status.directories_accessible += 1;
        }
    }

    // TODO: Add tracking for files processed, uploads, etc. via watch core
    
    status
}

