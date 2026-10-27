//! Daemon command implementation
//!
//! Delegates to the putio-sync-daemon crate for full daemon functionality

use anyhow::{anyhow, Context, Result};
use putio_sync_core::cleanup::EmptyFolderCleanup;
use putio_sync_core::config::{AuthMethod, Config};
use putio_sync_daemon::{DaemonConfig, PutioDaemon};
use putio_sync_download::DownloadConfig;
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

    // Create daemon configuration
    let daemon_config = daemon_config(
        &config,
        oauth_token,
        watch_dirs,
        watch_interval,
        download_interval,
        parent_id,
    );

    let queue_db = Arc::new(OnceLock::new());
    crate::telemetry::start(
        &config,
        queue_db.clone(),
        daemon_config.download.workers,
        daemon_config.download.target_directory.clone(),
    )
    .await?;

    // Reload [logging] when the config file changes (FR-016)
    putio_sync_core::config::spawn_reload_poller(
        config_file_path.clone(),
        std::time::Duration::from_secs(10),
        |logging| {
            let _ = putio_sync_core::logging::apply_level(&logging);
        },
    );

    info!("Daemon configuration:");
    let download = &daemon_config.download;
    info!(
        "  Download directory: {}",
        download.target_directory.display()
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
    info!("  Download workers: {}", download.workers);
    info!("  Buffer size: {}MB", download.buffer_size_mb);
    info!(
        "  Bandwidth limit: {}",
        download
            .bandwidth_limit_bytes_per_sec
            .map_or("unlimited".to_string(), |b| format!("{} bytes/s", b))
    );
    info!(
        "  Retries: {} (initial delay {}s)",
        download.max_retries, download.retry_initial_delay_secs
    );
    info!("  Skip existing: {}", download.skip_existing);
    info!("  Parent folder ID: {:?}", daemon_config.parent_id);
    info!(
        "  Delete after download: {}",
        download.delete_after_download
    );
    info!(
        "  Delete after upload: {}",
        daemon_config.delete_after_upload
    );
    info!("  Archive directory: {:?}", daemon_config.archive_dir);
    info!(
        "  Blocked extensions: {}",
        if config.download.blocked_extensions.is_empty() {
            "none".to_string()
        } else {
            config.download.blocked_extensions.join(", ")
        }
    );
    info!("  Folder rules: {}", config.download.rules.len());
    info!(
        "  Empty-folder cleanup: {} (daily at {}, min age {}h)",
        if config.cleanup.enabled {
            "enabled"
        } else {
            "disabled"
        },
        config.cleanup.run_at,
        config.cleanup.min_age_hours
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

    // One cleanup service for the daily run and the API, so their runs never overlap
    let cleanup = daemon
        .get_client()
        .map(|client| Arc::new(EmptyFolderCleanup::from_config(client, &config)));
    if let Some(cleanup) = &cleanup {
        daemon = daemon.with_cleanup(cleanup.clone());
    }

    // API and web UI on the daemon's own engine (feature 004). Needs a local download
    // manager, so it is skipped when downloads are delegated to a remote API.
    match (daemon.get_client(), daemon.get_download_manager()) {
        (Some(client), Some(download_manager)) => {
            let state = putio_sync_api::AppState::new(
                config.clone(),
                Some(config_file_path),
                client,
                daemon.get_database(),
                download_manager,
                daemon.get_file_watcher(),
            )
            .with_stall_tracker(daemon.stall_tracker())
            .on_daemon();
            let state = match &cleanup {
                Some(cleanup) => state.with_cleanup(cleanup.clone()),
                None => state,
            };
            crate::api_listener::start(&config, state).await?;
        }
        _ => warn!("API listener not started: no local download manager (api.enabled is set)"),
    }

    info!("✅ Daemon initialized successfully");
    info!("🔄 Starting daemon loop...");
    info!("Press Ctrl+C to stop");

    daemon.start().await.context("Daemon execution failed")?;

    info!("👋 Daemon stopped gracefully");
    Ok(())
}

/// The daemon's configuration from `config.toml` and the CLI arguments. Every
/// `[download]` setting goes through the shared `DownloadConfig::from` conversion.
fn daemon_config(
    config: &Config,
    oauth_token: String,
    watch_dirs: Vec<PathBuf>,
    watch_interval: u64,
    download_interval: u64,
    parent_id: Option<i64>,
) -> DaemonConfig {
    DaemonConfig {
        oauth_token,
        database_path: Some(config.download.target_directory.join(".putiosync.db")),
        download: DownloadConfig::from(&config.download),
        watch_dirs,
        watch_interval,
        download_interval,
        parent_id,
        delete_after_upload: config.watch.delete_after_upload,
        archive_dir: config.watch.archive_directory.clone(),
        watch_enabled: config.watch.enabled,
        api_enabled: config.api.enabled,
        api_url: config.api.url.clone(),
        stall_threshold: std::time::Duration::from_secs(
            config.transfers.stall_threshold_hours.saturating_mul(3600),
        ),
        stall_check_interval: config.transfers.stall_check_interval_secs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `[transfers]` reaches the daemon's stall check (feature 007)
    #[test]
    fn daemon_config_takes_the_stall_settings() {
        let mut config = Config::default();
        config.transfers.stall_threshold_hours = 12;
        config.transfers.stall_check_interval_secs = 120;
        let daemon = daemon_config(&config, "t".to_string(), vec![], 60, 300, None);
        assert_eq!(
            daemon.stall_threshold,
            std::time::Duration::from_secs(12 * 3600)
        );
        assert_eq!(daemon.stall_check_interval, 120);
    }

    /// Every `[download]` setting the manager has reaches the daemon, not only the
    /// target directory, workers, buffer, delete-after and blocklist it used to copy
    #[test]
    fn daemon_config_takes_every_download_setting() {
        let mut config = Config::default();
        config.download.target_directory = PathBuf::from("/srv/putio");
        config.download.workers = 5;
        config.download.buffer_size_mb = 48;
        config.download.skip_existing = true;
        config.download.delete_after_download = true;
        config.download.bandwidth_limit_bytes_per_sec = Some(2_097_152);
        config.download.max_retries = 8;
        config.download.retry_initial_delay_secs = 6;
        config.download.blocked_extensions = vec!["iso".to_string()];

        let daemon = daemon_config(&config, "t".to_string(), vec![], 60, 300, None);
        assert_eq!(
            daemon.download,
            DownloadConfig {
                target_directory: PathBuf::from("/srv/putio"),
                skip_existing: true,
                buffer_size_mb: 48,
                bandwidth_limit_bytes_per_sec: Some(2_097_152),
                max_retries: 8,
                retry_initial_delay_secs: 6,
                workers: 5,
                delete_after_download: true,
                rules: putio_sync_core::rules::RuleSet::from_config(&config.download),
            }
        );
        assert_eq!(
            daemon.database_path,
            Some(PathBuf::from("/srv/putio/.putiosync.db"))
        );
    }
}
