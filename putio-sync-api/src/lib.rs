//! # putio-sync-api
//!
//! REST API for putio-sync, providing HTTP endpoints for managing Put.io files,
//! transfers, and local downloads.
//!
//! ## Features
//!
//! - **Files API**: List, get, and delete files from Put.io
//! - **Local Files API**: Manage files in the local sync directory
//! - **Transfers API**: Manage torrent transfers
//! - **Queue API**: Manage download queue
//!
//! ## Usage
//!
//! ```no_run
//! use putio_sync_api::create_app;
//! use putio_sync_core::config::Config;
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = Config::default();
//!     let app = create_app(config);
//!     let listener = tokio::net::TcpListener::bind("0.0.0.0:15432")
//!         .await
//!         .unwrap();
//!     axum::serve(listener, app).await.unwrap();
//! }
//! ```

pub mod error;
pub mod handlers;
pub mod middleware;
pub mod models;
pub mod routes;

use axum::Router;
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::Config;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// Extract DownloadConfig from Config
pub fn extract_download_config(config: &Config) -> DownloadConfig {
    DownloadConfig {
        target_directory: config.download.target_directory.clone(),
        skip_existing: config.download.skip_existing,
        buffer_size_mb: config.download.buffer_size_mb,
        bandwidth_limit_bytes_per_sec: config.download.bandwidth_limit_bytes_per_sec,
        max_retries: config.download.max_retries,
        retry_initial_delay_secs: config.download.retry_initial_delay_secs,
        workers: config.download.workers,
        delete_after_download: config.download.delete_after_download,
    }
}

/// Global application state
///
/// This state is shared across all route handlers and provides access to
/// all core services: database, download manager, API client, and configuration.
#[derive(Clone)]
pub struct AppState {
    /// Application configuration (wrapped in RwLock for settings updates)
    pub config: Arc<RwLock<Config>>,
    /// Path to config file for hot reload and persistence
    pub config_path: Option<Arc<std::path::PathBuf>>,
    /// Put.io API client
    pub client: Arc<PutioApiClient>,
    /// Database connection (optional)
    pub database: Option<Arc<Database>>,
    /// Download manager
    pub download_manager: Arc<RwLock<DownloadManager>>,
}

/// Create the main application router
///
/// This function creates and configures the main Axum router with all
/// API routes and middleware.
///
/// # Arguments
///
/// * `config` - Application configuration
/// * `database_path` - Optional path to SQLite database file
/// * `config_path` - Optional path to config file for hot reload and persistence
///
/// # Examples
///
/// ```no_run
/// use putio_sync_api::create_app;
/// use putio_sync_core::config::Config;
/// use std::path::PathBuf;
///
/// #[tokio::main]
/// async fn main() {
///     let config = Config::default();
///     let db_path = Some(PathBuf::from("putio-sync.db"));
///     let config_path = Some(PathBuf::from("config.toml"));
///     let app = create_app(config, db_path, config_path).await.unwrap();
///     let listener = tokio::net::TcpListener::bind("0.0.0.0:15432")
///         .await
///         .unwrap();
///     axum::serve(listener, app).await.unwrap();
/// }
/// ```
pub async fn create_app(
    config: Config,
    database_path: Option<std::path::PathBuf>,
    config_path: Option<std::path::PathBuf>,
) -> Result<Router, Box<dyn std::error::Error>> {
    use putio_sync_core::config::AuthMethod;
    use tracing::{info, warn};

    // Get auth config and create API client
    let auth = config
        .auth
        .as_ref()
        .ok_or("No authentication configured")?;

    let client = match auth.auth_method {
        AuthMethod::Token => {
            let token = auth.token.as_ref().ok_or("Token not configured")?;
            Arc::new(PutioApiClient::new(token.clone()))
        }
        AuthMethod::AppPassword => {
            let username = auth.username.as_ref().ok_or("Username not configured")?;
            let password = auth.password.as_ref().ok_or("Password not configured")?;
            Arc::new(PutioApiClient::with_credentials(
                username.clone(),
                password.clone(),
            ))
        }
    };

    // Initialize database (if path provided)
    let database = if let Some(ref db_path) = database_path {
        match Database::new(db_path).await {
            Ok(db) => {
                info!("Database initialized: {}", db_path.display());
                Some(Arc::new(db))
            }
            Err(e) => {
                warn!("Failed to initialize database at {}: {}", db_path.display(), e);
                warn!("Continuing without database (download tracking disabled)");
                None
            }
        }
    } else {
        info!("No database path provided, running without persistence");
        None
    };

    // Initialize download manager
    let download_config = extract_download_config(&config);

    let download_manager = if let Some(ref db) = database {
        DownloadManager::new(client.clone(), db.clone(), download_config)
    } else {
        DownloadManager::new_without_database(client.clone(), download_config)
    };

    let state = AppState {
        config: Arc::new(RwLock::new(config)),
        config_path: config_path.map(Arc::new),
        client,
        database,
        download_manager: Arc::new(RwLock::new(download_manager)),
    };

    Ok(Router::new()
        // New organized routes
        .nest("/putio/files", create_putio_files_router())
        .nest("/putio/transfers", create_putio_transfers_router())
        .nest("/local/files", create_local_files_router())
        .nest("/local/queue", create_local_queue_router())
        .nest("/local/downloads", create_local_downloads_router())
        .nest("/local/watcher", create_local_watcher_router())
        .nest("/settings", create_settings_router())
        .nest("/stats", create_stats_router())
        .nest("/history", create_history_router())
        // Legacy routes (for backward compatibility)
        .nest("/api/files", create_legacy_files_router())
        .nest("/api/transfers", create_legacy_transfers_router())
        .nest("/api/queue", create_legacy_queue_router())
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http()))
}

