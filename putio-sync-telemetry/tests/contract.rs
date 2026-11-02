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
    "putio_sync_rule_decisions_total",
    "putio_sync_sync_in_progress",
    "putio_sync_queue_items",
    "putio_sync_downloads_active",
    "putio_sync_download_workers",
    "putio_sync_downloads_completed_total",
    "putio_sync_downloads_failed_total",
    "putio_sync_existing_files_total",
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
    "putio_sync_transfers_stalled",
    "putio_sync_pipeline_items",
    "putio_sync_pipeline_actions_total",
    "putio_sync_nas_downloads_paused",
    "putio_sync_arr_rpc_requests_total",
    "putio_sync_arr_items",
    "putio_sync_arr_blocklist_total",
    "putio_sync_arr_setup_total",
    "putio_sync_precheck_total",
    "putio_sync_precheck_avoided_bytes_total",
    "putio_sync_precheck_cache_lookups_total",
    "putio_sync_precheck_cache_lookup_seconds",
    "putio_sync_precheck_fallback_total",
    "putio_sync_precheck_fallback_putio_bytes_total",
    "putio_sync_release_flags_total",
    "putio_sync_precheck_held_total",
    "putio_sync_precheck_held",
    "putio_sync_precheck_held_seconds",
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
    t.sync_rule_decision(RuleDecision::Deleted);
    {
        let _active = t.download_active();
        t.download_bytes(4096);
        t.worker_busy(0, true);
        t.worker_inflight(0, 4096);
        t.worker_busy(0, false);
    }
    t.download_completed(Duration::from_secs(2));
    t.download_failed(FailReason::Integrity);
    t.download_existing(ExistingOutcome::SavedAs);
    t.watch_scan();
    t.watch_uploaded();
    t.watch_cleaned_up();
    t.watch_invalid_file();
    t.watch_cleanup_failed();
    t.empty_folder_run(CleanupTrigger::Schedule, SyncResult::Success);
    t.empty_folders_deleted(3);
    t.empty_folder_delete_failed();
    t.arr_rpc_request(ArrMethod::TorrentAdd, ArrOutcome::AuthFailed);
    t.set_arr_items(ArrItemState::Done, 2);
    t.arr_blocklist(BlocklistOutcome::Error);
    t.arr_setup(ArrSetupKind::Radarr, ArrSetupResult::Created);
    t.precheck(
        PrecheckSource::Torrent,
        PrecheckStage::Before,
        PrecheckOutcome::Rejected,
    );
    t.precheck_avoided(PrecheckStage::Before, 1_500_000_000);
    t.precheck_caches(&["itorrents.net".to_string()]);
    t.precheck_cache_lookup("itorrents.net", LookupResult::Found);
    t.precheck_lookup_time(Duration::from_millis(300));
    t.precheck_fallback(FallbackResult::Rejected, 1024);
    t.release_flag(FlagKind::Fake);
    t.precheck_hold_ended(HoldResult::FoundPassed, Duration::from_secs(90));
    t.set_precheck_held(1);
    t.set_transfers_stalled(2);
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
        "outcome" => {
            let mut v = strs(ApiOutcome::ALL, ApiOutcome::as_str);
            v.extend(strs(ArrOutcome::ALL, ArrOutcome::as_str));
            v.extend(strs(BlocklistOutcome::ALL, BlocklistOutcome::as_str));
            v.extend(strs(PrecheckOutcome::ALL, PrecheckOutcome::as_str));
            v.extend(strs(ExistingOutcome::ALL, ExistingOutcome::as_str));
            v
        }
        "source" => strs(PrecheckSource::ALL, PrecheckSource::as_str),
        "stage" => {
            let mut v = strs(PrecheckStage::ALL, PrecheckStage::as_str);
            v.extend(strs(PipelineStage::ALL, PipelineStage::as_str));
            v
        }
        // Host names from the config's cache list
        "cache" => vec!["itorrents.net"],
        "method" => strs(ArrMethod::ALL, ArrMethod::as_str),
        "reason" => {
            let mut v = strs(RetryReason::ALL, RetryReason::as_str);
            v.extend(strs(FailReason::ALL, FailReason::as_str));
            v
        }
        "result" => {
            let mut v = strs(SyncResult::ALL, SyncResult::as_str);
            v.extend(strs(ArrSetupResult::ALL, ArrSetupResult::as_str));
            v.extend(strs(LookupResult::ALL, LookupResult::as_str));
            v.extend(strs(FallbackResult::ALL, FallbackResult::as_str));
            v.extend(strs(HoldResult::ALL, HoldResult::as_str));
            v
        }
        "kind" => {
            let mut v = strs(SyncKind::ALL, SyncKind::as_str);
            v.extend(strs(ArrSetupKind::ALL, ArrSetupKind::as_str));
            v.extend(strs(FlagKind::ALL, FlagKind::as_str));
            v
        }
        "state" => {
            let mut v = strs(QueueState::ALL, QueueState::as_str);
            v.extend(strs(ArrItemState::ALL, ArrItemState::as_str));
            v.extend(strs(PipelineState::ALL, PipelineState::as_str));
            v
        }
        "action" => strs(PipelineAction::ALL, PipelineAction::as_str),
        "trigger" => strs(CleanupTrigger::ALL, CleanupTrigger::as_str),
        "decision" => strs(RuleDecision::ALL, RuleDecision::as_str),
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

