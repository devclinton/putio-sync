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
use putio_sync_core::config::Config;
use routes::files::AppState;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

/// Create the main application router
///
/// This function creates and configures the main Axum router with all
/// API routes and middleware.
///
/// # Arguments
///
/// * `config` - Application configuration
///
/// # Examples
///
/// ```no_run
/// use putio_sync_api::create_app;
/// use putio_sync_core::config::Config;
///
/// #[tokio::main]
/// async fn main() {
///     let config = Config::default();
///     let app = create_app(config);
///     let listener = tokio::net::TcpListener::bind("0.0.0.0:15432")
///         .await
///         .unwrap();
///     axum::serve(listener, app).await.unwrap();
/// }
/// ```
pub fn create_app(config: Config) -> Router {
    let state = AppState {
        config: Arc::new(config),
    };

    Router::new()
        .nest("/api/files", routes::files::router())
        .nest("/api/local", routes::local::router())
        .nest("/api/transfers", routes::transfers::router())
        .nest("/api/queue", routes::queue::router())
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_app() {
        let config = Config::default();
        let _app = create_app(config);
        // Just ensure the app can be created without panicking
    }
}
