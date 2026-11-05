//! Server command implementation
//!
//! Starts an HTTP API server with optional daemon modes for watch and download.
//!
//! This is the thin dev mode: it builds its own engine through `create_app_state`. Of
//! `[api]` it reads the token (`auth_token` / `PUTIO_SYNC_API_TOKEN`) and the addresses
//! (feature 037): it listens where `daemon` would (`[api] bind`, tailnet-only), unless
//! `--bind` / `--port` say otherwise (see [`server_listen`]). `api.listen` is for
//! `daemon` only.

use anyhow::{anyhow, bail, Context, Result};
use putio_sync_api::listen::{Bound, Supervisor};
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::{AuthMethod, Config};
use putio_sync_core::listen::ListenAddr;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadFile, DownloadManager};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::signal;
use tracing::{debug, error, info, warn};

/// Execute the server command
///
/// Starts the HTTP API server and optionally enables watch and download daemon modes.
///
/// # Arguments
///
/// * `watch` - Enable watch mode for monitoring torrent directories
/// * `download` - Enable automatic download mode
/// * `port` - Port for every API address, else the configured ports
/// * `bind` - One address instead of `[api] bind`
/// * `config_path` - Optional custom config file path
///
/// # Examples
///
/// ```no_run
/// # use putio_sync_cli::commands::execute_server;
/// # async fn example() {
/// // Start server only, on the configured addresses
/// execute_server(false, false, None, None, None).await.unwrap();
///
/// // Start server with watch and download modes on 0.0.0.0:15432
/// execute_server(true, true, Some(15432), Some("0.0.0.0".to_string()), None).await.unwrap();
/// # }
/// ```
pub async fn execute_server(
    watch: bool,
    download: bool,
    port: Option<u16>,
    bind: Option<String>,
    config_path: Option<PathBuf>,
) -> Result<()> {
    info!("Starting putio-sync server");

    // Load configuration
    let config_file_path = config_path
        .or_else(|| Config::default_path().ok())
        .context("Could not determine config path")?;

    let mut config = if config_file_path.exists() {
        Config::load(&config_file_path).context("Failed to load configuration")?
    } else {
        warn!("Config file not found, using default configuration");
        Config::default()
    };

    // Validate config
    config
        .validate()
        .context("Configuration validation failed")?;

    info!("Configuration loaded successfully");

    // Only the token applies in this mode; the env override wins over the file
    config.api.auth_token = config.resolve_api()?.value.token;
    if config.api.auth_token.is_none() {
        warn!("API token not configured (set [api] auth_token or PUTIO_SYNC_API_TOKEN); all API and UI requests will be refused");
    }
    debug!(
        "Watch config: enabled={}, paths={:?}, archive_dir={:?}, delete_after_upload={}",
        config.watch.enabled,
        config.watch.paths,
        config.watch.archive_directory,
        config.watch.delete_after_upload
    );

    // Metrics run in every server configuration, even without a token (FR-007)
    let queue_db = Arc::new(OnceLock::new());
    crate::telemetry::start(
        &config,
        queue_db.clone(),
        config.download.workers,
        config.download.target_directory.clone(),
    )
    .await?;

    // Reload [logging] when the config file changes (FR-016); nothing to watch without one
    if config_file_path.exists() {
        putio_sync_core::config::spawn_reload_poller(
            config_file_path.clone(),
            std::time::Duration::from_secs(10),
            |logging| {
                let _ = putio_sync_core::logging::apply_level(&logging);
                putio_sync_core::log_capture::set_capacity(logging.buffer_lines);
            },
        );
    }

    let flags = bind.is_some() || port.is_some();
    let entries = server_listen(&config, bind.as_deref(), port)?;
    let shown = entries
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    info!("HTTP API server will listen on: {}", shown);

    // Report enabled modes
    if watch {
        info!("Watch mode: ENABLED");
        if !config.watch.enabled {
            warn!("Watch mode requested but not enabled in config");
        }
    } else {
        info!("Watch mode: DISABLED");
    }

    if download {
        info!("Download mode: ENABLED");
    } else {
        info!("Download mode: DISABLED");
    }

    // Create application with config (with database path and config path)
    let database_path = Some(std::path::PathBuf::from(".putiosync.db"));
    let bound = Bound::default();
    let state = putio_sync_api::create_app_state(
        config.clone(),
        database_path,
        Some(config_file_path.clone()),
        None, // No file watcher in standalone server mode
    )
    .await
    .map_err(|e| anyhow!("Failed to create API app: {}", e))?
    .with_listening(bound.clone())
    .with_listen_flags(flags.then(|| entries.clone()));
    // Hooks run off the event bus from here on (feature 027)
    state.hooks.start();
    // and so do plugins (feature 028)
    state.plugins.start();
    // This mode has no daemon loops, so poll for `transfer.*` and `storage.low` here
    // (feature 027, R6 and R7)
    tokio::spawn(hook_events_ticker(state.clone()));

    // Every address through the supervisor: one that can't be had now is retried, and
    // nothing falls back to another address
    let handle = Supervisor::spawn(
        "API",
        entries,
        state.tailscale.clone(),
        putio_sync_api::router(state.clone()),
        bound.clone(),
    );
    match handle.first_bound(Duration::from_secs(2)).await {
        Some(addr) => {
            info!("✓ HTTP API server started successfully");
            info!("API endpoints available at:");
            info!("  - Health:    http://{}/health", addr);
            info!("  - Put.io:    http://{}/putio/{{files,transfers}}", addr);
            info!(
                "  - Local:     http://{}/local/{{files,queue,downloads,watcher,workers}}",
                addr
            );
            info!("  - System:    http://{}/{{settings,stats,history}}", addr);
        }
        None => warn!("No API address could be listened on yet ({shown}); still trying"),
    }
    // After the first bind, so Radarr/Sonarr hear the port actually listened on
    putio_sync_api::spawn_arr_refresh(&state).await;
    info!("");
    info!("Press Ctrl+C to stop the server");

    // Start watch mode thread if enabled
    let _watch_handle = if watch {
        if !config.watch.enabled {
            warn!("Watch mode enabled via flag but not configured in config.toml");
            warn!("No watch directories will be monitored");
            None
        } else if config.watch.paths.is_empty() {
            warn!("Watch mode enabled but no watch paths configured");
            None
        } else {
            info!("Starting watch mode daemon...");
            Some(spawn_watch_daemon(config.clone()))
        }
    } else {
        None
    };

    // Start download mode thread if enabled
    let _download_handle = if download {
        info!("Starting download mode daemon...");
        Some(spawn_download_daemon(config.clone(), queue_db))
    } else {
        None
    };

    // Serve until Ctrl+C or SIGTERM, then let open requests finish
    shutdown_signal().await;
    handle.stop(Duration::from_secs(10)).await;

    info!("Server shut down gracefully");

    // Note: Daemon threads will continue running until Ctrl+C is pressed again
    // The TorrentWatcher has its own Ctrl+C handler that will stop it gracefully
    // For now, we don't wait for the daemon threads to finish to allow quick server shutdown
    // In the future, we could implement a shared shutdown signal for all threads

    Ok(())
}

