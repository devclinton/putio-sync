//! Watch-folder scans, uploads, cleanups, invalid files and cleanup failures are counted
//! (US1, T014). Tests share the global registry, so they run one at a time and compare
//! before/after values.

use putio_sync_client::PutioApiClient;
use putio_sync_db::Database;
use putio_sync_file_manager::{calculate_string_checksum, TorrentWatchCore};
use putio_sync_telemetry::global;
use std::path::PathBuf;

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const MAGNET: &str = "magnet:?xt=urn:btih:abc";
const TRANSFER: &str = r#"{"status":"OK","transfer":{"id":1,"name":"t","status":"IN_QUEUE",
    "created_at":"2026-01-01T00:00:00","save_parent_id":0,"is_private":false}}"#;

fn value(name: &str) -> f64 {
    global().sample(name, &[]).unwrap_or(0.0)
}

struct Counts {
    scans: f64,
    uploads: f64,
    cleanups: f64,
    invalid: f64,
    // Only the unix-only permissions test reads it
    #[cfg_attr(not(unix), allow(dead_code))]
    failures: f64,
}

fn counts() -> Counts {
    Counts {
        scans: value("putio_sync_watch_scans_total"),
        uploads: value("putio_sync_watch_uploads_total"),
        cleanups: value("putio_sync_watch_cleanups_total"),
        invalid: value("putio_sync_watch_invalid_total"),
        failures: value("putio_sync_watch_cleanup_failures_total"),
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    watch: PathBuf,
    db: Database,
    server: mockito::ServerGuard,
}

async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let watch = dir.path().join("watch");
    std::fs::create_dir(&watch).unwrap();
    let db = Database::new(dir.path().join(".putiosync.db"))
        .await
        .unwrap();
    Fixture {
        _dir: dir,
        watch,
        db,
        server: mockito::Server::new_async().await,
    }
}

async fn record(f: &Fixture, name: &str) {
    f.db.processed_torrents()
        .record_processed(
            f.watch.join(name).to_str().unwrap(),
            &calculate_string_checksum(MAGNET),
            MAGNET.len() as i64,
            Some(MAGNET),
        )
        .await
        .unwrap();
}

async fn scan(f: &Fixture, core: &TorrentWatchCore, delete: bool) {
    let client = PutioApiClient::with_base_url("test_token".to_string(), f.server.url());
    core.scan_directories(
        std::slice::from_ref(&f.watch),
        &client,
        None,
        None,
        delete,
        Some(&f.db),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn upload_and_scans_are_counted() {
    let _serial = SERIAL.lock().await;
    let mut f = fixture().await;
    std::fs::write(f.watch.join("new.magnet"), MAGNET).unwrap();
    let add = f
        .server
        .mock("POST", "/transfers/add")
        .with_status(200)
        .with_body(TRANSFER)
        .expect(1)
        .create_async()
        .await;

    let before = counts();
    let core = TorrentWatchCore::new();
    scan(&f, &core, true).await;
    scan(&f, &core, true).await;
    let after = counts();

    add.assert_async().await;
    assert_eq!(after.scans - before.scans, 2.0);
    assert_eq!(after.uploads - before.uploads, 1.0);
}

#[tokio::test]
async fn already_uploaded_cleanup_is_counted() {
    let _serial = SERIAL.lock().await;
    let f = fixture().await;
    std::fs::write(f.watch.join("x.magnet"), MAGNET).unwrap();
    record(&f, "x.magnet").await;

    let before = counts();
    scan(&f, &TorrentWatchCore::new(), true).await;
    let after = counts();

    assert_eq!(after.cleanups - before.cleanups, 1.0);
    assert_eq!(after.uploads - before.uploads, 0.0);
    assert_eq!(after.scans - before.scans, 1.0);
}

#[tokio::test]
async fn invalid_file_is_counted_once() {
    let _serial = SERIAL.lock().await;
    let f = fixture().await;
    std::fs::write(f.watch.join("bad.magnet"), "not a magnet").unwrap();

    let before = counts();
    let core = TorrentWatchCore::new();
    scan(&f, &core, true).await;
    scan(&f, &core, true).await;
    let after = counts();

    assert_eq!(after.invalid - before.invalid, 1.0);
    assert_eq!(after.scans - before.scans, 2.0);
}

#[cfg(unix)]
#[tokio::test]
async fn cleanup_failure_is_counted() {
    use std::os::unix::fs::PermissionsExt;

    let _serial = SERIAL.lock().await;
    let f = fixture().await;
    std::fs::write(f.watch.join("x.magnet"), MAGNET).unwrap();
    record(&f, "x.magnet").await;
    std::fs::set_permissions(&f.watch, std::fs::Permissions::from_mode(0o555)).unwrap();

    let before = counts();
    scan(&f, &TorrentWatchCore::new(), true).await;
    let after = counts();

    std::fs::set_permissions(&f.watch, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(after.failures - before.failures, 1.0);
    assert_eq!(after.cleanups - before.cleanups, 0.0);
}
