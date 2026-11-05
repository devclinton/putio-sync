//! The hooks service (feature 027, T007).
#![cfg(unix)]

mod common;

use std::time::{Duration, Instant};

use common::*;
use putio_sync_core::events::{self, EventType, HookTest, Payload, StorageLow};
use putio_sync_hooks::{HooksService, RunResult, TestError};

fn storage_low() -> Payload {
    Payload::StorageLow(StorageLow {
        total_bytes: 100,
        used_bytes: 95,
        free_bytes: 5,
        free_percent: 5.0,
        warn_free_percent: 10,
    })
}

/// The bus is shared by every test in this binary, so each test's script records only
/// events for its own marker file and checks by event id.
async fn wait_for(path: &std::path::Path, ours: &[String], within: Duration) -> Vec<String> {
    let lines = ours.len();
    let start = Instant::now();
    loop {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let got: Vec<String> = text
            .lines()
            .filter(|l| ours.iter().any(|o| o == l))
            .map(str::to_string)
            .collect();
        if got.len() >= lines || start.elapsed() > within {
            return got;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_matching_hook_runs_quickly_and_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("ids");
    let program = script(
        dir.path(),
        "log.sh",
        &format!("echo \"$PUTIO_SYNC_EVENT_ID\" >> {}", log.display()),
    );
    let mut hook = command_hook("order", &program);
    hook.events = vec!["storage.low".to_string()];
    let service = HooksService::new(vec![hook]);
    service.start();
    assert!(service.wants(EventType::StorageLow));
    assert!(!service.wants(EventType::DownloadFinished));

    let start = Instant::now();
    let first = events::emit(storage_low());
    let got = wait_for(
        &log,
        std::slice::from_ref(&first.id),
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(got, vec![first.id.clone()]);
    // The goal is 2 s; a full `make ci` run has taken 2.1 s to start the shell
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "{:?}",
        start.elapsed()
    );

    // Not matching: never run.
    events::emit(Payload::HookTest(HookTest {
        hook: "order".to_string(),
        message: "x".to_string(),
    }));
    let ids: Vec<String> = (0..10)
        .map(|_| events::emit(storage_low()).id.clone())
        .collect();
    let got = wait_for(&log, &ids, Duration::from_secs(10)).await;
    assert_eq!(got, ids);
    let status = &service.status()[0];
    assert!(status.runs.iter().all(|r| r.result == RunResult::Success));
    assert!(status.runs.iter().all(|r| r.event_type == "storage.low"));
}

#[tokio::test]
async fn a_hanging_hook_does_not_block_another() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("fast");
    let slow = script(dir.path(), "slow.sh", "sleep 30");
    let fast = script(
        dir.path(),
        "fast.sh",
        &format!("echo \"$PUTIO_SYNC_EVENT_ID\" >> {}", log.display()),
    );
    let mut slow = command_hook("slow", &slow);
    slow.events = vec!["storage.low".to_string()];
    let mut fast = command_hook("fast", &fast);
    fast.events = vec!["storage.low".to_string()];
    let service = HooksService::new(vec![slow, fast]);
    service.start();

    let start = Instant::now();
    let a = events::emit(storage_low()).id.clone();
    let b = events::emit(storage_low()).id.clone();
    assert!(start.elapsed() < Duration::from_millis(100));
    let got = wait_for(&log, &[a.clone(), b.clone()], Duration::from_secs(5)).await;
    assert_eq!(got, vec![a, b]);
    let slow = service
        .status()
        .into_iter()
        .find(|s| s.name == "slow")
        .unwrap();
    assert!(slow.queued >= 1);
}

#[tokio::test]
async fn reconfigure_keeps_history_and_disabled_hooks_do_not_run() {
    let dir = tempfile::tempdir().unwrap();
    let program = script(dir.path(), "ok.sh", "exit 0");
    let mut hook = command_hook("keep", &program);
    hook.events = vec!["transfer.added".to_string()];
    let service = HooksService::new(vec![hook.clone()]);
    service.start();
    let run = service.test("keep").await.unwrap();
    assert_eq!(run.result, RunResult::Success);
    assert_eq!(run.event_type, "hook.test");

    let mut off = hook.clone();
    off.enabled = false;
    let mut other = command_hook("other", &program);
    other.events = vec!["transfer.added".to_string()];
    service.configure(vec![off, other]);
    let status = service.status();
    assert_eq!(status.len(), 2);
    assert_eq!(status[0].runs.len(), 1);
    assert!(!status[0].enabled);
    assert_eq!(status[0].target, program.display().to_string());
    assert!(service.wants(EventType::TransferAdded));
    assert!(!service.wants(EventType::StorageLow));

    // A disabled hook can still be tested.
    assert_eq!(
        service.test("keep").await.unwrap().result,
        RunResult::Success
    );
    assert_eq!(service.test("nope").await.unwrap_err(), TestError::Unknown);
}

#[tokio::test]
async fn a_second_test_of_the_same_hook_is_busy() {
    let dir = tempfile::tempdir().unwrap();
    let program = script(dir.path(), "slow.sh", "sleep 1");
    let service = HooksService::new(vec![command_hook("busy", &program)]);
    let first = tokio::spawn({
        let service = service.clone();
        async move { service.test("busy").await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(service.test("busy").await.unwrap_err(), TestError::Busy);
    assert!(first.await.unwrap().is_ok());
    assert!(service.test("busy").await.is_ok());
}

#[tokio::test]
async fn a_full_queue_drops_the_oldest() {
    let dir = tempfile::tempdir().unwrap();
    let program = script(dir.path(), "slow.sh", "sleep 30");
    let mut hook = command_hook("full", &program);
    hook.events = vec!["download.failed".to_string()];
    let service = HooksService::new(vec![hook]);
    service.start();
    let failed = || {
        Payload::DownloadFailed(events::DownloadFailed {
            file_id: 1,
            name: "a".to_string(),
            relative_path: "a".to_string(),
            size_bytes: 1,
            error: "x".to_string(),
            reason: "other".to_string(),
            attempts: 1,
        })
    };
    // One running, 1000 queued, 5 dropped.
    for _ in 0..1006 {
        events::emit(failed());
        tokio::task::yield_now().await;
    }
    let start = Instant::now();
    while service.status()[0].queued < 1000 && start.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(service.status()[0].queued, 1000);
}
