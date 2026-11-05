//! Daemon command implementation
//!
//! Delegates to the putio-sync-daemon crate for full daemon functionality

use anyhow::{anyhow, Context, Result};
use putio_sync_core::cleanup::EmptyFolderCleanup;
use putio_sync_core::config::{AuthMethod, Config, ExistingFile};
use putio_sync_daemon::{DaemonConfig, PutioDaemon};
use putio_sync_doctor::Doctor;
use putio_sync_download::DownloadConfig;
use putio_sync_hooks::HooksService;
use putio_sync_plugins::PluginsService;
use putio_sync_storage::StorageService;
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
    on_existing: Option<ExistingFile>,
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

    // Without a usable config, wait in setup mode instead of exiting (feature 013)
    while let Some((base, reasons)) = super::setup_mode::needs_setup(&config_file_path).await {
        if !super::setup_mode::run(&config_file_path, base, reasons).await? {
            return Ok(());
        }
    }

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
    let mut daemon_config = daemon_config(
        &config,
        oauth_token,
        watch_dirs,
        watch_interval,
        download_interval,
        parent_id,
    );
    // --on-existing / --skip-existing for this run; folder rules still win per folder
    if let Some(choice) = on_existing {
        daemon_config.download.on_existing = choice;
    }

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
            putio_sync_core::log_capture::set_capacity(logging.buffer_lines);
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
    info!("  On existing: {}", download.on_existing);
    info!("  Resume partial downloads: {}", download.resume_enabled);
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
    info!(
        "  Put.io storage: warn below {}% free, free up to {}%, automatic free-up {}",
        config.storage.warn_free_percent,
        config.storage.target_free_percent,
        if config.storage.auto_free {
            format!("daily at {}", config.storage.run_at)
        } else {
            "off".to_string()
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

    // One cleanup service for the daily run and the API, so their runs never overlap
    let cleanup = daemon
        .get_client()
        .map(|client| Arc::new(EmptyFolderCleanup::from_config(client, &config)));
    if let Some(cleanup) = &cleanup {
        daemon = daemon.with_cleanup(cleanup.clone());
    }

    // One storage service for the daemon's loop and the API, so free-ups never overlap
    let storage = daemon.get_client().map(|client| {
        Arc::new(StorageService::from_config(
            client,
            daemon.get_database(),
            &config,
        ))
    });
    if let Some(storage) = &storage {
        daemon = daemon.with_storage(storage.clone());
    }

    // One hooks service for the event bus and the API, so a test event and a settings
    // save reach the hooks that run (feature 027)
    let hooks = Arc::new(HooksService::new(config.hooks.clone()));
    hooks.start();

    // One plugins service likewise (feature 028)
    let plugins = Arc::new(PluginsService::from_config(
        &config,
        Some(config_file_path.as_path()),
    ));
    plugins.start();

    // One doctor for the scheduled runs and the API, so the Doctor page shows both
    let doctor = match (daemon.get_client(), &cleanup) {
        (Some(client), Some(cleanup)) => Some(Arc::new(
            Doctor::new(
                config.clone(),
                client,
                daemon.get_database(),
                cleanup.clone(),
            )
            .with_stalls(daemon.stall_tracker())
            .with_hooks(hooks.clone())
            .with_plugins(plugins.clone())
            .with_config_path(Some(config_file_path.clone()))
            .with_metrics(),
        )),
        _ => None,
    };
    if let Some(doctor) = &doctor {
        daemon = daemon.with_doctor(doctor.clone());
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
            let state = match &doctor {
                Some(doctor) => state.with_doctor(doctor.clone()),
                None => state,
            };
            let state = match &storage {
                Some(storage) => state.with_storage(storage.clone()),
                None => state,
            };
            let state = state
                .with_hooks(hooks.clone())
                .with_plugins(plugins.clone());
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
        finished_retention_hours: config.transfers.finished_retention_hours,
        precheck: config
            .resolve_precheck()
            .map_err(|e| tracing::warn!(error = %e, "Release pre-check disabled for watch folders"))
            .ok(),
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

    /// `[precheck]` reaches the watch folders' release check (042 US4)
    #[test]
    fn daemon_config_takes_the_precheck() {
        let mut config = Config::default();
        config.precheck.dry_run = true;
        let daemon = daemon_config(&config, "t".to_string(), vec![], 60, 300, None);
        assert!(daemon.precheck.is_some_and(|p| p.dry_run));
    }

    /// Every `[download]` setting the manager has reaches the daemon, not only the
    /// target directory, workers, buffer, delete-after and blocklist it used to copy
    #[test]
    fn daemon_config_takes_every_download_setting() {
        let mut config = Config::default();
        config.download.target_directory = PathBuf::from("/srv/putio");
        config.download.workers = 5;
        config.download.buffer_size_mb = 48;
        config.download.on_existing = Some(ExistingFile::KeepBoth);
        config.download.resume_enabled = false;
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
                on_existing: ExistingFile::KeepBoth,
                resume_enabled: false,
                buffer_size_mb: 48,
                bandwidth_limit_bytes_per_sec: Some(2_097_152),
                bandwidth_schedule: Vec::new(),
                slow_limit_bytes_per_sec: putio_sync_core::bandwidth::DEFAULT_SLOW_LIMIT,
                max_retries: 8,
                retry_initial_delay_secs: 6,
                workers: 5,
                delete_after_download: true,
                rules: putio_sync_core::rules::RuleSet::from_config(&config.download),
                verify: putio_sync_download::manager::VerifySettings::default(),
            }
        );
        assert_eq!(
            daemon.database_path,
            Some(PathBuf::from("/srv/putio/.putiosync.db"))
        );
    }
}
