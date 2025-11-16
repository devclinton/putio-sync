//! Server command implementation
//!
//! Starts an HTTP API server with optional daemon modes for watch and download.

use anyhow::{Context, Result};
use putio_sync_core::config::Config;
use std::path::PathBuf;
use tokio::signal;
use tracing::{info, warn};

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
    config.validate().context("Configuration validation failed")?;

    info!("Configuration loaded successfully");

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

    // Create application with config
    let app = putio_sync_api::create_app(config.clone());

    // Start HTTP server
    info!("Starting HTTP server on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .context(format!("Failed to bind to {}", addr))?;

    info!("✓ HTTP API server started successfully");
    info!("API endpoints available at:");
    info!("  - Files:     http://{}/api/files", addr);
    info!("  - Local:     http://{}/api/local", addr);
    info!("  - Transfers: http://{}/api/transfers", addr);
    info!("  - Queue:     http://{}/api/queue", addr);
    info!("");
    info!("Press Ctrl+C to stop the server");

    // TODO: Start watch mode thread if enabled
    if watch {
        // Spawn watch mode task
        info!("TODO: Start watch mode thread");
    }

    // TODO: Start download mode thread if enabled
    if download {
        // Spawn download mode task
        info!("TODO: Start download mode thread");
    }

    // Run server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Server error")?;

    info!("Server shut down gracefully");

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_command_signature() {
        // Just ensure the function signature compiles
        let _f: fn(bool, bool, u16, String, Option<PathBuf>) -> _ = execute_server;
    }
}
