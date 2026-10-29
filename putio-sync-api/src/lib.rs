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
//!     let mut config = Config::default();
//!     config.api.auth_token = Some("change-me".to_string());
//!     let app = create_app(config, None, None, None).await.unwrap();
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
pub mod ui;

use axum::Router;
use putio_sync_client::PutioApiClient;
use putio_sync_core::cleanup::EmptyFolderCleanup;
use putio_sync_core::config::Config;
use putio_sync_core::stall::StallTracker;
use putio_sync_db::Database;
use putio_sync_doctor::Doctor;
use putio_sync_download::{DownloadConfig, DownloadManager};
use putio_sync_file_manager::TorrentWatcher;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;

/// Extract DownloadConfig from Config (the shared `From<&[download]>` conversion)
pub fn extract_download_config(config: &Config) -> DownloadConfig {
    DownloadConfig::from(&config.download)
}

/// Global application state
///
/// This state is shared across all route handlers and provides access to
/// all core services: database, download manager, API client, and configuration.
///
/// Note: `PUT /settings/logging` only saves the file; the config poller started by
/// `daemon`/`server` applies `level` and `module_filters` from it. Other logging
/// settings need a restart.
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
    /// Download manager (uses internal RwLock for hot reload)
    pub download_manager: Arc<DownloadManager>,
    /// File watcher (optional, for hot reload)
    pub file_watcher: Option<Arc<TorrentWatcher>>,
    /// When the state was built, for uptime
    pub started_at: Instant,
    /// Failed-authentication limiter shared by the middleware and `/auth/login`
    pub limiter: Arc<middleware::ratelimit::AuthLimiter>,
    /// The engine the API runs on, which decides what a settings save reaches live
    pub engine: Engine,
    /// Whether a config reload poller applies `[logging]` level changes from the file.
    /// Both commands start one exactly when the config file exists at startup.
    pub logging_reload: bool,
    /// Empty-folder cleanup; under `daemon`, the same service runs the daily cleanup
    pub cleanup: Arc<EmptyFolderCleanup>,
    /// Stalled-transfer verdicts (feature 007). Empty unless the daemon's tracker is
    /// swapped in with [`AppState::with_stall_tracker`].
    pub stall_tracker: Arc<StallTracker>,
    /// The Put.io doctor (feature 010); under `daemon`, the one its schedule runs
    pub doctor: Arc<Doctor>,
    /// Radarr/Sonarr download client (feature 011); `None` without a database. It's built
    /// even when `[arr]` is off, so turning it on from Settings needs no restart
    /// (feature 018); the RPC checks `enabled` per request.
    pub arr: Option<routes::arr::ArrRpc>,
}

/// The engine behind the API (FR-018: a save says what applies now)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// `daemon`: the API shares the daemon's download manager and watcher
    Daemon,
    /// `server`: the API has its own download manager; the `--watch`/`--download`
    /// threads keep their startup config
    Server,
}

impl AppState {
    /// Build the state from an engine the caller already runs (the daemon) or has just
    /// built (`create_app`).
    ///
    /// `config.api.auth_token` must already hold the resolved token (env override
    /// applied); the auth middleware reads it from here.
    pub fn new(
        config: Config,
        config_path: Option<PathBuf>,
        client: Arc<PutioApiClient>,
        database: Option<Arc<Database>>,
        download_manager: Arc<DownloadManager>,
        file_watcher: Option<Arc<TorrentWatcher>>,
    ) -> AppState {
        let logging_reload = config_path.as_ref().is_some_and(|path| path.exists());
        let cleanup = Arc::new(EmptyFolderCleanup::from_config(client.clone(), &config));
        let doctor = Arc::new(
            Doctor::new(
                config.clone(),
                client.clone(),
                database.clone(),
                cleanup.clone(),
            )
            .with_config_path(config_path.clone())
            .with_metrics(),
        );
        let arr = build_arr(&config, &client, database.as_ref(), &download_manager);
        AppState {
            arr,
            cleanup,
            doctor,
            engine: Engine::Server,
            logging_reload,
            config: Arc::new(RwLock::new(config)),
            config_path: config_path.map(Arc::new),
            client,
            database,
            download_manager,
            file_watcher,
            started_at: Instant::now(),
            stall_tracker: Arc::new(StallTracker::default()),
            limiter: Arc::new(middleware::ratelimit::AuthLimiter::new(
                middleware::ratelimit::AUTH_FAILURE_LIMIT,
                middleware::ratelimit::AUTH_FAILURE_WINDOW,
            )),
        }
    }

    /// Read stall verdicts from `tracker`, the one the daemon's stall check task feeds
    pub fn with_stall_tracker(mut self, tracker: Arc<StallTracker>) -> AppState {
        self.stall_tracker = tracker;
        self
    }

    /// Mark the state as running on the daemon's own engine (`api_listener::start`)
    pub fn on_daemon(mut self) -> AppState {
        self.engine = Engine::Daemon;
        self
    }

    /// Share `cleanup` with the daemon's daily run, so a manual run and the daily one
    /// never overlap
    pub fn with_cleanup(mut self, cleanup: Arc<EmptyFolderCleanup>) -> AppState {
        self.cleanup = cleanup;
        self
    }

    /// Share `doctor` with the daemon's schedule, so the page shows its latest run
    pub fn with_doctor(mut self, doctor: Arc<Doctor>) -> AppState {
        self.doctor = doctor;
        self
    }
}

