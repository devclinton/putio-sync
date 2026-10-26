//! Server command implementation
//!
//! Starts an HTTP API server with optional daemon modes for watch and download.

use anyhow::{anyhow, Context, Result};
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::{AuthMethod, Config};
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadFile, DownloadManager};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
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
/// * `port` - Port to bind the HTTP server
/// * `bind` - Address to bind the HTTP server
/// * `config_path` - Optional custom config file path
///
/// # Examples
///
/// ```no_run
/// # use putio_sync_cli::commands::execute_server;
/// # async fn example() {
/// // Start server only
/// execute_server(false, false, 15432, "0.0.0.0".to_string(), None).await.unwrap();
///
/// // Start server with watch and download modes
/// execute_server(true, true, 15432, "0.0.0.0".to_string(), None).await.unwrap();
/// # }
/// ```
pub async fn execute_server(
    watch: bool,
    download: bool,
    port: u16,
    bind: String,
    config_path: Option<PathBuf>,
) -> Result<()> {
    info!("Starting putio-sync server");

    // Load configuration
    let config_file_path = config_path
        .or_else(|| Config::default_path().ok())
        .context("Could not determine config path")?;

    let config = if config_file_path.exists() {
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
            },
        );
    }

    // Prepare server address
    let addr = format!("{}:{}", bind, port);
    info!("HTTP API server will bind to: {}", addr);

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
    let app = putio_sync_api::create_app(
        config.clone(),
        database_path,
        Some(config_file_path.clone()),
        None, // No file watcher in standalone server mode
    )
    .await
    .map_err(|e| anyhow!("Failed to create API app: {}", e))?;

    // Start HTTP server
    info!("Starting HTTP server on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .context(format!("Failed to bind to {}", addr))?;

    info!("✓ HTTP API server started successfully");
    info!("API endpoints available at:");
    info!("  - Health:    http://{}/health", addr);
    info!("  - Put.io:    http://{}/putio/{{files,transfers}}", addr);
    info!(
        "  - Local:     http://{}/local/{{files,queue,downloads,watcher,workers}}",
        addr
    );
    info!("  - System:    http://{}/{{settings,stats,history}}", addr);
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

    // Run server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Server error")?;

    info!("Server shut down gracefully");

    // Note: Daemon threads will continue running until Ctrl+C is pressed again
    // The TorrentWatcher has its own Ctrl+C handler that will stop it gracefully
    // For now, we don't wait for the daemon threads to finish to allow quick server shutdown
    // In the future, we could implement a shared shutdown signal for all threads

    Ok(())
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
            let download_config = DownloadConfig {
                target_directory: config.download.target_directory.clone(),
                skip_existing: config.download.skip_existing,
                buffer_size_mb: config.download.buffer_size_mb,
                bandwidth_limit_bytes_per_sec: config.download.bandwidth_limit_bytes_per_sec,
                max_retries: config.download.max_retries,
                retry_initial_delay_secs: config.download.retry_initial_delay_secs,
                workers: config.download.workers,
                delete_after_download: config.download.delete_after_download,
                blocked_extensions: config.download.blocked_extensions.clone(),
            };

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_command_signature() {
        // Just ensure the function signature compiles
        let _f: fn(bool, bool, u16, String, Option<PathBuf>) -> _ = execute_server;
    }
}
