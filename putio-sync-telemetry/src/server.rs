//! The metrics listener: one `GET <path>` route that returns the text exposition.

use std::io;
use std::net::SocketAddr;

use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::{Detail, Telemetry};

/// Content type of the Prometheus text exposition format.
pub const CONTENT_TYPE_TEXT: &str = "text/plain; version=0.0.4; charset=utf-8";

/// Resolved `[metrics]` settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsSettings {
    /// Whether the listener starts.
    pub enabled: bool,
    /// Listen address.
    pub bind: SocketAddr,
    /// Scrape path, starting with `/`.
    pub path: String,
    /// Output detail level.
    pub detail: Detail,
}

impl Default for MetricsSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: SocketAddr::from(([0, 0, 0, 0], 9742)),
            path: "/metrics".to_string(),
            detail: Detail::Basic,
        }
    }
}

/// Bind the listener. A failure (such as the port being in use) is returned, not a panic.
pub async fn bind(settings: &MetricsSettings) -> io::Result<TcpListener> {
    TcpListener::bind(settings.bind).await
}

/// Serve `GET <path>` on `listener` until the task is aborted. Every other path is a 404.
pub fn serve(listener: TcpListener, path: &str, telemetry: &'static Telemetry) -> JoinHandle<()> {
    let app = Router::new().route(
        path,
        get(move || async move {
            ([(CONTENT_TYPE, CONTENT_TYPE_TEXT)], telemetry.encode()).into_response()
        }),
    );
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!(error = %e, "Metrics listener stopped");
        }
    })
}