/// The addresses `server` listens on (feature 037):
///
/// | Flags | Listens on |
/// |-------|------------|
/// | none | [`Config::api_listen`] (`[api] bind`, or tailnet-only) |
/// | `--bind H` | `H` with the first configured port, or `H`'s own port |
/// | `--bind H --port P` | `H:P` |
/// | `--port P` | every configured entry, with port `P` |
/// | `--bind H:P2 --port P` | an error: give the port once |
///
/// `H` is an IP, an interface name or `tailnet`.
pub fn server_listen(
    config: &Config,
    bind: Option<&str>,
    port: Option<u16>,
) -> Result<Vec<ListenAddr>> {
    let configured = config.api_listen()?;
    let Some(host) = bind.map(str::trim) else {
        let Some(port) = port else {
            return Ok(configured);
        };
        let mut out: Vec<ListenAddr> = Vec::new();
        for entry in configured.iter().map(|e| e.with_port(port)) {
            if !out.contains(&entry) {
                out.push(entry);
            }
        }
        return Ok(out);
    };
    let with_port = |p: u16| -> Result<ListenAddr> {
        let entry = match host.parse::<IpAddr>() {
            Ok(IpAddr::V6(ip)) => format!("[{ip}]:{p}"),
            _ => format!("{host}:{p}"),
        };
        entry
            .parse()
            .map_err(|why| anyhow!("invalid --bind '{host}': {why}"))
    };
    // An address without its port: a bare IP, or a name that doesn't parse with one
    if host.parse::<IpAddr>().is_ok() || host.parse::<ListenAddr>().is_err() {
        let p = port
            .or_else(|| configured.first().map(ListenAddr::port))
            .unwrap_or(putio_sync_core::config::DEFAULT_API_PORT);
        return Ok(vec![with_port(p)?]);
    }
    if port.is_some() {
        bail!("--bind {host} already has a port; give the port once (--bind or --port)");
    }
    Ok(vec![host.parse().map_err(|why| {
        anyhow!("invalid --bind '{host}': {why}")
    })?])
}

