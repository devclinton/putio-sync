//! Sync and download metrics (T013). Tests in this file share the global registry, so
//! they run one at a time and compare before/after values.

use std::sync::Arc;

use putio_sync_client::PutioApiClient;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager};
use putio_sync_telemetry::global;

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const ROOT: &str = r#"{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false}"#;

fn entry(id: i64, name: &str, size: u64) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","size":{size},"file_type":"VIDEO","parent_id":0,"is_folder":false,"is_shared":false}}"#
    )
}

fn value(name: &str, labels: &[(&str, &str)]) -> f64 {
    global().sample(name, labels).unwrap_or(0.0)
}

struct Snapshot {
    runs: f64,
    blocked: f64,
    completed: f64,
    bytes: f64,
    integrity: f64,
}

fn snapshot() -> Snapshot {
    Snapshot {
        runs: value("putio_sync_sync_runs_total", &[("result", "success")]),
        blocked: value("putio_sync_files_blocked_total", &[]),
        completed: value("putio_sync_downloads_completed_total", &[]),
        bytes: value("putio_sync_downloaded_bytes_total", &[]),
        integrity: value(
            "putio_sync_downloads_failed_total",
            &[("reason", "integrity")],
        ),
    }
}

/// Mock Put.io with one file `id` of `size` bytes whose download returns `body` bytes,
/// plus one blocked `.exe`.
async fn setup(
    id: i64,
    size: u64,
    body: usize,
) -> (
    mockito::ServerGuard,
    DownloadManager,
    tempfile::TempDir,
    Vec<mockito::Mock>,
) {
    let mut server = mockito::Server::new_async().await;
    let list = format!(
        r#"{{"files":[{},{}],"parent":{ROOT},"status":"OK"}}"#,
        entry(id, "movie.mkv", size),
        entry(id + 1, "setup.exe", 100),
    );
    let mocks = vec![
        server
            .mock("GET", "/files/list?parent_id=0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(list)
            .create_async()
            .await,
        server
            .mock("GET", format!("/files/{id}").as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{"file":{},"status":"OK"}}"#,
                entry(id, "movie.mkv", size)
            ))
            .create_async()
            .await,
        server
            .mock("GET", format!("/files/{id}/download").as_str())
            .with_status(200)
            .with_body(vec![7u8; body])
            .create_async()
            .await,
    ];

    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(PutioApiClient::with_base_url(
        "tok_SECRET".into(),
        server.url(),
    ));
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        max_retries: 0,
        retry_initial_delay_secs: 0,
        ..DownloadConfig::default()
    };
    let manager = DownloadManager::new(client, db, config);
    (server, manager, dir, mocks)
}

#[tokio::test]
async fn sync_and_download_are_counted() {
    let _serial = SERIAL.lock().await;
    let (_server, manager, dir, _mocks) = setup(1, 4096, 4096).await;
    let before = snapshot();

    for _ in 0..2 {
        manager.sync_downloads().await.unwrap();
    }
    assert!(manager.process_queue().await.unwrap());

    let after = snapshot();
    assert_eq!(after.runs - before.runs, 2.0);
    assert_eq!(
        value("putio_sync_last_sync_files", &[("kind", "blocked")]),
        1.0
    );
    assert_eq!(after.blocked - before.blocked, 1.0, "once per file id");
    assert_eq!(after.completed - before.completed, 1.0);
    assert_eq!(after.bytes - before.bytes, 4096.0);
    assert_eq!(value("putio_sync_downloads_active", &[]), 0.0);
    assert!(value("putio_sync_last_success_timestamp_seconds", &[]) > 0.0);
    assert!(dir.path().join("movie.mkv").exists());
}

#[tokio::test]
async fn size_mismatch_is_an_integrity_failure() {
    let _serial = SERIAL.lock().await;
    let (_server, manager, _dir, _mocks) = setup(10, 5000, 4096).await;
    let before = snapshot();

    manager.sync_downloads().await.unwrap();
    manager.process_queue().await.unwrap();

    let after = snapshot();
    assert_eq!(after.integrity - before.integrity, 1.0);
    assert_eq!(after.completed, before.completed);
    assert_eq!(value("putio_sync_downloads_active", &[]), 0.0);
}

/// SC-002: telemetry adds ≤2% to download time. Recording can't be switched off (only the
/// listener can), so this times a 64 MiB stream, then times the telemetry calls that
/// stream makes, assuming 4 KiB chunks (more calls than real reads produce).
/// Run with `cargo test -p putio-sync-download --release --test metrics_sync -- --ignored --nocapture`.
#[tokio::test]
#[ignore]
async fn telemetry_overhead_is_under_two_percent() {
    const SIZE: usize = 64 * 1024 * 1024;
    let _serial = SERIAL.lock().await;
    let (_server, manager, _dir, _mocks) = setup(100, SIZE as u64, SIZE).await;
    manager.sync_downloads().await.unwrap();

    let started = std::time::Instant::now();
    assert!(manager.process_queue().await.unwrap());
    let download = started.elapsed();

    let chunks = SIZE / 4096;
    let started = std::time::Instant::now();
    for i in 0..chunks {
        global().download_bytes(4096);
        global().worker_inflight(0, (i * 4096) as u64);
    }
    let telemetry = started.elapsed();

    let share = telemetry.as_secs_f64() / download.as_secs_f64() * 100.0;
    println!(
        "64 MiB download {:?}, {} chunk updates {:?}: {:.3}% of download time",
        download, chunks, telemetry, share
    );
    assert!(share <= 2.0, "telemetry took {:.3}%", share);
}
