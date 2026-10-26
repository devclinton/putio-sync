//! Contract test for `specs/003-telemetry-prometheus/contracts/metrics.md`, invariants 1–5.

use std::collections::BTreeMap;
use std::time::Duration;

use putio_sync_telemetry::*;

const TOKEN: &str = "tok_SECRET";
const FIXTURE_NAMES: &[&str] = &["movie.mkv", "setup.exe", "ubuntu.torrent"];

const BASIC_FAMILIES: &[&str] = &[
    "putio_sync_api_requests_total",
    "putio_sync_api_retries_total",
    "putio_sync_api_request_duration_seconds",
    "putio_sync_sync_runs_total",
    "putio_sync_sync_duration_seconds",
    "putio_sync_last_sync_duration_seconds",
    "putio_sync_last_success_timestamp_seconds",
    "putio_sync_last_sync_files",
    "putio_sync_files_blocked_total",
    "putio_sync_queue_items",
    "putio_sync_downloads_active",
    "putio_sync_download_workers",
    "putio_sync_downloads_completed_total",
    "putio_sync_downloads_failed_total",
    "putio_sync_downloaded_bytes_total",
    "putio_sync_download_duration_seconds",
    "putio_sync_watch_scans_total",
    "putio_sync_watch_uploads_total",
    "putio_sync_watch_cleanups_total",
    "putio_sync_watch_invalid_total",
    "putio_sync_watch_cleanup_failures_total",
    "putio_sync_empty_folder_runs_total",
    "putio_sync_empty_folders_deleted_total",
    "putio_sync_empty_folder_delete_failures_total",
    "putio_sync_empty_folder_last_success_timestamp_seconds",
    "putio_sync_db_errors_total",
    "putio_sync_disk_free_bytes",
    "putio_sync_disk_total_bytes",
    "putio_sync_start_time_seconds",
    "putio_sync_build_info",
];

const DETAILED_FAMILIES: &[&str] = &[
    "putio_sync_api_operation_duration_seconds",
    "putio_sync_worker_state",
    "putio_sync_worker_inflight_bytes",
];

/// One parsed sample line: metric name and its labels.
struct Sample {
    name: String,
    labels: BTreeMap<String, String>,
    value: f64,
}

fn parse(output: &str) -> Vec<Sample> {
    output
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|line| {
            let (head, value) = line.rsplit_once(' ').unwrap();
            let (name, labels) = match head.split_once('{') {
                Some((name, rest)) => {
                    let body = rest.strip_suffix('}').unwrap();
                    let labels = body
                        .split("\",")
                        .filter(|kv| !kv.is_empty())
                        .map(|kv| {
                            let (k, v) = kv.split_once("=\"").unwrap();
                            (k.to_string(), v.trim_end_matches('"').to_string())
                        })
                        .collect();
                    (name.to_string(), labels)
                }
                None => (head.to_string(), BTreeMap::new()),
            };
            Sample {
                name,
                labels,
                value: value.parse().unwrap(),
            }
        })
        .collect()
}

fn family_of(sample: &str) -> &str {
    for suffix in ["_bucket", "_sum", "_count"] {
        if let Some(base) = sample.strip_suffix(suffix) {
            if base.ends_with("_seconds") {
                return base;
            }
        }
    }
    sample
}

fn families(output: &str) -> Vec<String> {
    let mut names: Vec<String> = parse(output)
        .iter()
        .map(|s| family_of(&s.name).to_string())
        .collect();
    names.dedup();
    names
}

fn record_everything(t: &Telemetry) {
    for op in ApiOp::ALL {
        t.api_attempt(*op, ApiOutcome::Success, Duration::from_millis(80));
    }
    t.api_attempt(
        ApiOp::ListFiles,
        ApiOutcome::ServerError,
        Duration::from_millis(5),
    );
    t.api_retry(RetryReason::ServerError);
    t.sync_run(
        SyncResult::Success,
        Duration::from_secs(3),
        &[
            (SyncKind::Found, 3),
            (SyncKind::Queued, 1),
            (SyncKind::Skipped, 0),
            (SyncKind::Blocked, 1),
            (SyncKind::Failed, 1),
        ],
    );
    t.sync_file_blocked();
    {
        let _active = t.download_active();
        t.download_bytes(4096);
        t.worker_busy(0, true);
        t.worker_inflight(0, 4096);
        t.worker_busy(0, false);
    }
    t.download_completed(Duration::from_secs(2));
    t.download_failed(FailReason::Integrity);
    t.watch_scan();
    t.watch_uploaded();
    t.watch_cleaned_up();
    t.watch_invalid_file();
    t.watch_cleanup_failed();
    t.empty_folder_run(CleanupTrigger::Schedule, SyncResult::Success);
    t.empty_folders_deleted(3);
    t.empty_folder_delete_failed();
    t.db_error(DbOp::Queue);
    t.set_queue_counts(&[(QueueState::Pending, 2)]);
    let dir = tempfile::tempdir().unwrap();
    t.refresh_disk(dir.path()).unwrap();
}

fn closed_set(label: &str) -> Vec<&'static str> {
    fn strs<T: Copy>(all: &[T], f: fn(&T) -> &'static str) -> Vec<&'static str> {
        all.iter().map(f).collect()
    }
    match label {
        "operation" => {
            let mut v = strs(ApiOp::ALL, ApiOp::as_str);
            v.extend(strs(DbOp::ALL, DbOp::as_str));
            v
        }
        "outcome" => strs(ApiOutcome::ALL, ApiOutcome::as_str),
        "reason" => {
            let mut v = strs(RetryReason::ALL, RetryReason::as_str);
            v.extend(strs(FailReason::ALL, FailReason::as_str));
            v
        }
        "result" => strs(SyncResult::ALL, SyncResult::as_str),
        "kind" => strs(SyncKind::ALL, SyncKind::as_str),
        "state" => strs(QueueState::ALL, QueueState::as_str),
        "trigger" => strs(CleanupTrigger::ALL, CleanupTrigger::as_str),
        other => panic!("unexpected label {}", other),
    }
}