/// Wait for Ctrl+C signal for graceful shutdown
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C, shutting down...");
        },
        _ = terminate => {
            info!("Received SIGTERM, shutting down...");
        },
    }
}

/// Spawn watch mode daemon thread
///
/// Monitors configured watch directories for new .torrent files and automatically
/// uploads them to Put.io.
fn spawn_watch_daemon(config: Config) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        use putio_sync_file_manager::TorrentWatcher;

        // Create a new Tokio runtime for this thread
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

        rt.block_on(async move {
            info!("Watch daemon started");
            info!("  Directories: {:?}", config.watch.paths);
            info!("  Interval: {}s", config.watch.refresh_cycle);
            info!("  Parent folder: {:?}", config.watch.parent_folder_id);

            // Create API client
            let auth_config = match config.auth {
                Some(auth) => auth,
                None => {
                    warn!("Watch daemon: No authentication configured, stopping");
                    return;
                }
            };

            let client = match auth_config.auth_method {
                AuthMethod::Token => {
                    let token = match auth_config.token {
                        Some(t) => t,
                        None => {
                            warn!("Token not configured, stopping");
                            return;
                        }
                    };
                    PutioApiClient::new(token)
                }
                AuthMethod::AppPassword => {
                    let username = match auth_config.username {
                        Some(u) => u,
                        None => {
                            warn!("Username not configured, stopping");
                            return;
                        }
                    };
                    let password = match auth_config.password {
                        Some(p) => p,
                        None => {
                            warn!("Password not configured, stopping");
                            return;
                        }
                    };
                    PutioApiClient::with_credentials(username, password)
                }
            };

            // Create and run watcher
            let watcher = TorrentWatcher::new(
                config.watch.paths.clone(),
                config.watch.parent_folder_id,
                config.watch.refresh_cycle as u64,
            );

            if let Err(e) = watcher.run_daemon(&client).await {
                warn!("Watch daemon error: {}", e);
            }

            info!("Watch daemon stopped");
        })
    })
}

