//! Prometheus metrics for putio-sync.
//!
//! This crate sits below every engine crate. It owns the registry, every metric family
//! (see `specs/003-telemetry-prometheus/contracts/metrics.md`), the closed label sets and
//! the metrics listener. Engine crates record through the free functions in [`api`],
//! [`sync`], [`download`], [`watch`], [`cleanup`] and [`db`], which use the process-wide [`global`]
//! instance. Tests build their own instance with [`Telemetry::new`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use prometheus::{
    Encoder, Gauge, GaugeVec, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec,
    IntGauge, IntGaugeVec, Opts, Registry, TextEncoder,
};

pub mod api;
pub mod cleanup;
pub mod db;
pub mod download;
pub mod gauges;
pub mod labels;
pub mod server;
pub mod sync;
pub mod transfers;
pub mod watch;

pub use labels::*;

const API_BUCKETS: &[f64] = &[0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0];
const DOWNLOAD_BUCKETS: &[f64] = &[1.0, 5.0, 15.0, 60.0, 300.0, 900.0, 1800.0, 3600.0, 7200.0];
const SYNC_BUCKETS: &[f64] = &[1.0, 5.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0];

/// Every metric family, registered in one registry.
pub struct Telemetry {
    registry: Registry,
    detailed: AtomicBool,
    worker_children: OnceLock<Vec<(IntGauge, IntGauge)>>,

    pub(crate) api_requests: IntCounterVec,
    pub(crate) api_retries: IntCounterVec,
    pub(crate) api_duration: Histogram,
    pub(crate) api_op_duration: HistogramVec,

    pub(crate) sync_runs: IntCounterVec,
    pub(crate) sync_duration: Histogram,
    pub(crate) last_sync_duration: Gauge,
    // Label-less vecs: the series is absent until the first `with_label_values(&[])`.
    pub(crate) last_success: GaugeVec,
    pub(crate) last_sync_files: IntGaugeVec,
    pub(crate) files_blocked: IntCounter,

    pub(crate) queue_items: IntGaugeVec,
    pub(crate) downloads_active: IntGauge,
    pub(crate) download_workers: IntGauge,
    pub(crate) downloads_completed: IntCounter,
    pub(crate) downloads_failed: IntCounterVec,
    pub(crate) downloaded_bytes: IntCounter,
    pub(crate) download_duration: Histogram,
    pub(crate) worker_state: IntGaugeVec,
    pub(crate) worker_inflight: IntGaugeVec,

    pub(crate) watch_scans: IntCounter,
    pub(crate) watch_uploads: IntCounter,
    pub(crate) watch_cleanups: IntCounter,
    pub(crate) watch_invalid: IntCounter,
    pub(crate) watch_cleanup_failures: IntCounter,

    pub(crate) empty_folder_runs: IntCounterVec,
    pub(crate) empty_folders_deleted: IntCounter,
    pub(crate) empty_folder_delete_failures: IntCounter,
    // Label-less vec: absent until the first successful run
    pub(crate) empty_folder_last_success: GaugeVec,
    pub(crate) transfers_stalled: IntGauge,

    pub(crate) db_errors: IntCounterVec,
    pub(crate) disk_free: GaugeVec,
    pub(crate) disk_total: GaugeVec,
    pub(crate) start_time: Gauge,
    pub(crate) build_info: IntGaugeVec,
}

fn opts(name: &str, help: &str) -> Opts {
    Opts::new(format!("putio_sync_{}", name), help)
}

