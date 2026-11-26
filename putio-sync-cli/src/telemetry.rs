//! Starts the Prometheus metrics listener and gauge refresh. Shared by `daemon` and
//! `server` modes so both expose the same endpoint (FR-007).

use putio_sync_api::listen::{Bound, ListenerHandle, Supervisor};
use putio_sync_core::config::{Config, MetricsListen};
use putio_sync_core::error::CoreError;
use putio_sync_core::listen::ListenAddr;
use putio_sync_db::Database;
use putio_sync_tailscale::{Cli, TailscaleService};
use putio_sync_telemetry::server::MetricsSettings;
use putio_sync_telemetry::{gauges, global, server, QueueState};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tracing::{info, warn};

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

/// The metrics listener, followed while the daemon runs (hot reload, part 3): a
/// published config turns it on or off, moves it to a new `[metrics] bind`, or serves a
/// new `path` and `detail`. `server` mode starts it once and never applies anything.
pub struct Metrics {
    running: tokio::sync::Mutex<Running>,
    tailscale: Arc<TailscaleService>,
    bound: Bound,
    queue_db: Arc<OnceLock<Arc<Database>>>,
    workers: usize,
    download_dir: PathBuf,
}

#[derive(Default)]
struct Running {
    /// The listener and the path it serves
    listener: Option<(ListenerHandle, String)>,
    /// Build info, gauges and exposure warnings, set up the first time metrics turn on
    set_up: bool,
}

/// Start metrics for this process.
///
/// `queue_db` can be set by the caller at any time (or never); until then the queue
/// gauges stay at zero. `[metrics] bind` is a list like `[api] bind` (feature 037): an
/// address that can't be bound is logged and retried in the background (FR-008); an
/// invalid setting is an error. Waits up to 2 s for a first bound address.
pub async fn start(
    config: &Config,
    queue_db: Arc<OnceLock<Arc<Database>>>,
    workers: usize,
    download_dir: PathBuf,
) -> Result<Arc<Metrics>, CoreError> {
    let resolved = config.resolve_metrics()?;
    let metrics = Arc::new(Metrics {
        running: tokio::sync::Mutex::new(Running::default()),
        tailscale: Arc::new(TailscaleService::new(
            config.tailscale.socket_path(),
            Cli::Auto,
        )),
        bound: Bound::default(),
        queue_db,
        workers,
        download_dir,
    });
    let source = resolved.source;
    let MetricsListen { settings, bind } = resolved.value;
    if !settings.enabled {
        info!(source = ?source, "Metrics disabled");
        return Ok(metrics);
    }
    metrics.turn_on(&settings, bind.clone()).await;
    metrics.first_bound(Duration::from_secs(2)).await;
    info!(
        bind = %shown(&bind),
        path = %settings.path,
        detail = %settings.detail,
        source = ?source,
        "Metrics enabled"
    );
    Ok(metrics)
}

fn shown(entries: &[ListenAddr]) -> String {
    entries
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

impl Metrics {
    /// What the listener holds now
    pub fn bound(&self) -> Bound {
        self.bound.clone()
    }

    async fn first_bound(&self, timeout: Duration) -> Option<SocketAddr> {
        let until = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(addr) = self.bound.first() {
                return Some(addr);
            }
            if tokio::time::Instant::now() >= until {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// Follow `config`: on, off, a new list of addresses, a new path or detail. An
    /// invalid `[metrics]` (or its environment overrides) is logged and changes nothing.
    pub async fn apply(&self, config: &Config) {
        let socket = config.tailscale.socket_path();
        if self.tailscale.socket() != socket {
            self.tailscale.configure(socket);
        }
        let MetricsListen { settings, bind } = match config.resolve_metrics() {
            Ok(r) => r.value,
            Err(e) => {
                warn!(error = %e, "Metrics settings not applied");
                return;
            }
        };
        if !settings.enabled {
            let listener = self.running.lock().await.listener.take();
            if let Some((handle, _)) = listener {
                handle.stop(Duration::from_secs(1)).await;
                info!("Metrics turned off");
            }
            return;
        }
        global().set_detail(settings.detail);
        let running = self.running.lock().await;
        match &running.listener {
            Some((handle, path)) if *path == settings.path => {
                if handle.set_entries(bind.clone()) {
                    info!(bind = %shown(&bind), "Metrics addresses changed");
                }
            }
            _ => {
                let was_on = running.listener.is_some();
                drop(running);
                self.turn_on(&settings, bind.clone()).await;
                info!(
                    bind = %shown(&bind),
                    path = %settings.path,
                    "{}",
                    if was_on { "Metrics path changed" } else { "Metrics turned on" }
                );
            }
        }
    }

    /// Serve `settings.path` on `bind`, closing any listener there was first
    async fn turn_on(&self, settings: &MetricsSettings, bind: Vec<ListenAddr>) {
        let telemetry = global();
        let mut running = self.running.lock().await;
        if let Some((handle, _)) = running.listener.take() {
            handle.stop(Duration::from_secs(1)).await;
        }
        telemetry.set_detail(settings.detail);
        if !running.set_up {
            running.set_up = true;
            telemetry.init(
                self.workers,
                env!("CARGO_PKG_VERSION"),
                option_env!("PUTIO_SYNC_GIT_SHA").unwrap_or("unknown"),
            );
            spawn_exposure_warnings(self.bound.clone(), settings.path.clone());
            let queue_db = self.queue_db.clone();
            gauges::spawn_refresh(telemetry, self.download_dir.clone(), move || {
                let db = queue_db.get().cloned();
                async move {
                    let counts = db?.queue().counts_by_status().await.ok()?;
                    Some(
                        counts
                            .into_iter()
                            .filter_map(|(status, n)| {
                                QueueState::from_status(&status).map(|s| (s, n))
                            })
                            .collect(),
                    )
                }
            });
        }
        let handle = Supervisor::spawn(
            "Metrics",
            bind,
            self.tailscale.clone(),
            server::router(&settings.path, telemetry),
            self.bound.clone(),
        );
        running.listener = Some((handle, settings.path.clone()));
    }
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
