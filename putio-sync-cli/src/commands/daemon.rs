//! Daemon command implementation
//!
//! Delegates to the putio-sync-daemon crate for full daemon functionality

use anyhow::{anyhow, Context, Result};
use putio_sync_core::config::{AuthMethod, Config};
use putio_sync_daemon::{DaemonConfig, PutioDaemon};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tracing::{info, warn};

/// Execute the daemon command
///
/// This creates a daemon configuration from CLI arguments and config file,
/// then delegates to PutioDaemon for execution.
#[allow(clippy::too_many_arguments)]
pub async fn execute_daemon(
    download_interval: u64,
    watch_interval: u64,
    watch_paths: Vec<PathBuf>,
    filter: Option<String>,
    skip_existing: bool,
    delete_after: bool,
    delete_empty_folders: Option<bool>,
    parent_id: Option<i64>,
    watch_only: bool,
    download_only: bool,
    archive_dir: Option<PathBuf>,
    delete_torrents: bool,
    config_path: Option<PathBuf>,
) -> Result<()> {
    info!("🚀 Starting Put.io sync daemon");

    // Load configuration
    let config_file_path = config_path
        .or_else(|| Config::default_path().ok())
        .ok_or_else(|| anyhow!("Could not determine config file path"))?;

    info!("Loading configuration from: {}", config_file_path.display());
    let config = Config::load(&config_file_path)
        .context("Failed to load configuration. Have you run 'putio-sync login'?")?;

    let auth = config.auth.clone().ok_or_else(|| {
        anyhow!("No authentication credentials found. Please run 'putio-sync login' first.")
    })?;

    // Extract OAuth token from auth config
    let oauth_token = match auth.auth_method {
        AuthMethod::Token => auth.token.ok_or_else(|| anyhow!("Token not configured"))?,
        AuthMethod::AppPassword => {
            // For app password, we'll need to create a client and get a session token
            // For now, return an error as daemon mode requires OAuth token
            return Err(anyhow!(
                "Daemon mode requires OAuth token authentication. App password not supported yet."
            ));
        }
    };

    // Determine watch directories
    let watch_dirs = if watch_paths.is_empty() {
        config.watch.paths.clone()
    } else {
        watch_paths
    };

    // Determine download directory, workers, buffer size, and delete_after_download
    let download_dir = config.download.target_directory.clone();
    let workers = config.download.workers;
    let buffer_size_mb = config.download.buffer_size_mb;
    let delete_after_download = config.download.delete_after_download;

    // Warn about unsupported CLI flags
    if filter.is_some() {
        warn!("--filter flag not yet supported in daemon mode, ignoring");
    }
    if skip_existing {
        warn!("--skip-existing flag not yet supported in daemon mode, ignoring");
    }
    if delete_after {
        warn!("--delete-after flag not yet supported in daemon mode, ignoring");
    }
    if delete_empty_folders.is_some() {
        warn!("--delete-empty-folders flag not yet supported in daemon mode, ignoring");
    }
    if watch_only {
        warn!("--watch-only flag not yet supported in daemon mode, ignoring");
    }
    if download_only {
        warn!("--download-only flag not yet supported in daemon mode, ignoring");
    }
    if archive_dir.is_some() {
        warn!("--archive-dir flag not yet supported in daemon mode, ignoring");
    }
    if delete_torrents {
        warn!("--delete-torrents flag not yet supported in daemon mode, ignoring");
    }
    if config.watch.archive_directory.is_some() {
        warn!("watch.archive_directory is not supported in daemon mode, ignoring");
    }

    // Create daemon configuration
    let daemon_config = DaemonConfig {
        oauth_token,
        database_path: Some(download_dir.join(".putiosync.db")),
        download_dir,
        watch_dirs,
        watch_interval,
        download_interval,
        parent_id,
        delete_after_upload: config.watch.delete_after_upload,
        watch_enabled: config.watch.enabled,
        workers,
        buffer_size_mb,
        delete_after_download,
        api_enabled: config.api.enabled,
        api_url: config.api.url.clone(),
        blocked_extensions: config.download.blocked_extensions.clone(),
    };

    let queue_db = Arc::new(OnceLock::new());
    crate::telemetry::start(
        &config,
        queue_db.clone(),
        daemon_config.workers,
        daemon_config.download_dir.clone(),
    )
    .await?;

    // Reload [logging] when the config file changes (FR-016)
    putio_sync_core::config::spawn_reload_poller(
        config_file_path,
        std::time::Duration::from_secs(10),
        |logging| {
            let _ = putio_sync_core::logging::apply_level(&logging);
        },
    );

    info!("Daemon configuration:");
    info!(
        "  Download directory: {}",
        daemon_config.download_dir.display()
    );
    info!(
        "  Watch directories: {} paths",
        daemon_config.watch_dirs.len()
    );
    for path in &daemon_config.watch_dirs {
        info!("    - {}", path.display());
    }
    info!("  Watch interval: {}s", daemon_config.watch_interval);
    info!(
        "  Put.io sync interval: {}s",
        daemon_config.download_interval
    );
    info!("  Download workers: {}", daemon_config.workers);
    info!("  Buffer size: {}MB", daemon_config.buffer_size_mb);
    info!("  Parent folder ID: {:?}", daemon_config.parent_id);
    info!(
        "  Delete after download: {}",
        daemon_config.delete_after_download
    );
    info!(
        "  Delete after upload: {}",
        daemon_config.delete_after_upload
    );
    info!(
        "  Blocked extensions: {}",
        if daemon_config.blocked_extensions.is_empty() {
            "none".to_string()
        } else {
            daemon_config.blocked_extensions.join(", ")
        }
    );

    // Create and start daemon
    let mut daemon = PutioDaemon::new(daemon_config);

    info!("Initializing daemon components...");
    daemon
        .initialize()
        .await
        .context("Failed to initialize daemon")?;

    if let Some(db) = daemon.get_database() {
        let _ = queue_db.set(db);
    }

    info!("✅ Daemon initialized successfully");
    info!("🔄 Starting daemon loop...");
    info!("Press Ctrl+C to stop");

    daemon.start().await.context("Daemon execution failed")?;

    info!("👋 Daemon stopped gracefully");
    Ok(())
}