#[test]
fn basic_output_meets_the_contract() {
    let t = Telemetry::new();
    t.set_detail(Detail::Basic);
    t.init(2, "1.2.3", "abc1234");

    // Absent until first set.
    let before = t.encode();
    for name in [
        "putio_sync_last_success_timestamp_seconds",
        "putio_sync_empty_folder_last_success_timestamp_seconds",
        "putio_sync_disk_free_bytes",
        "putio_sync_disk_total_bytes",
    ] {
        assert!(
            !before.contains(&format!("\n{} ", name)),
            "{} present early",
            name
        );
    }

    record_everything(&t);
    let output = t.encode();
    let names = families(&output);

    // 1. Every basic family is present.
    for family in BASIC_FAMILIES {
        assert!(names.iter().any(|n| n == family), "missing {}", family);
    }

    // 2. No detailed-only family or worker label at basic.
    for family in DETAILED_FAMILIES {
        assert!(
            !names.iter().any(|n| n == family),
            "{} present at basic",
            family
        );
    }
    let samples = parse(&output);
    assert!(samples.iter().all(|s| !s.labels.contains_key("worker")));

    // 3 and 4. Closed label sets; nothing sensitive.
    for s in &samples {
        for (k, v) in &s.labels {
            if k == "le" || s.name == "putio_sync_build_info" {
                continue;
            }
            assert!(
                closed_set(k).contains(&v.as_str()),
                "{}={} on {}",
                k,
                v,
                s.name
            );
            assert!(!v.contains("://") && !v.contains('/'), "{}={}", k, v);
        }
    }
    assert!(!output.contains(TOKEN));
    for name in FIXTURE_NAMES {
        assert!(!output.contains(name));
    }
}

#[test]
fn detailed_output_adds_worker_and_operation_series() {
    let t = Telemetry::new();
    t.set_detail(Detail::Detailed);
    t.init(2, "1.2.3", "abc1234");
    record_everything(&t);
    let output = t.encode();
    let names = families(&output);
    for family in DETAILED_FAMILIES {
        assert!(names.iter().any(|n| n == family), "missing {}", family);
    }
    assert!(output.contains("putio_sync_worker_state{worker=\"1\"} 0"));
    assert!(!output.contains("worker=\"2\""));
}

#[test]
fn counters_never_decrease() {
    let t = Telemetry::new();
    t.init(1, "1.2.3", "abc1234");
    record_everything(&t);
    let first = parse(&t.encode());
    t.init(1, "1.2.3", "abc1234");
    record_everything(&t);
    let second = parse(&t.encode());
    for s in first.iter().filter(|s| s.name.ends_with("_total")) {
        let later = second
            .iter()
            .find(|o| o.name == s.name && o.labels == s.labels)
            .unwrap();
        assert!(later.value >= s.value, "{} decreased", s.name);
    }
}

#[test]
fn detailed_creates_one_series_per_worker_and_feeds_operation_latency() {
    let t = Telemetry::new();
    t.set_detail(Detail::Detailed);
    t.init(3, "1.2.3", "abc1234");
    for id in ["0", "1", "2"] {
        assert_eq!(
            t.sample("putio_sync_worker_state", &[("worker", id)]),
            Some(0.0),
            "worker {}",
            id
        );
    }
    assert_eq!(
        t.sample("putio_sync_worker_state", &[("worker", "3")]),
        None
    );

    let op = [("operation", "list_files")];
    assert_eq!(
        t.sample("putio_sync_api_operation_duration_seconds", &op),
        None
    );
    t.api_attempt(
        ApiOp::ListFiles,
        ApiOutcome::Success,
        Duration::from_millis(80),
    );
    assert_eq!(
        t.sample("putio_sync_api_operation_duration_seconds", &op),
        Some(1.0)
    );
}

#[test]
fn empty_folder_cleanup_metrics() {
    let t = Telemetry::new();
    let runs = |trigger, result| {
        t.sample(
            "putio_sync_empty_folder_runs_total",
            &[("trigger", trigger), ("result", result)],
        )
    };
    // Every trigger × result series exists before the first run
    assert_eq!(runs("manual", "failure"), Some(0.0));
    assert_eq!(
        t.sample(
            "putio_sync_empty_folder_last_success_timestamp_seconds",
            &[]
        ),
        None
    );

    t.empty_folder_run(CleanupTrigger::Manual, SyncResult::Failure);
    assert_eq!(runs("manual", "failure"), Some(1.0));
    assert_eq!(
        t.sample(
            "putio_sync_empty_folder_last_success_timestamp_seconds",
            &[]
        ),
        None
    );

    t.empty_folder_run(CleanupTrigger::Schedule, SyncResult::Success);
    t.empty_folders_deleted(4);
    t.empty_folder_delete_failed();
    assert_eq!(runs("schedule", "success"), Some(1.0));
    assert!(
        t.sample(
            "putio_sync_empty_folder_last_success_timestamp_seconds",
            &[]
        )
        .unwrap()
            > 0.0
    );
    assert_eq!(
        t.sample("putio_sync_empty_folders_deleted_total", &[]),
        Some(4.0)
    );
    assert_eq!(
        t.sample("putio_sync_empty_folder_delete_failures_total", &[]),
        Some(1.0)
    );
}
