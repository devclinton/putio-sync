//! Prometheus metrics for putio-sync.
//!
//! This crate sits below every engine crate. It owns the registry, every metric family
//! (see `specs/003-telemetry-prometheus/contracts/metrics.md`), the closed label sets and
//! the metrics listener. Engine crates record through the free functions in [`api`],
//! [`sync`], [`download`], [`watch`], [`cleanup`] and [`db`], which use the process-wide [`global`]
//! instance. [`process`] reads the process's own CPU and memory figures at scrape time. Tests build their own instance with [`Telemetry::new`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use prometheus::{
    Encoder, Gauge, GaugeVec, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec,
    IntGauge, IntGaugeVec, Opts, Registry, TextEncoder,
};

pub mod api;
pub mod arr;
pub mod cleanup;
pub mod db;
pub mod doctor;
pub mod download;
pub mod gauges;
pub mod hooks;
pub mod labels;
pub mod logs;
pub mod pipeline;
pub mod plugins;
pub mod precheck;
pub mod process;
pub mod server;
pub mod setup;
pub mod steps;
pub mod storage;
pub mod sync;
pub mod tailscale;
pub mod transfers;
pub mod verify;
pub mod watch;

pub use labels::*;

const API_BUCKETS: &[f64] = &[0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0];
const DOWNLOAD_BUCKETS: &[f64] = &[1.0, 5.0, 15.0, 60.0, 300.0, 900.0, 1800.0, 3600.0, 7200.0];
const LOOKUP_BUCKETS: &[f64] = &[0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0];
/// How long a magnet was held (043)
const HOLD_BUCKETS: &[f64] = &[30.0, 60.0, 120.0, 180.0, 300.0, 600.0];
/// Plugin call time (028)
const PLUGIN_BUCKETS: &[f64] = &[0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0];
/// Hook run time (027)
const HOOK_BUCKETS: &[f64] = &[0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 600.0];
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
    pub(crate) sync_in_progress: IntGauge,
    pub(crate) rule_decisions: IntCounterVec,

    pub(crate) queue_items: IntGaugeVec,
    pub(crate) downloads_active: IntGauge,
    pub(crate) download_workers: IntGauge,
    // Feature 032
    pub(crate) bandwidth_limit: Gauge,
    pub(crate) bandwidth_override: IntGauge,
    pub(crate) downloads_completed: IntCounter,
    pub(crate) downloads_failed: IntCounterVec,
    pub(crate) existing_files: IntCounterVec,
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
    pub(crate) setup_required: IntGauge,
    // Feature 039
    pub(crate) pipeline_items: IntGaugeVec,
    pub(crate) pipeline_actions: IntCounterVec,
    // Feature 038
    pub(crate) pipeline_steps: IntCounterVec,
    pub(crate) pipeline_step_seconds: HistogramVec,
    pub(crate) pipeline_items_waiting: IntGaugeVec,
    pub(crate) nas_downloads_paused: IntGauge,

    // Feature 010; absent until the first doctor run
    pub(crate) doctor_check_status: GaugeVec,
    pub(crate) doctor_check_items: IntGaugeVec,
    pub(crate) doctor_last_run: GaugeVec,
    pub(crate) doctor_last_duration: GaugeVec,
    pub(crate) arr_rpc_requests: IntCounterVec,
    pub(crate) arr_items: IntGaugeVec,
    pub(crate) arr_blocklist: IntCounterVec,
    pub(crate) arr_setup: IntCounterVec,
    pub(crate) arr_path_mapping: IntCounterVec,
    // Feature 042
    pub(crate) precheck: IntCounterVec,
    pub(crate) precheck_avoided_bytes: IntCounterVec,
    pub(crate) precheck_cache_lookups: IntCounterVec,
    pub(crate) precheck_cache_lookup_seconds: Histogram,
    pub(crate) precheck_fallback: IntCounterVec,
    pub(crate) precheck_fallback_putio_bytes: IntCounter,
    pub(crate) release_flags: IntCounterVec,
    // Feature 043
    pub(crate) precheck_held: IntCounterVec,
    pub(crate) precheck_held_now: IntGauge,
    pub(crate) precheck_held_seconds: HistogramVec,
    // Feature 033
    pub(crate) putio_storage_size: Gauge,
    pub(crate) putio_storage_used: Gauge,
    pub(crate) storage_free_runs: IntCounterVec,
    pub(crate) storage_freed_files: IntCounter,
    pub(crate) storage_freed_bytes: IntCounter,
    // Feature 034
    pub(crate) verify_checks: IntCounterVec,
    pub(crate) verify_redownloads: IntCounterVec,
    pub(crate) verify_run_files: IntCounterVec,
    pub(crate) verify_read_bytes: IntCounter,
    pub(crate) verify_run_active: IntGauge,
    // Feature 036
    pub(crate) tailscale_identity: IntCounterVec,
    // Feature 027
    pub(crate) events: IntCounterVec,
    pub(crate) hook_runs: IntCounterVec,
    pub(crate) hook_duration: HistogramVec,
    pub(crate) hook_dropped: IntCounterVec,
    // Feature 028
    pub(crate) plugin_runs: IntCounterVec,
    pub(crate) plugin_duration: HistogramVec,
    pub(crate) plugin_dropped: IntCounterVec,
    pub(crate) plugin_counter: IntCounterVec,
    // Feature 047
    pub(crate) log_lines_dropped: IntCounterVec,

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
                "Files blocked or skipped by a folder rule or the blocked-extension list, once per file id.",
            ))
            .unwrap(),
            rule_decisions: IntCounterVec::new(
                opts(
                    "rule_decisions_total",
                    "Folder rule decisions: files blocked, skipped or deleted from Put.io.",
                ),
                &["decision"],
            )
            .unwrap(),

            queue_items: IntGaugeVec::new(
                opts("queue_items", "Download queue rows by state."),
                &["state"],
            )
            .unwrap(),
            sync_in_progress: IntGauge::with_opts(opts(
                "sync_in_progress",
                "1 while a Put.io sync run is in progress.",
            ))
            .unwrap(),
            transfers_stalled: IntGauge::with_opts(opts(
                "transfers_stalled",
                "Put.io transfers with no progress for the stall threshold, at the last check.",
            ))
            .unwrap(),
            setup_required: IntGauge::with_opts(opts(
                "setup_required",
                "1 while the daemon waits in setup mode for its first-run setup.",
            ))
            .unwrap(),
            pipeline_items: IntGaugeVec::new(
                opts(
                    "pipeline_items",
                    "Active transfers on the Active page, by stage and state.",
                ),
                &["stage", "state"],
            )
            .unwrap(),
            pipeline_actions: IntCounterVec::new(
                opts(
                    "pipeline_actions_total",
                    "Pause, hold and resume actions on active transfers.",
                ),
                &["action"],
            )
            .unwrap(),
            pipeline_steps: IntCounterVec::new(
                opts(
                    "pipeline_steps_total",
                    "Processing pipeline step runs, by stage, kind and result.",
                ),
                &["stage", "kind", "result"],
            )
            .unwrap(),
            pipeline_step_seconds: HistogramVec::new(
                hopts(
                    "pipeline_step_seconds",
                    "How long processing pipeline steps took.",
                    HOOK_BUCKETS,
                ),
                &["stage", "kind"],
            )
            .unwrap(),
            pipeline_items_waiting: IntGaugeVec::new(
                opts(
                    "pipeline_items_waiting",
                    "Items waiting for the operator at a processing pipeline stage.",
                ),
                &["stage"],
            )
            .unwrap(),
            nas_downloads_paused: IntGauge::with_opts(opts(
                "nas_downloads_paused",
                "1 while the Pause NAS downloads switch is on, else 0.",
            ))
            .unwrap(),
            doctor_check_status: GaugeVec::new(
                opts(
                    "doctor_check_status",
                    "Doctor check result: 0 ok, 1 warning, 2 problem, -1 skipped.",
                ),
                &["check"],
            )
            .unwrap(),
            doctor_check_items: IntGaugeVec::new(
                opts(
                    "doctor_check_items",
                    "Items a doctor check found at its last run.",
                ),
                &["check"],
            )
            .unwrap(),
            doctor_last_run: GaugeVec::new(
                opts(
                    "doctor_last_run_timestamp_seconds",
                    "Unix time the last doctor run finished.",
                ),
                &[],
            )
            .unwrap(),
            doctor_last_duration: GaugeVec::new(
                opts(
                    "doctor_last_run_duration_seconds",
                    "Duration of the last doctor run.",
                ),
                &[],
            )
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
            bandwidth_limit: Gauge::with_opts(opts(
                "download_bandwidth_limit_bytes_per_second",
                "Download limit in force for all downloads together; +Inf unlimited, 0 paused.",
            ))
            .unwrap(),
            bandwidth_override: IntGauge::with_opts(opts(
                "download_bandwidth_override",
                "1 while a timed bandwidth pause or slow-down is in force.",
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
            existing_files: IntCounterVec::new(
                opts(
                    "existing_files_total",
                    "Files that already existed locally, by what was done with them.",
                ),
                &["outcome"],
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

            arr_rpc_requests: IntCounterVec::new(
                opts(
                    "arr_rpc_requests_total",
                    "Transmission RPC requests from Radarr/Sonarr, by method and outcome.",
                ),
                &["method", "outcome"],
            )
            .unwrap(),
            arr_items: IntGaugeVec::new(
                opts("arr_items", "Radarr/Sonarr items tracked, by state."),
                &["state"],
            )
            .unwrap(),
            arr_blocklist: IntCounterVec::new(
                opts(
                    "arr_blocklist_total",
                    "Calls asking Radarr/Sonarr to blocklist a failed release.",
                ),
                &["outcome"],
            )
            .unwrap(),
            arr_setup: IntCounterVec::new(
                opts(
                    "arr_setup_total",
                    "Radarr/Sonarr auto-setups, by app kind and result.",
                ),
                &["kind", "result"],
            )
            .unwrap(),
            arr_path_mapping: IntCounterVec::new(
                opts(
                    "arr_path_mapping_total",
                    "Remote Path Mapping changes made in Radarr/Sonarr by setup, by result.",
                ),
                &["result"],
            )
            .unwrap(),
            precheck: IntCounterVec::new(
                opts(
                    "precheck_total",
                    "Release pre-checks, by source, stage and outcome.",
                ),
                &["source", "stage", "outcome"],
            )
            .unwrap(),
            precheck_avoided_bytes: IntCounterVec::new(
                opts(
                    "precheck_avoided_bytes_total",
                    "Bytes in the file lists of rejected releases.",
                ),
                &["stage"],
            )
            .unwrap(),
            precheck_cache_lookups: IntCounterVec::new(
                opts(
                    "precheck_cache_lookups_total",
                    "Magnet .torrent cache lookups, by cache host and result.",
                ),
                &["cache", "result"],
            )
            .unwrap(),
            precheck_cache_lookup_seconds: Histogram::with_opts(hopts(
                "precheck_cache_lookup_seconds",
                "Time to look one magnet up across the caches.",
                LOOKUP_BUCKETS,
            ))
            .unwrap(),
            precheck_fallback: IntCounterVec::new(
                opts(
                    "precheck_fallback_total",
                    "Magnets added to Put.io unchecked, by how the check on Put.io went.",
                ),
                &["result"],
            )
            .unwrap(),
            precheck_fallback_putio_bytes: IntCounter::with_opts(opts(
                "precheck_fallback_putio_bytes_total",
                "Bytes Put.io downloaded for unchecked magnets that were then rejected.",
            ))
            .unwrap(),
            release_flags: IntCounterVec::new(
                opts("release_flags_total", "Operator flags on releases, by kind."),
                &["kind"],
            )
            .unwrap(),
            precheck_held: IntCounterVec::new(
                opts(
                    "precheck_held_total",
                    "Magnets held for a later cache ask, by how the hold ended.",
                ),
                &["result"],
            )
            .unwrap(),
            precheck_held_now: IntGauge::with_opts(opts(
                "precheck_held",
                "Magnets held now, waiting for a cache to have their file list.",
            ))
            .unwrap(),
            precheck_held_seconds: HistogramVec::new(
                hopts(
                    "precheck_held_seconds",
                    "How long a magnet was held, by how the hold ended.",
                    HOLD_BUCKETS,
                ),
                &["result"],
            )
            .unwrap(),

            putio_storage_size: Gauge::with_opts(opts(
                "putio_storage_size_bytes",
                "Put.io account storage size.",
            ))
            .unwrap(),
            putio_storage_used: Gauge::with_opts(opts(
                "putio_storage_used_bytes",
                "Put.io account storage in use.",
            ))
            .unwrap(),
            storage_free_runs: IntCounterVec::new(
                opts(
                    "storage_free_runs_total",
                    "Finished Put.io free-up runs (previews not counted).",
                ),
                &["trigger", "result"],
            )
            .unwrap(),
            storage_freed_files: IntCounter::with_opts(opts(
                "storage_freed_files_total",
                "Files deleted from Put.io to free space.",
            ))
            .unwrap(),
            storage_freed_bytes: IntCounter::with_opts(opts(
                "storage_freed_bytes_total",
                "Bytes deleted from Put.io to free space.",
            ))
            .unwrap(),
            verify_checks: IntCounterVec::new(
                opts(
                    "verify_total",
                    "Files checked against Put.io, by what started the check, how and the result.",
                ),
                &["trigger", "method", "result"],
            )
            .unwrap(),
            verify_redownloads: IntCounterVec::new(
                opts(
                    "verify_redownloads_total",
                    "Files downloaded again after a mismatch, by what found it.",
                ),
                &["trigger"],
            )
            .unwrap(),
            verify_run_files: IntCounterVec::new(
                opts(
                    "verify_run_files_total",
                    "Files a verify run looked at, by result.",
                ),
                &["result"],
            )
            .unwrap(),
            verify_read_bytes: IntCounter::with_opts(opts(
                "verify_read_bytes_total",
                "Bytes read back from disk to compute a CRC32 (resumed downloads and verify runs).",
            ))
            .unwrap(),
            verify_run_active: IntGauge::with_opts(opts(
                "verify_run_active",
                "1 while a verify run is going.",
            ))
            .unwrap(),
            tailscale_identity: IntCounterVec::new(
                opts(
                    "tailscale_identity_total",
                    "Requests that claimed a Tailscale identity, by how it was found and whether it was let in.",
                ),
                &["via", "result"],
            )
            .unwrap(),
            events: IntCounterVec::new(
                opts("events_total", "Events put on the internal event bus, by type."),
                &["type"],
            )
            .unwrap(),
            hook_runs: IntCounterVec::new(
                opts(
                    "hook_runs_total",
                    "Finished hook runs, by hook, event type and result.",
                ),
                &["hook", "event", "result"],
            )
            .unwrap(),
            hook_duration: HistogramVec::new(
                hopts(
                    "hook_run_duration_seconds",
                    "How long hook runs took, retries included.",
                    HOOK_BUCKETS,
                ),
                &["hook"],
            )
            .unwrap(),
            hook_dropped: IntCounterVec::new(
                opts(
                    "hook_events_dropped_total",
                    "Events a hook never ran for because its queue was full.",
                ),
                &["hook"],
            )
            .unwrap(),
            log_lines_dropped: IntCounterVec::new(
                opts(
                    "log_lines_dropped_total",
                    "Log lines the Logs page lost: evicted from the full buffer, or skipped by a live tail that fell behind.",
                ),
                &["reason"],
            )
            .unwrap(),
            plugin_runs: IntCounterVec::new(
                opts(
                    "plugin_runs_total",
                    "Finished plugin calls, by plugin, event type (or tick, init) and result.",
                ),
                &["plugin", "event", "result"],
            )
            .unwrap(),
            plugin_duration: HistogramVec::new(
                hopts(
                    "plugin_run_duration_seconds",
                    "How long plugin calls took.",
                    PLUGIN_BUCKETS,
                ),
                &["plugin"],
            )
            .unwrap(),
            plugin_dropped: IntCounterVec::new(
                opts(
                    "plugin_events_dropped_total",
                    "Events a plugin never ran for because its queue was full.",
                ),
                &["plugin"],
            )
            .unwrap(),
            plugin_counter: IntCounterVec::new(
                opts(
                    "plugin_counter_total",
                    "Counts a plugin keeps with putio.metrics.count (at most 16 names each).",
                ),
                &["plugin", "name"],
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
            Box::new(self.sync_in_progress.clone()),
            Box::new(self.rule_decisions.clone()),
            Box::new(self.queue_items.clone()),
            Box::new(self.downloads_active.clone()),
            Box::new(self.download_workers.clone()),
            Box::new(self.bandwidth_limit.clone()),
            Box::new(self.bandwidth_override.clone()),
            Box::new(self.downloads_completed.clone()),
            Box::new(self.downloads_failed.clone()),
            Box::new(self.existing_files.clone()),
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
            Box::new(self.setup_required.clone()),
            Box::new(self.pipeline_items.clone()),
            Box::new(self.pipeline_actions.clone()),
            Box::new(self.pipeline_steps.clone()),
            Box::new(self.pipeline_step_seconds.clone()),
            Box::new(self.pipeline_items_waiting.clone()),
            Box::new(self.nas_downloads_paused.clone()),
            Box::new(self.doctor_check_status.clone()),
            Box::new(self.doctor_check_items.clone()),
            Box::new(self.doctor_last_run.clone()),
            Box::new(self.doctor_last_duration.clone()),
            Box::new(self.arr_rpc_requests.clone()),
            Box::new(self.arr_items.clone()),
            Box::new(self.arr_blocklist.clone()),
            Box::new(self.arr_setup.clone()),
            Box::new(self.arr_path_mapping.clone()),
            Box::new(self.precheck.clone()),
            Box::new(self.precheck_avoided_bytes.clone()),
            Box::new(self.precheck_cache_lookups.clone()),
            Box::new(self.precheck_cache_lookup_seconds.clone()),
            Box::new(self.precheck_fallback.clone()),
            Box::new(self.precheck_fallback_putio_bytes.clone()),
            Box::new(self.precheck_held.clone()),
            Box::new(self.precheck_held_now.clone()),
            Box::new(self.precheck_held_seconds.clone()),
            Box::new(self.release_flags.clone()),
            Box::new(self.putio_storage_size.clone()),
            Box::new(self.putio_storage_used.clone()),
            Box::new(self.storage_free_runs.clone()),
            Box::new(self.storage_freed_files.clone()),
            Box::new(self.storage_freed_bytes.clone()),
            Box::new(self.verify_checks.clone()),
            Box::new(self.verify_redownloads.clone()),
            Box::new(self.verify_run_files.clone()),
            Box::new(self.verify_read_bytes.clone()),
            Box::new(self.verify_run_active.clone()),
            Box::new(self.tailscale_identity.clone()),
            Box::new(self.events.clone()),
            Box::new(self.hook_runs.clone()),
            Box::new(self.hook_duration.clone()),
            Box::new(self.hook_dropped.clone()),
            Box::new(self.plugin_runs.clone()),
            Box::new(self.plugin_duration.clone()),
            Box::new(self.plugin_dropped.clone()),
            Box::new(self.plugin_counter.clone()),
            Box::new(self.log_lines_dropped.clone()),
            Box::new(self.db_errors.clone()),
            Box::new(self.disk_free.clone()),
            Box::new(self.disk_total.clone()),
            Box::new(self.start_time.clone()),
            Box::new(self.build_info.clone()),
            Box::new(process::ProcessCollector::new()),
        ];
        for c in collectors {
            r.register(c).expect("metric families have unique names");
        }
    }

    fn zero_init(&self) {
        self.bandwidth_limit.set(f64::INFINITY);
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
        for d in RuleDecision::ALL {
            self.rule_decisions.with_label_values(&[d.as_str()]);
        }
        for s in QueueState::ALL {
            self.queue_items.with_label_values(&[s.as_str()]);
        }
        for r in FailReason::ALL {
            self.downloads_failed.with_label_values(&[r.as_str()]);
        }
        for o in ExistingOutcome::ALL {
            self.existing_files.with_label_values(&[o.as_str()]);
        }
        for t in CleanupTrigger::ALL {
            for r in SyncResult::ALL {
                self.empty_folder_runs
                    .with_label_values(&[t.as_str(), r.as_str()]);
                self.storage_free_runs
                    .with_label_values(&[t.as_str(), r.as_str()]);
            }
        }
        verify::zero_init(self);
        for via in tailscale::VIAS {
            for result in tailscale::RESULTS {
                self.tailscale_identity.with_label_values(&[via, result]);
            }
        }
        for t in hooks::EVENT_TYPES {
            self.events.with_label_values(&[t]);
        }
        for r in logs::DROP_REASONS {
            self.log_lines_dropped.with_label_values(&[r]);
        }
        for m in ArrMethod::ALL {
            for o in ArrOutcome::ALL {
                self.arr_rpc_requests
                    .with_label_values(&[m.as_str(), o.as_str()]);
            }
        }
        for s in ArrItemState::ALL {
            self.arr_items.with_label_values(&[s.as_str()]);
        }
        for (stage, states) in PIPELINE_STATES {
            for state in *states {
                self.pipeline_items
                    .with_label_values(&[stage.as_str(), state.as_str()]);
            }
        }
        for a in PipelineAction::ALL {
            self.pipeline_actions.with_label_values(&[a.as_str()]);
        }
        for stage in steps::STAGES {
            self.pipeline_items_waiting.with_label_values(&[stage]);
            for kind in steps::KINDS {
                self.pipeline_step_seconds.with_label_values(&[stage, kind]);
                for result in steps::RESULTS {
                    self.pipeline_steps
                        .with_label_values(&[stage, kind, result]);
                }
            }
        }
        for o in BlocklistOutcome::ALL {
            self.arr_blocklist.with_label_values(&[o.as_str()]);
        }
        for k in ArrSetupKind::ALL {
            for r in ArrSetupResult::ALL {
                self.arr_setup.with_label_values(&[k.as_str(), r.as_str()]);
            }
        }
        for r in ArrMappingResult::ALL {
            self.arr_path_mapping.with_label_values(&[r.as_str()]);
        }
        for source in PrecheckSource::ALL {
            for stage in PrecheckStage::ALL {
                for outcome in PrecheckOutcome::ALL {
                    self.precheck.with_label_values(&[
                        source.as_str(),
                        stage.as_str(),
                        outcome.as_str(),
                    ]);
                }
            }
        }
        for stage in PrecheckStage::ALL {
            self.precheck_avoided_bytes
                .with_label_values(&[stage.as_str()]);
        }
        for r in FallbackResult::ALL {
            self.precheck_fallback.with_label_values(&[r.as_str()]);
        }
        for k in FlagKind::ALL {
            self.release_flags.with_label_values(&[k.as_str()]);
        }
        for r in HoldResult::ALL {
            self.precheck_held.with_label_values(&[r.as_str()]);
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
