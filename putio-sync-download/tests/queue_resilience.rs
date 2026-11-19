//! How the queue worker treats failures: a row is dropped only when Put.io says the file is
//! gone, a transient failure is queued again by a later sync, and a stopped file keeps its
//! row. Small payloads against mockito; nothing reaches Put.io.

use std::sync::Arc;
use std::time::Duration;

use mockito::ServerGuard;
use putio_sync_client::PutioApiClient;
use putio_sync_db::models::QueueStatus;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager};

const SIZE: usize = 4096;

struct Setup {
    server: ServerGuard,
    _dir: tempfile::TempDir,
    db: Arc<Database>,
    manager: DownloadManager,
}

async fn setup() -> Setup {
    let server = mockito::Server::new_async().await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(PutioApiClient::with_base_url("tok".into(), server.url()));
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        max_retries: 0,
        retry_initial_delay_secs: 0,
        ..DownloadConfig::default()
    };
    let manager = DownloadManager::new(client, db.clone(), config);
    db.queue()
        .add_with_path_kept(
            1,
            "movie.mkv",
            SIZE as i64,
            "",
            "",
            0,
            QueueStatus::Pending,
            false,
        )
        .await
        .unwrap();
    Setup {
        server,
        _dir: dir,
        db,
        manager,
    }
}

impl Setup {
    async fn status(&self) -> Option<String> {
        self.db
            .queue()
            .get_by_file_id(1)
            .await
            .unwrap()
            .map(|row| row.status)
    }

    async fn details(&mut self) {
        self.server
            .mock("GET", "/files/1")
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{"file":{{"id":1,"name":"movie.mkv","size":{SIZE},"file_type":"VIDEO","parent_id":0,"is_folder":false,"is_shared":false,"crc32":null}},"status":"OK"}}"#
            ))
            .create_async()
            .await;
    }
}

#[tokio::test]
async fn a_404_from_put_io_drops_the_row() {
    let mut s = setup().await;
    let _gone = s
        .server
        .mock("GET", "/files/1")
        .with_status(404)
        .with_body(r#"{"error_type":"NotFound","status":"ERROR"}"#)
        .create_async()
        .await;

    assert!(s.manager.process_queue().await.unwrap());
    assert_eq!(s.status().await, None);
}

#[tokio::test]
async fn another_error_from_put_io_keeps_the_row() {
    let mut s = setup().await;
    let _denied = s
        .server
        .mock("GET", "/files/1")
        .with_status(403)
        .with_body(r#"{"status":"ERROR"}"#)
        .create_async()
        .await;

    assert!(s.manager.process_queue().await.is_err());
    // Back to pending, not removed and not stuck on "downloading"
    assert_eq!(s.status().await.as_deref(), Some("pending"));
}

#[tokio::test]
async fn a_transient_failure_is_queued_again_by_the_next_sync() {
    let mut s = setup().await;
    s.details().await;
    let _down = s
        .server
        .mock("GET", "/files/1/download")
        .with_status(503)
        .expect_at_least(1)
        .create_async()
        .await;
    let _list = s
        .server
        .mock("GET", "/files/list?parent_id=0")
        .with_header("content-type", "application/json")
        .with_body(r#"{"files":[],"parent":{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false},"status":"OK"}"#)
        .create_async()
        .await;

    assert!(s.manager.process_queue().await.unwrap());
    assert_eq!(s.status().await.as_deref(), Some("failed"));

    s.manager.sync_downloads().await.unwrap();
    assert_eq!(s.status().await.as_deref(), Some("pending"));
}

#[tokio::test]
async fn a_permanent_failure_stays_failed_through_a_sync() {
    let mut s = setup().await;
    s.details().await;
    let _missing = s
        .server
        .mock("GET", "/files/1/download")
        .with_status(403)
        .expect_at_least(1)
        .create_async()
        .await;
    let _list = s
        .server
        .mock("GET", "/files/list?parent_id=0")
        .with_header("content-type", "application/json")
        .with_body(r#"{"files":[],"parent":{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false},"status":"OK"}"#)
        .create_async()
        .await;

    assert!(s.manager.process_queue().await.unwrap());
    assert_eq!(s.status().await.as_deref(), Some("failed"));

    s.manager.sync_downloads().await.unwrap();
    assert_eq!(s.status().await.as_deref(), Some("failed"));
}

#[tokio::test]
async fn paths_that_differ_only_in_case_are_one_claim() {
    let s = setup().await;
    let first = s
        .manager
        .claim_path(std::path::Path::new("/dl/Movie.mkv"))
        .await;
    let second = tokio::time::timeout(
        Duration::from_millis(500),
        s.manager.claim_path(std::path::Path::new("/dl/movie.MKV")),
    )
    .await;
    assert!(second.is_err(), "the second claim waits for the first");
    drop(first);
    let _ = s
        .manager
        .claim_path(std::path::Path::new("/dl/movie.MKV"))
        .await;
}

#[tokio::test]
async fn a_folder_path_that_leaves_the_download_directory_is_refused() {
    let mut s = setup().await;
    s.db.queue().remove(1).await.unwrap();
    s.db.queue()
        .add_with_path_kept(
            1,
            "movie.mkv",
            SIZE as i64,
            "",
            "../outside",
            0,
            QueueStatus::Pending,
            true,
        )
        .await
        .unwrap();
    s.details().await;
    let never = s
        .server
        .mock("GET", "/files/1/download")
        .expect(0)
        .create_async()
        .await;

    let _ = s.manager.process_queue().await;
    never.assert_async().await;
    assert!(!s._dir.path().parent().unwrap().join("outside").exists());
}
