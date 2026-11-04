//! The metrics listener: one `GET <path>` route that returns the text exposition.

use axum::http::header::CONTENT_TYPE;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use crate::{Detail, Telemetry};

/// Content type of the Prometheus text exposition format.
pub const CONTENT_TYPE_TEXT: &str = "text/plain; version=0.0.4; charset=utf-8";

/// Resolved `[metrics]` settings. Where it listens is the config's business
/// (`Config::resolve_metrics`), since an entry can name an interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricsSettings {
    /// Whether the listener starts.
    pub enabled: bool,
    /// Scrape path, starting with `/`.
    pub path: String,
    /// Output detail level.
    pub detail: Detail,
}

impl Default for MetricsSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            path: "/metrics".to_string(),
            detail: Detail::Basic,
        }
    }
}

/// `GET <path>` returning the text exposition. Every other path is a 404.
pub fn router(path: &str, telemetry: &'static Telemetry) -> Router {
    Router::new().route(
        path,
        get(move || async move {
            ([(CONTENT_TYPE, CONTENT_TYPE_TEXT)], telemetry.encode()).into_response()
        }),
    )
}

/// Serve [`router`] on `listener` until the task is aborted.
pub fn serve(listener: TcpListener, path: &str, telemetry: &'static Telemetry) -> JoinHandle<()> {
    let app = router(path, telemetry);
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!(error = %e, "Metrics listener stopped");
        }
    })
}