/// Feature 007: the stalled-transfer gauge starts at 0 and holds the last count
#[test]
fn transfers_stalled_reads_the_last_count() {
    let t = Telemetry::new();
    assert_eq!(t.sample("putio_sync_transfers_stalled", &[]), Some(0.0));
    t.set_transfers_stalled(3);
    assert_eq!(t.sample("putio_sync_transfers_stalled", &[]), Some(3.0));
    t.set_transfers_stalled(0);
    assert_eq!(t.sample("putio_sync_transfers_stalled", &[]), Some(0.0));
}

/// Feature 039: pipeline gauges start at zero for every stage and state, and hold the last
/// values set
#[test]
fn pipeline_metrics() {
    let t = Telemetry::new();
    let items = |stage, state| {
        t.sample(
            "putio_sync_pipeline_items",
            &[("stage", stage), ("state", state)],
        )
    };
    for (stage, states) in PIPELINE_STATES {
        for state in *states {
            assert_eq!(items(stage.as_str(), state.as_str()), Some(0.0));
        }
    }
    assert_eq!(items("putio", "waiting_for_sync"), None);
    assert_eq!(t.sample("putio_sync_nas_downloads_paused", &[]), Some(0.0));

    t.set_pipeline_items(PipelineStage::Nas, PipelineState::Downloading, 4);
    assert_eq!(items("nas", "downloading"), Some(4.0));
    assert_eq!(items("putio", "downloading"), Some(0.0));

    t.pipeline_action(PipelineAction::Hold);
    t.pipeline_action(PipelineAction::Hold);
    assert_eq!(
        t.sample("putio_sync_pipeline_actions_total", &[("action", "hold")]),
        Some(2.0)
    );
    assert_eq!(
        t.sample("putio_sync_pipeline_actions_total", &[("action", "pause")]),
        Some(0.0)
    );

    t.set_nas_downloads_paused(true);
    assert_eq!(t.sample("putio_sync_nas_downloads_paused", &[]), Some(1.0));
}

/// Feature 010: doctor series appear after the first run and hold the last result
#[test]
fn doctor_metrics() {
    let t = Telemetry::new();
    let before = t.encode();
    assert!(!before.contains("putio_sync_doctor_check_status{"));
    assert!(!before.contains("\nputio_sync_doctor_last_run_timestamp_seconds "));

    t.doctor_check("dead_queue_rows", 1.0, 3);
    t.doctor_check("auth", -1.0, 0);
    t.doctor_run(Duration::from_millis(1500));
    let status = |c| t.sample("putio_sync_doctor_check_status", &[("check", c)]);
    assert_eq!(status("dead_queue_rows"), Some(1.0));
    assert_eq!(status("auth"), Some(-1.0));
    assert_eq!(
        t.sample(
            "putio_sync_doctor_check_items",
            &[("check", "dead_queue_rows")]
        ),
        Some(3.0)
    );
    assert_eq!(
        t.sample("putio_sync_doctor_last_run_duration_seconds", &[]),
        Some(1.5)
    );
    assert!(t
        .sample("putio_sync_doctor_last_run_timestamp_seconds", &[])
        .is_some_and(|v| v > 0.0));

    t.doctor_check("dead_queue_rows", 0.0, 0);
    assert_eq!(status("dead_queue_rows"), Some(0.0));
}

