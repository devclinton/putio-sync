//! Per-worker metrics at `detailed` (US4, T040). Its own test binary, because it switches
//! the global registry to `detailed`.

use std::sync::Arc;
use std::time::Duration;

use putio_sync_client::PutioApiClient;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager};
use putio_sync_telemetry::{global, Detail};

const ROOT: &str = r#"{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false}"#;
const ENTRY: &str = r#"{"id":1,"name":"movie.mkv","size":8192,"file_type":"VIDEO","parent_id":0,"is_folder":false,"is_shared":false}"#;

fn worker0(name: &str) -> f64 {
    global().sample(name, &[("worker", "0")]).unwrap_or(-1.0)
}

async fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    for _ in 0..100 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {}", what);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn worker_is_busy_with_inflight_bytes_then_idle() {
    global().set_detail(Detail::Detailed);
    global().init(1, "test", "test");

    let mut server = mockito::Server::new_async().await;
    let _list = server
        .mock("GET", "/files/list?parent_id=0")
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"files":[{ENTRY}],"parent":{ROOT},"status":"OK"}}"#
        ))
        .create_async()
        .await;
    let _file = server
        .mock("GET", "/files/1")
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"file":{ENTRY},"status":"OK"}}"#))
        .create_async()
        .await;
    // Half the file, a pause long enough to observe the busy worker, then the rest.
    let _download = server
        .mock("GET", "/files/1/download")
        .with_chunked_body(|w| {
            w.write_all(&[7u8; 4096])?;
            w.flush()?;
            std::thread::sleep(Duration::from_millis(800));
            w.write_all(&[7u8; 4096])
        })
        .create_async()
        .await;

    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(PutioApiClient::with_base_url("tok".into(), server.url()));
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        workers: 1,
        max_retries: 0,
        retry_initial_delay_secs: 0,
        ..DownloadConfig::default()
    };
    let manager = DownloadManager::new(client, db, config);
    manager.sync_downloads().await.unwrap();
    assert_eq!(worker0("putio_sync_worker_state"), 0.0);

    manager.start_workers().await;
    wait_for("busy worker with bytes in flight", || {
        worker0("putio_sync_worker_state") == 1.0
            && worker0("putio_sync_worker_inflight_bytes") == 4096.0
    })
    .await;
    wait_for("worker back to idle", || {
        worker0("putio_sync_worker_state") == 0.0
    })
    .await;
    assert_eq!(worker0("putio_sync_worker_inflight_bytes"), 0.0);
    assert!(dir.path().join("movie.mkv").exists());

    manager.shutdown_all_workers().await;
}
