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
    let handle = Supervisor::spawn(
        "Metrics",
        entries,
        tailscale,
        server::router(&settings.path, telemetry),
        Bound::default(),
    );
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