fn hopts(name: &str, help: &str, buckets: &[f64]) -> HistogramOpts {
    HistogramOpts::new(format!("putio_sync_{}", name), help).buckets(buckets.to_vec())
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl Telemetry {
    /// Build a standalone instance with every family registered and every closed
    /// label combination zero-initialised, so all families appear in the first scrape.
    pub fn new() -> Self {
        let t = Telemetry {
            registry: Registry::new(),
            detailed: AtomicBool::new(false),
            worker_children: OnceLock::new(),

            api_requests: IntCounterVec::new(
                opts(
                    "api_requests_total",
                    "Put.io HTTP attempts, retries included.",
                ),
                &["operation", "outcome"],
            )
            .unwrap(),
            api_retries: IntCounterVec::new(
                opts(
                    "api_retries_total",
                    "Put.io retries scheduled by the retry policy.",
                ),
                &["reason"],
            )
            .unwrap(),
            api_duration: Histogram::with_opts(hopts(
                "api_request_duration_seconds",
                "Latency of each Put.io HTTP attempt.",
                API_BUCKETS,
            ))
            .unwrap(),
            api_op_duration: HistogramVec::new(
                hopts(
                    "api_operation_duration_seconds",
                    "Latency of each Put.io HTTP attempt by operation (detailed only).",
                    API_BUCKETS,
                ),
                &["operation"],
            )
            .unwrap(),

            sync_runs: IntCounterVec::new(
                opts("sync_runs_total", "Completed sync runs."),
                &["result"],
            )
            .unwrap(),
            sync_duration: Histogram::with_opts(hopts(
                "sync_duration_seconds",
                "Duration of each sync run.",
                SYNC_BUCKETS,
            ))
            .unwrap(),
            last_sync_duration: Gauge::with_opts(opts(
                "last_sync_duration_seconds",
                "Duration of the latest sync run.",
            ))
            .unwrap(),
            last_success: GaugeVec::new(
                opts(
                    "last_success_timestamp_seconds",
                    "Unix time the last successful sync run finished.",
                ),
                &[],
            )
            .unwrap(),
            last_sync_files: IntGaugeVec::new(
                opts("last_sync_files", "File counts from the latest sync run."),
                &["kind"],
            )
            .unwrap(),
            files_blocked: IntCounter::with_opts(opts(
                "files_blocked_total",
                "Files skipped by the blocked-extension list, once per file id.",
            ))
            .unwrap(),

            queue_items: IntGaugeVec::new(
                opts("queue_items", "Download queue rows by state."),
                &["state"],
            )
            .unwrap(),
            transfers_stalled: IntGauge::with_opts(opts(
                "transfers_stalled",
                "Put.io transfers with no progress for the stall threshold, at the last check.",
            ))
            .unwrap(),
            downloads_active: IntGauge::with_opts(opts(
                "downloads_active",
                "Downloads in progress.",
            ))
            .unwrap(),
            download_workers: IntGauge::with_opts(opts(
                "download_workers",
                "Configured download worker count.",
            ))
            .unwrap(),
            downloads_completed: IntCounter::with_opts(opts(
                "downloads_completed_total",
                "Downloads verified and renamed into place.",
            ))
            .unwrap(),
            downloads_failed: IntCounterVec::new(
                opts("downloads_failed_total", "Downloads that ended in failure."),
                &["reason"],
            )
            .unwrap(),
            downloaded_bytes: IntCounter::with_opts(opts(
                "downloaded_bytes_total",
                "Bytes written to disk by downloads.",
            ))
            .unwrap(),
            download_duration: Histogram::with_opts(hopts(
                "download_duration_seconds",
                "Time per completed download.",
                DOWNLOAD_BUCKETS,
            ))
            .unwrap(),
            worker_state: IntGaugeVec::new(
                opts(
                    "worker_state",
                    "Download worker state, 0 idle and 1 downloading (detailed only).",
                ),
                &["worker"],
            )
            .unwrap(),
            worker_inflight: IntGaugeVec::new(
                opts(
                    "worker_inflight_bytes",
                    "Bytes written so far for the worker's current file (detailed only).",
                ),
                &["worker"],
            )
            .unwrap(),

            watch_scans: IntCounter::with_opts(opts("watch_scans_total", "Watch-folder scans."))
                .unwrap(),
            watch_uploads: IntCounter::with_opts(opts(
                "watch_uploads_total",
                "Torrent and magnet files uploaded to Put.io.",
            ))
            .unwrap(),
            watch_cleanups: IntCounter::with_opts(opts(
                "watch_cleanups_total",
                "Already-uploaded watch files cleaned up.",
            ))
            .unwrap(),
            watch_invalid: IntCounter::with_opts(opts(
                "watch_invalid_total",
                "Invalid watch files, once per file.",
            ))
            .unwrap(),
            watch_cleanup_failures: IntCounter::with_opts(opts(
                "watch_cleanup_failures_total",
                "Watch-file cleanup (delete or archive) failures.",
            ))
            .unwrap(),

            empty_folder_runs: IntCounterVec::new(
                opts(
                    "empty_folder_runs_total",
                    "Finished empty-folder cleanup runs (dry runs not counted).",
                ),
                &["trigger", "result"],
            )
            .unwrap(),
            empty_folders_deleted: IntCounter::with_opts(opts(
                "empty_folders_deleted_total",
                "Empty folders removed from Put.io, nested ones included.",
            ))
            .unwrap(),
            empty_folder_delete_failures: IntCounter::with_opts(opts(
                "empty_folder_delete_failures_total",
                "Empty-folder deletes that failed.",
            ))
            .unwrap(),
            empty_folder_last_success: GaugeVec::new(
                opts(
                    "empty_folder_last_success_timestamp_seconds",
                    "Unix time the last successful empty-folder cleanup finished.",
                ),
                &[],
            )
            .unwrap(),

            db_errors: IntCounterVec::new(
                opts("db_errors_total", "Database errors."),
                &["operation"],
            )
            .unwrap(),
            disk_free: GaugeVec::new(
                opts("disk_free_bytes", "Free space for the download folder."),
                &[],
            )
            .unwrap(),
            disk_total: GaugeVec::new(
                opts(
                    "disk_total_bytes",
                    "Size of the download folder's filesystem.",
                ),
                &[],
            )
            .unwrap(),
            start_time: Gauge::with_opts(opts(
                "start_time_seconds",
                "Process start time, Unix seconds.",
            ))
            .unwrap(),
            build_info: IntGaugeVec::new(
                opts("build_info", "Build identity; always 1."),
                &["version", "commit"],
            )
            .unwrap(),
        };
        t.register_all();
        t.zero_init();
        t
    }

    fn register_all(&self) {
        let r = &self.registry;
        let collectors: Vec<Box<dyn prometheus::core::Collector>> = vec![
            Box::new(self.api_requests.clone()),
            Box::new(self.api_retries.clone()),
            Box::new(self.api_duration.clone()),
            Box::new(self.api_op_duration.clone()),
            Box::new(self.sync_runs.clone()),
            Box::new(self.sync_duration.clone()),
            Box::new(self.last_sync_duration.clone()),
            Box::new(self.last_success.clone()),
            Box::new(self.last_sync_files.clone()),
            Box::new(self.files_blocked.clone()),
            Box::new(self.queue_items.clone()),
            Box::new(self.downloads_active.clone()),
            Box::new(self.download_workers.clone()),
            Box::new(self.downloads_completed.clone()),
            Box::new(self.downloads_failed.clone()),
            Box::new(self.downloaded_bytes.clone()),
            Box::new(self.download_duration.clone()),
            Box::new(self.worker_state.clone()),
            Box::new(self.worker_inflight.clone()),
            Box::new(self.watch_scans.clone()),
            Box::new(self.watch_uploads.clone()),
            Box::new(self.watch_cleanups.clone()),
            Box::new(self.watch_invalid.clone()),
            Box::new(self.watch_cleanup_failures.clone()),
            Box::new(self.empty_folder_runs.clone()),
            Box::new(self.empty_folders_deleted.clone()),
            Box::new(self.empty_folder_delete_failures.clone()),
            Box::new(self.empty_folder_last_success.clone()),
            Box::new(self.transfers_stalled.clone()),
            Box::new(self.db_errors.clone()),
            Box::new(self.disk_free.clone()),
            Box::new(self.disk_total.clone()),
            Box::new(self.start_time.clone()),
            Box::new(self.build_info.clone()),
        ];
        for c in collectors {
            r.register(c).expect("metric families have unique names");
        }
    }

    fn zero_init(&self) {
        for op in ApiOp::ALL {
            for outcome in ApiOutcome::ALL {
                self.api_requests
                    .with_label_values(&[op.as_str(), outcome.as_str()]);
            }
        }
        for r in RetryReason::ALL {
            self.api_retries.with_label_values(&[r.as_str()]);
        }
        for r in SyncResult::ALL {
            self.sync_runs.with_label_values(&[r.as_str()]);
        }
        for k in SyncKind::ALL {
            self.last_sync_files.with_label_values(&[k.as_str()]);
        }
        for s in QueueState::ALL {
            self.queue_items.with_label_values(&[s.as_str()]);
        }
        for r in FailReason::ALL {
            self.downloads_failed.with_label_values(&[r.as_str()]);
        }
        for t in CleanupTrigger::ALL {
            for r in SyncResult::ALL {
                self.empty_folder_runs
                    .with_label_values(&[t.as_str(), r.as_str()]);
            }
        }
        for op in DbOp::ALL {
            self.db_errors.with_label_values(&[op.as_str()]);
        }
    }

    /// Set build info, start time and the configured worker count. At `detailed`, also
    /// creates the per-worker series `worker="0"` up to `workers - 1`, so call
    /// [`Telemetry::set_detail`] first. Only the first call creates worker series.
    pub fn init(&self, workers: usize, version: &str, commit: &str) {
        self.build_info.reset();
        self.build_info.with_label_values(&[version, commit]).set(1);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);
        self.start_time.set(now.as_secs_f64());
        self.download_workers.set(workers as i64);
        if self.detailed() {
            self.worker_children.get_or_init(|| {
                (0..workers)
                    .map(|i| {
                        let id = i.to_string();
                        (
                            self.worker_state.with_label_values(&[&id]),
                            self.worker_inflight.with_label_values(&[&id]),
                        )
                    })
                    .collect()
            });
        }
    }

    /// Choose `basic` or `detailed` output. Set once at startup, before [`Telemetry::init`].
    pub fn set_detail(&self, detail: Detail) {
        self.detailed
            .store(detail == Detail::Detailed, Ordering::Relaxed);
    }

    /// Whether detailed-only families are recorded.
    pub fn detailed(&self) -> bool {
        self.detailed.load(Ordering::Relaxed)
    }

    pub(crate) fn worker(&self, id: usize) -> Option<&(IntGauge, IntGauge)> {
        self.worker_children.get().and_then(|w| w.get(id))
    }

    /// Encode every family in the Prometheus text exposition format.
    pub fn encode(&self) -> String {
        let mut buf = Vec::new();
        TextEncoder::new()
            .encode(&self.registry.gather(), &mut buf)
            .expect("text encoding into a Vec cannot fail");
        String::from_utf8(buf).expect("text encoder writes UTF-8")
    }

    /// Read one sample: a counter or gauge value, or a histogram's observation count.
    /// Returns `None` when the series doesn't exist. Meant for tests.
    pub fn sample(&self, name: &str, labels: &[(&str, &str)]) -> Option<f64> {
        use prometheus::proto::MetricType;

        let family = self
            .registry
            .gather()
            .into_iter()
            .find(|f| f.name() == name)?;
        let kind = family.get_field_type();
        family.get_metric().iter().find_map(|m| {
            let pairs = m.get_label();
            let matches = pairs.len() == labels.len()
                && labels
                    .iter()
                    .all(|(k, v)| pairs.iter().any(|p| p.name() == *k && p.value() == *v));
            if !matches {
                return None;
            }
            match kind {
                MetricType::COUNTER => Some(m.get_counter().get_value()),
                MetricType::GAUGE => Some(m.get_gauge().get_value()),
                MetricType::HISTOGRAM => Some(m.get_histogram().get_sample_count() as f64),
                _ => None,
            }
        })
    }
}

/// The process-wide instance used by every engine crate.
pub fn global() -> &'static Telemetry {
    static GLOBAL: OnceLock<Telemetry> = OnceLock::new();
    GLOBAL.get_or_init(Telemetry::new)
}

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs_f64()
}
