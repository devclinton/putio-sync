//! Starts the Prometheus metrics listener and gauge refresh. Shared by `daemon` and
//! `server` modes so both expose the same endpoint (FR-007).

use putio_sync_api::listen::{Bound, Supervisor};
use putio_sync_core::config::Config;
use putio_sync_core::error::CoreError;
use putio_sync_db::Database;
use putio_sync_tailscale::{Cli, TailscaleService};
use putio_sync_telemetry::{gauges, global, server, QueueState};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tracing::info;

/// Warnings for bound addresses that serve the metrics without authentication on a
/// non-loopback address, one per address not in `seen` yet (which it then records).
fn exposure_warnings(
    bound: &[SocketAddr],
    path: &str,
    seen: &mut std::collections::HashSet<SocketAddr>,
) -> Vec<(SocketAddr, String)> {
    bound
        .iter()
        .filter(|a| seen.insert(**a))
        .filter_map(|a| server::exposure_warning(a.ip(), path).map(|w| (*a, w)))
        .collect()
}

/// Log the exposure warning for every address the listener binds, including ones that
/// bind later (an interface that comes up after start).
fn spawn_exposure_warnings(bound: Bound, path: String) {
    tokio::spawn(async move {
        let mut seen = std::collections::HashSet::new();
        loop {
            for (addr, warning) in exposure_warnings(&bound.get(), &path, &mut seen) {
                tracing::warn!(%addr, "{warning}");
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

/// Start metrics for this process.
///
/// `queue_db` can be set by the caller at any time (or never); until then the queue
/// gauges stay at zero. Returns the first bound address when one bound within 2 s.
/// `[metrics] bind` is a list like `[api] bind` (feature 037): an address that can't be
/// bound is logged and retried in the background (FR-008); an invalid setting is an error.
pub async fn start(
    config: &Config,
    queue_db: Arc<OnceLock<Arc<Database>>>,
    workers: usize,
    download_dir: PathBuf,
) -> Result<Option<SocketAddr>, CoreError> {
    let resolved = config.resolve_metrics()?;
    let settings = resolved.value.settings;
    let entries = resolved.value.bind;
    let source = resolved.source;

    if !settings.enabled {
        info!(source = ?source, "Metrics disabled");
        return Ok(None);
    }

    let telemetry = global();
    telemetry.set_detail(settings.detail);
    telemetry.init(
        workers,
        env!("CARGO_PKG_VERSION"),
        option_env!("PUTIO_SYNC_GIT_SHA").unwrap_or("unknown"),
    );

    let shown: Vec<String> = entries.iter().map(ToString::to_string).collect();
    let tailscale = Arc::new(TailscaleService::new(
        config.tailscale.socket_path(),
        Cli::Auto,
    ));
    let bound = Bound::default();
    let handle = Supervisor::spawn(
        "Metrics",
        entries,
        tailscale,
        server::router(&settings.path, telemetry),
        bound.clone(),
    );
    spawn_exposure_warnings(bound, settings.path.clone());
    let addr = handle.first_bound(Duration::from_secs(2)).await;
    info!(
        bind = %shown.join(", "),
        path = %settings.path,
        detail = %settings.detail,
        source = ?source,
        "Metrics enabled"
    );

    gauges::spawn_refresh(telemetry, download_dir, move || {
        let db = queue_db.get().cloned();
        async move {
            let counts = db?.queue().counts_by_status().await.ok()?;
            Some(
                counts
                    .into_iter()
                    .filter_map(|(status, n)| QueueState::from_status(&status).map(|s| (s, n)))
                    .collect(),
            )
        }
    });

    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_once_per_non_loopback_address() {
        let mut seen = std::collections::HashSet::new();
        let all: Vec<SocketAddr> = vec![
            "127.0.0.1:9742".parse().unwrap(),
            "0.0.0.0:9742".parse().unwrap(),
        ];
        let first = exposure_warnings(&all, "/metrics", &mut seen);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].0, all[1]);
        assert!(exposure_warnings(&all, "/metrics", &mut seen).is_empty());
    }
}