// New route constructors with proper state types
fn create_putio_files_router() -> Router<AppState> {
    
    // Map AppState to route-specific state
    Router::new()
        .route("/", axum::routing::get(files_placeholder))
        .route("/:id", axum::routing::get(files_placeholder))
        .route("/:id", axum::routing::delete(files_placeholder))
        .route("/:id/download", axum::routing::get(files_placeholder))
}

fn create_putio_transfers_router() -> Router<AppState> {
    Router::new()
        .route("/", axum::routing::get(transfers_placeholder))
        .route("/", axum::routing::post(transfers_placeholder))
}

fn create_local_files_router() -> Router<AppState> {
    Router::new()
        .route("/", axum::routing::get(local_files_placeholder))
}

fn create_local_queue_router() -> Router<AppState> {
    Router::new()
        .route("/", axum::routing::get(queue_placeholder))
}

fn create_local_downloads_router() -> Router<AppState> {
    Router::new()
        .route("/", axum::routing::get(downloads_placeholder))
}

fn create_local_watcher_router() -> Router<AppState> {
    routes::local::watcher::router()
}

fn create_settings_router() -> Router<AppState> {
    routes::system::settings::router()
}

fn create_stats_router() -> Router<AppState> {
    Router::new()
        .route("/downloads", axum::routing::get(stats_placeholder))
}

fn create_history_router() -> Router<AppState> {
    Router::new()
        .route("/", axum::routing::get(history_placeholder))
}

fn create_legacy_files_router() -> Router<AppState> {
    // Legacy routes deprecated - use /putio/files and /local/files instead
    Router::new()
        .route("/", axum::routing::get(legacy_deprecated_placeholder))
}

fn create_legacy_transfers_router() -> Router<AppState> {
    // Legacy routes deprecated - use /putio/transfers instead
    Router::new()
        .route("/", axum::routing::get(legacy_deprecated_placeholder))
}

fn create_legacy_queue_router() -> Router<AppState> {
    // Legacy routes deprecated - use /local/queue instead
    Router::new()
        .route("/", axum::routing::get(legacy_deprecated_placeholder))
}

async fn legacy_deprecated_placeholder() -> &'static str {
    "Legacy API deprecated - please use /putio/* and /local/* endpoints"
}

// Placeholder handlers (will be replaced with actual implementations)
async fn files_placeholder() -> &'static str {
    "Files endpoint - implementation pending"
}

async fn transfers_placeholder() -> &'static str {
    "Transfers endpoint - implementation pending"
}

async fn local_files_placeholder() -> &'static str {
    "Local files endpoint - implementation pending"
}

async fn queue_placeholder() -> &'static str {
    "Queue endpoint - implementation pending"
}

async fn downloads_placeholder() -> &'static str {
    "Downloads endpoint - implementation pending"
}

async fn stats_placeholder() -> &'static str {
    "Stats endpoint - implementation pending"
}

async fn history_placeholder() -> &'static str {
    "History endpoint - implementation pending"
}

#[cfg(test)]
mod tests {
    use super::*;
    use putio_sync_core::config::AuthConfig;

    #[tokio::test]
    async fn test_create_app() {
        let mut config = Config::default();
        // Set up minimal auth config
        config.auth = Some(AuthConfig::with_token("test_token".to_string()));

        let _app = create_app(config, None, None).await;
        // Test that app can be created without database or config path
    }
}