/// Spawn download mode daemon thread
///
/// Monitors completed transfers and automatically downloads them to the configured
/// download directory.
fn spawn_download_daemon(
    config: Config,
    queue_db: Arc<OnceLock<Arc<Database>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        // Create a new Tokio runtime for this thread
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

        rt.block_on(async move {
            info!("Download daemon started");
            info!(
                "  Target directory: {}",
                config.download.target_directory.display()
            );
            info!("  Workers: {}", config.download.workers);

            // Create API client
            let auth_config = match config.auth {
                Some(auth) => auth,
                None => {
                    warn!("Download daemon: No authentication configured, stopping");
                    return;
                }
            };

            let client = match auth_config.auth_method {
                AuthMethod::Token => {
                    let token = match auth_config.token {
                        Some(t) => t,
                        None => {
                            warn!("Token not configured, stopping");
                            return;
                        }
                    };
                    Arc::new(PutioApiClient::new(token))
                }
                AuthMethod::AppPassword => {
                    let username = match auth_config.username {
                        Some(u) => u,
                        None => {
                            warn!("Username not configured, stopping");
                            return;
                        }
                    };
                    let password = match auth_config.password {
                        Some(p) => p,
                        None => {
                            warn!("Password not configured, stopping");
                            return;
                        }
                    };
                    Arc::new(PutioApiClient::with_credentials(username, password))
                }
            };

            // Initialize database for download tracking
            let database_path = std::path::PathBuf::from(".putiosync.db");
            let database = match Database::new(&database_path).await {
                Ok(db) => {
                    info!("Download daemon: Database initialized");
                    let db = Arc::new(db);
                    let _ = queue_db.set(db.clone());
                    Some(db)
                }
                Err(e) => {
                    warn!("Download daemon: Failed to initialize database: {}", e);
                    warn!("Continuing without database (download tracking disabled)");
                    None
                }
            };

            // Initialize download manager
            let download_config = DownloadConfig::from(&config.download);

            let download_manager = if let Some(ref db) = database {
                Arc::new(DownloadManager::new(
                    client.clone(),
                    db.clone(),
                    download_config,
                ))
            } else {
                Arc::new(DownloadManager::new_without_database(
                    client.clone(),
                    download_config,
                ))
            };

            if let Err(e) = download_manager.restore_bandwidth_override().await {
                warn!("Failed to restore the bandwidth override: {}", e);
            }
            info!("Download daemon: Manager initialized");

            // Run download daemon loop
            loop {
                // Check for completed transfers
                match client.list_transfers().await {
                    Ok(response) => {
                        let completed: Vec<_> = response
                            .transfers
                            .iter()
                            .filter(|t| t.status == "COMPLETED" && t.file_id.is_some())
                            .collect();

                        if !completed.is_empty() {
                            info!("Found {} completed transfer(s)", completed.len());

                            // Download each completed transfer
                            for transfer in completed {
                                let file_id = match transfer.file_id {
                                    Some(id) => id,
                                    None => {
                                        warn!(
                                            "Transfer {} has no file_id, skipping",
                                            transfer.name
                                        );
                                        continue;
                                    }
                                };

                                info!(
                                    "Starting automatic download: {} (file_id: {})",
                                    transfer.name, file_id
                                );

                                // Get file details from Put.io
                                let file_details = match client.get_file_details(file_id).await {
                                    Ok(details) => details,
                                    Err(e) => {
                                        error!("Failed to get file details for {}: {}", file_id, e);
                                        continue;
                                    }
                                };

                                let file_info = file_details.file;

                                // Create download file struct
                                let download_file = DownloadFile {
                                    file_id: file_info.id,
                                    filename: file_info.name.clone(),
                                    file_size: file_info.size as u64,
                                    download_url: format!(
                                        "https://api.put.io/v2/files/{}/download",
                                        file_info.id
                                    ),
                                    relative_path: String::new(),
                                    crc32: file_info.crc32.clone(),
                                    save_as: None,
                                    fixed_path: false,
                                    overwrite: false,
                                };

                                // Spawn download task
                                let download_manager_clone = download_manager.clone();
                                let filename = download_file.filename.clone();

                                tokio::spawn(async move {
                                    match download_manager_clone
                                        .download_file(download_file, None)
                                        .await
                                    {
                                        Ok(result) => {
                                            if result.success {
                                                info!(
                                                    "Daemon-initiated download completed: {}",
                                                    filename
                                                );
                                            } else {
                                                error!(
                                                    "Daemon-initiated download failed: {} - {:?}",
                                                    filename, result.error
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            error!(
                                                "Daemon-initiated download error: {} - {}",
                                                filename, e
                                            );
                                        }
                                    }
                                });

                                info!("Download task spawned for: {}", transfer.name);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Download daemon: Failed to list transfers: {}", e);
                    }
                }

                // Sleep for interval (60 seconds)
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            }
        })
    })
}

/// Every 30 s, the shared transfer list (which sends `transfer.*`) while a hook or a
/// plugin wants a transfer event; every 15 minutes, Put.io storage (which sends `storage.low`)
async fn hook_events_ticker(state: putio_sync_api::AppState) {
    use putio_sync_core::events::EventType;
    const TRANSFERS: Duration = Duration::from_secs(30);
    const STORAGE: Duration = Duration::from_secs(15 * 60);
    let mut ticker = tokio::time::interval(TRANSFERS);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut storage_at: Option<tokio::time::Instant> = None;
    loop {
        ticker.tick().await;
        let transfers = [
            EventType::TransferAdded,
            EventType::TransferFinished,
            EventType::TransferFailed,
        ];
        if transfers
            .into_iter()
            .any(|t| state.hooks.wants(t) || state.plugins.wants(t))
        {
            if let Err(e) = state.download_manager.transfers_snapshot(TRANSFERS).await {
                debug!("Could not list Put.io transfers: {}", e);
            }
        }
        if storage_at.is_none_or(|at| at.elapsed() >= STORAGE) {
            storage_at = Some(tokio::time::Instant::now());
            if let Err(e) = state.storage.refresh().await {
                debug!("Could not read Put.io storage: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_command_signature() {
        // Just ensure the function signature compiles
        let _f: fn(bool, bool, Option<u16>, Option<String>, Option<PathBuf>) -> _ = execute_server;
    }

    fn listen(config: &Config, bind: Option<&str>, port: Option<u16>) -> Vec<String> {
        server_listen(config, bind, port)
            .unwrap()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn server_flags_merge_with_the_config() {
        use putio_sync_core::listen::BindList;
        let mut config = Config::default();
        config.api.bind = Some(BindList::from_entries(vec![
            "127.0.0.1:8080".into(),
            "[::1]:8080".into(),
        ]));
        assert_eq!(
            listen(&config, None, None),
            ["127.0.0.1:8080", "[::1]:8080"]
        );
        assert_eq!(listen(&config, Some("0.0.0.0"), None), ["0.0.0.0:8080"]);
        assert_eq!(listen(&config, Some("::"), Some(9000)), ["[::]:9000"]);
        assert_eq!(listen(&config, Some("lo0"), None), ["lo0:8080"]);
        assert_eq!(
            listen(&config, Some("tailnet"), Some(9000)),
            ["tailnet:9000"]
        );
        assert_eq!(
            listen(&config, Some("10.0.0.2:7000"), None),
            ["10.0.0.2:7000"]
        );
        assert_eq!(
            listen(&config, None, Some(9000)),
            ["127.0.0.1:9000", "[::1]:9000"]
        );
        let e = server_listen(&config, Some("10.0.0.2:7000"), Some(9000)).unwrap_err();
        assert!(e.to_string().contains("give the port once"), "{e}");
        assert!(server_listen(&config, Some("not an address"), None).is_err());

        // Tailnet only: `server` honours it too
        config.tailscale.tailnet_only = true;
        assert_eq!(listen(&config, None, None), ["tailnet:8080"]);
        assert_eq!(listen(&config, None, Some(9000)), ["tailnet:9000"]);
        // An explicit --bind takes its place
        assert_eq!(listen(&config, Some("127.0.0.1"), None), ["127.0.0.1:8080"]);
    }
}
