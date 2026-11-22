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

/// What to warn about when the metrics listener is on `ip`: it has no authentication,
/// so anything that can reach a non-loopback address can read the counters (file names
/// are not in them at the Basic detail level, but paths, categories and tracker hosts
/// can be at higher ones). `None` on loopback.
pub fn exposure_warning(ip: std::net::IpAddr, path: &str) -> Option<String> {
    if ip.to_canonical().is_loopback() {
        return None;
    }
    Some(format!(
        "Metrics at {path} are served without authentication on a non-loopback address; \
         restrict who can reach this port (firewall, a private Docker network), or set \
         [metrics] bind to 127.0.0.1 and scrape through a proxy that adds a token"
    ))
}

/// Serve [`router`] on `listener` until the task is aborted.
pub fn serve(listener: TcpListener, path: &str, telemetry: &'static Telemetry) -> JoinHandle<()> {
    let app = router(path, telemetry);
    if let Ok(addr) = listener.local_addr() {
        if let Some(warning) = exposure_warning(addr.ip(), path) {
            tracing::warn!(%addr, "{warning}");
        }
    }
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!(error = %e, "Metrics listener stopped");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::exposure_warning;

    #[test]
    fn only_non_loopback_binds_warn() {
        assert!(exposure_warning("127.0.0.1".parse().unwrap(), "/metrics").is_none());
        assert!(exposure_warning("::1".parse().unwrap(), "/metrics").is_none());
        assert!(exposure_warning("0.0.0.0".parse().unwrap(), "/metrics").is_some());
        assert!(exposure_warning("192.168.1.5".parse().unwrap(), "/metrics").is_some());
    }
}