/// Feature 011: the *arr families start at 0 for every label value
#[test]
fn arr_metrics() {
    let t = Telemetry::new();
    let rpc = |method, outcome| {
        t.sample(
            "putio_sync_arr_rpc_requests_total",
            &[("method", method), ("outcome", outcome)],
        )
    };
    assert_eq!(rpc("torrent_add", "auth_failed"), Some(0.0));
    assert_eq!(
        t.sample("putio_sync_arr_items", &[("state", "failed")]),
        Some(0.0)
    );
    assert_eq!(
        t.sample(
            "putio_sync_arr_blocklist_total",
            &[("outcome", "not_found")]
        ),
        Some(0.0)
    );

    t.arr_rpc_request(ArrMethod::from_method("torrent-add"), ArrOutcome::Ok);
    t.arr_rpc_request(ArrMethod::from_method("port-test"), ArrOutcome::Error);
    t.set_arr_items(ArrItemState::Local, 3);
    t.arr_blocklist(BlocklistOutcome::Ok);
    assert_eq!(rpc("torrent_add", "ok"), Some(1.0));
    assert_eq!(rpc("other", "error"), Some(1.0));
    assert_eq!(
        t.sample("putio_sync_arr_items", &[("state", "local")]),
        Some(3.0)
    );
    assert_eq!(
        t.sample("putio_sync_arr_blocklist_total", &[("outcome", "ok")]),
        Some(1.0)
    );
}

/// Feature 042: the pre-check families start at 0 for every closed label value, and the
/// cache series once the cache list is known
#[test]
fn precheck_metrics() {
    let t = Telemetry::new();
    let check = |source, stage, outcome| {
        t.sample(
            "putio_sync_precheck_total",
            &[("source", source), ("stage", stage), ("outcome", outcome)],
        )
    };
    assert_eq!(check("watch_magnet", "putio", "unknown"), Some(0.0));
    assert_eq!(
        t.sample("putio_sync_release_flags_total", &[("kind", "not_fake")]),
        Some(0.0)
    );
    let lookups = |result| {
        t.sample(
            "putio_sync_precheck_cache_lookups_total",
            &[("cache", "itorrents.net"), ("result", result)],
        )
    };
    assert_eq!(lookups("timeout"), None);
    t.precheck_caches(&["itorrents.net".to_string()]);
    assert_eq!(lookups("timeout"), Some(0.0));

    t.precheck(
        PrecheckSource::Url,
        PrecheckStage::Before,
        PrecheckOutcome::Rejected,
    );
    t.precheck_avoided(PrecheckStage::Before, 2048);
    t.precheck_cache_lookup("itorrents.net", LookupResult::WrongHash);
    t.precheck_fallback(FallbackResult::Passed, 999);
    t.precheck_fallback(FallbackResult::Rejected, 1000);
    t.release_flag(FlagKind::Fake);
    assert_eq!(check("url", "before", "rejected"), Some(1.0));
    assert_eq!(
        t.sample(
            "putio_sync_precheck_avoided_bytes_total",
            &[("stage", "before")]
        ),
        Some(2048.0)
    );
    assert_eq!(lookups("wrong_hash"), Some(1.0));
    assert_eq!(
        t.sample(
            "putio_sync_precheck_fallback_total",
            &[("result", "passed")]
        ),
        Some(1.0)
    );
    // Only rejected fallbacks count their Put.io bytes
    assert_eq!(
        t.sample("putio_sync_precheck_fallback_putio_bytes_total", &[]),
        Some(1000.0)
    );
    assert_eq!(
        t.sample("putio_sync_release_flags_total", &[("kind", "fake")]),
        Some(1.0)
    );
}
