//! The pipeline can hold back the Put.io delete of a downloaded file (feature 025). Small
//! payloads against mockito; nothing reaches Put.io.

use std::sync::Arc;

use mockito::ServerGuard;
use putio_sync_client::PutioApiClient;
use putio_sync_db::models::{DownloadStatus, QueueStatus};
use putio_sync_db::Database;
use putio_sync_download::{DeferFuture, DownloadConfig, DownloadManager, RemoteDeleteGate};

const SIZE: usize = 2048;

struct Defer(Option<&'static str>);

impl RemoteDeleteGate for Defer {
    fn defer<'a>(&'a self, _: i64, _: &'a str, _: &'a str) -> DeferFuture<'a> {
        let key = self.0.map(str::to_string);
        Box::pin(async move { key })
    }
}

struct Setup {
    server: ServerGuard,
    _dir: tempfile::TempDir,
    db: Arc<Database>,
    manager: DownloadManager,
}

async fn setup() -> Setup {
    let mut server = mockito::Server::new_async().await;
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
        delete_after_download: true,
        ..DownloadConfig::default()
    };
    let manager = DownloadManager::new(client, db.clone(), config);
    server
        .mock("GET", "/files/7")
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"file":{{"id":7,"name":"movie.mkv","size":{SIZE},"file_type":"VIDEO","parent_id":0,"is_folder":false,"is_shared":false}},"status":"OK"}}"#
        ))
        .create_async()
        .await;
    server
        .mock("GET", "/files/7/download")
        .with_body(vec![1u8; SIZE])
        .create_async()
        .await;
    db.queue()
        .add_with_path_kept(
            7,
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

#[tokio::test]
async fn a_gate_answer_holds_the_delete_and_records_it() {
    let mut s = setup().await;
    let delete = s
        .server
        .mock("POST", "/files/delete")
        .expect(0)
        .create_async()
        .await;
    s.manager.set_delete_gate(Arc::new(Defer(Some("transfer:3"))));
    assert!(s.manager.process_queue().await.unwrap());
    delete.assert_async().await;
    let rows = s.db.deferred_deletes().list().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].file_id, rows[0].item_key.as_str()), (7, "transfer:3"));
}

#[tokio::test]
async fn no_answer_deletes_as_before() {
    let mut s = setup().await;
    let delete = s
        .server
        .mock("POST", "/files/delete")
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(1)
        .create_async()
        .await;
    s.manager.set_delete_gate(Arc::new(Defer(None)));
    assert!(s.manager.process_queue().await.unwrap());
    delete.assert_async().await;
    assert!(s.db.deferred_deletes().list().await.unwrap().is_empty());
}

#[tokio::test]
async fn delete_remote_deletes_completed_downloads_and_refuses_the_rest() {
    let mut s = setup().await;
    s.db.downloads()
        .insert(1, "a.mkv", 10, "/d/a.mkv", DownloadStatus::Completed)
        .await
        .unwrap();
    s.db.downloads()
        .insert(2, "b.mkv", 10, "/d/b.mkv", DownloadStatus::Failed)
        .await
        .unwrap();
    let delete = s
        .server
        .mock("POST", "/files/delete")
        .match_body(mockito::Matcher::Regex("1".into()))
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(1)
        .create_async()
        .await;
    // 2 is not completed and 3 has no row: only 1 goes
    let done = s.manager.delete_remote(&[1, 2, 3]).await;
    assert_eq!(done, [1]);
    delete.assert_async().await;
    assert!(s.manager.delete_remote(&[2, 3]).await.is_empty());
}