/// The download client; it needs the database to track items. Off settings when `[arr]`
/// is invalid, so the rest of the API still starts.
fn build_arr(
    config: &Config,
    client: &Arc<PutioApiClient>,
    database: Option<&Arc<Database>>,
    download_manager: &Arc<DownloadManager>,
) -> Option<routes::arr::ArrRpc> {
    let Some(database) = database else {
        if config.arr.enabled {
            tracing::warn!("Radarr/Sonarr download client disabled: it needs the database");
        }
        return None;
    };
    let settings = config.resolve_arr().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Radarr/Sonarr download client disabled");
        putio_sync_core::config::ArrSettings {
            enabled: false,
            username: config.arr.username.clone(),
            password: None,
            servers: Vec::new(),
        }
    });
    let service = putio_sync_arr::ArrService::new(
        client.clone(),
        database.clone(),
        download_manager.clone(),
        settings,
        config.download.target_directory.clone(),
    );
    Some(routes::arr::ArrRpc::new(Arc::new(service)))
}

/// Build the router for `state`: protected routes behind the auth middleware, plus the
/// public routes, which never require a token.
///
/// The public routes are the base the protected ones merge into, so the merged router
/// keeps the protected fallback: an unknown path needs a token too (401, not 404).
pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .nest("/putio/files", create_putio_files_router())
        .nest("/putio/transfers", create_putio_transfers_router())
        .nest("/putio/cleanup", routes::putio::cleanup::router())
        .nest("/local/files", create_local_files_router())
        .nest("/local/queue", create_local_queue_router())
        .route(
            "/local/sync",
            axum::routing::post(routes::local::sync::sync_now),
        )
        .nest("/local/downloads", create_local_downloads_router())
        .nest("/local/watcher", create_local_watcher_router())
        .nest("/local/workers", create_local_workers_router())
        .nest("/settings", create_settings_router())
        .nest("/rules", routes::system::rules::router())
        .nest("/stats", create_stats_router())
        .nest("/history", create_history_router())
        .nest("/doctor", routes::system::doctor::router())
        .nest("/arr", routes::arr::router())
        .route(
            "/status",
            axum::routing::get(routes::system::status::get_status),
        )
        // Apply authentication middleware to all routes above and to the fallback
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::auth::require_auth,
        ))
        .with_state(state.clone());

    public_routes(state)
        .merge(protected)
        .layer(TraceLayer::new_for_http())
}

/// Routes that answer without a token: health, sign-in, and the web UI page and its
/// assets
fn public_routes(state: AppState) -> Router {
    Router::new()
        .route("/health", axum::routing::get(health))
        .route(
            "/auth/status",
            axum::routing::get(routes::system::auth::auth_status),
        )
        .route(
            "/auth/login",
            axum::routing::post(routes::system::auth::login),
        )
        .route("/", axum::routing::get(ui::index))
        .route("/assets/*path", axum::routing::get(ui::asset))
        // Radarr/Sonarr's Transmission client; Basic auth of its own (feature 011)
        .route(
            "/transmission/rpc",
            axum::routing::any(routes::arr::transmission_rpc),
        )
        .with_state(state)
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
///     let app = create_app(config, db_path, config_path, None).await.unwrap();
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
    file_watcher: Option<Arc<TorrentWatcher>>,
) -> Result<Router, Box<dyn std::error::Error>> {
    use putio_sync_core::config::AuthMethod;
    use tracing::{info, warn};

    // Get auth config and create API client
    let auth = config.auth.as_ref().ok_or("No authentication configured")?;

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
                warn!(
                    "Failed to initialize database at {}: {}",
                    db_path.display(),
                    e
                );
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
    // Before anything can claim a row: a crash leaves rows in `downloading`
    if let Err(e) = download_manager.recover_interrupted().await {
        warn!("{}", e);
    }

    let state = AppState::new(
        config,
        config_path,
        client,
        database,
        Arc::new(download_manager),
        file_watcher,
    );
    spawn_arr_refresh(&state).await;
    Ok(router(state))
}

/// Read each saved Radarr/Sonarr's categories now, then every 15 minutes (feature 018, R4)
pub async fn spawn_arr_refresh(state: &AppState) {
    let Some(arr) = &state.arr else {
        return;
    };
    let port = state
        .config
        .read()
        .await
        .resolve_api()
        .map(|r| r.value.bind.port())
        .unwrap_or(15432);
    let hosts = putio_sync_arr::setup::candidate_hosts(None, None);
    arr.service.spawn_category_refresh(port, hosts);
}

// New route constructors with proper state types
fn create_putio_files_router() -> Router<AppState> {
    routes::putio::files::router()
}

fn create_putio_transfers_router() -> Router<AppState> {
    routes::putio::transfers::router()
}

fn create_local_files_router() -> Router<AppState> {
    routes::local::files::router()
}

fn create_local_queue_router() -> Router<AppState> {
    routes::local::queue::router()
}

fn create_local_downloads_router() -> Router<AppState> {
    routes::local::downloads::router()
}

fn create_local_watcher_router() -> Router<AppState> {
    routes::local::watcher::router()
}

fn create_local_workers_router() -> Router<AppState> {
    routes::local::workers::router()
}

fn create_settings_router() -> Router<AppState> {
    routes::system::settings::router()
}

fn create_stats_router() -> Router<AppState> {
    routes::system::stats::router()
}

fn create_history_router() -> Router<AppState> {
    routes::system::history::router()
}

/// Unauthenticated liveness check
///
/// GET /health
async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use putio_sync_core::config::AuthConfig;

    #[tokio::test]
    async fn test_create_app() {
        // Set up minimal auth config
        let config = Config {
            auth: Some(AuthConfig::with_token("test_token".to_string())),
            ..Default::default()
        };

        let _app = create_app(config, None, None, None).await;
        // Test that app can be created without database, config path, or file watcher
    }
}
