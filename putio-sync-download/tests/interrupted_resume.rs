//! Rows left in `downloading` by a crash go back to `pending` at startup, keep their
//! priority and order, and the partial `.<name>.downloading` file resumes with a Range
//! request (post-review fix to feature 004).

use std::sync::Arc;

use putio_sync_client::PutioApiClient;
use putio_sync_db::models::QueueStatus;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager};

const ENTRY: &str = r#"{"id":1,"name":"movie.mkv","size":8192,"file_type":"VIDEO","parent_id":0,"is_folder":false,"is_shared":false}"#;

async fn status_and_priority(db: &Database, file_id: i64) -> (String, i64) {
    let item = db
        .queue()
        .get_all()
        .await
        .unwrap()
        .into_iter()
        .find(|item| item.file_id == file_id)
        .expect("queue row");
    (item.status, item.priority)
}

#[tokio::test]
async fn interrupted_download_is_requeued_and_resumes_from_partial_file() {
    let mut server = mockito::Server::new_async().await;
    let _file = server
        .mock("GET", "/files/1")
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"file":{ENTRY},"status":"OK"}}"#))
        .create_async()
        .await;
    // Only the missing half is served, and only for the Range the partial file implies
    let range = server
        .mock("GET", "/files/1/download")
        .match_header("range", "bytes=4096-8191")
        .with_status(206)
        .with_header("content-range", "bytes 4096-8191/8192")
        .with_body(vec![9u8; 4096])
        .expect(1)
        .create_async()
        .await;

    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let queue = db.queue();
    // The crashed download (highest priority), a pending row and a paused row
    queue.add(1, "movie.mkv", 8192, "", "", 5).await.unwrap();
    queue.add(2, "other.mkv", 8192, "", "", 1).await.unwrap();
    queue.add(3, "held.mkv", 8192, "", "", 9).await.unwrap();
    queue
        .update_status(1, QueueStatus::Downloading)
        .await
        .unwrap();
    queue.update_status(3, QueueStatus::Paused).await.unwrap();
    std::fs::write(dir.path().join(".movie.mkv.downloading"), [7u8; 4096]).unwrap();

    let client = Arc::new(PutioApiClient::with_base_url("tok".into(), server.url()));
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        max_retries: 0,
        retry_initial_delay_secs: 0,
        ..DownloadConfig::default()
    };
    let manager = DownloadManager::new(client, db.clone(), config);

    // Stranded: the claim only takes pending rows
    assert_eq!(status_and_priority(&db, 1).await, ("downloading".into(), 5));

    assert_eq!(manager.recover_interrupted().await.unwrap(), 1);
    assert_eq!(status_and_priority(&db, 1).await, ("pending".into(), 5));
    assert_eq!(status_and_priority(&db, 2).await, ("pending".into(), 1));
    assert_eq!(status_and_priority(&db, 3).await, ("paused".into(), 9));
    // Nothing left to recover the second time
    assert_eq!(manager.recover_interrupted().await.unwrap(), 0);

    // The requeued row is claimed first and resumes from its 4096 bytes
    assert!(manager.process_queue().await.unwrap());
    range.assert_async().await;
    let data = std::fs::read(dir.path().join("movie.mkv")).unwrap();
    assert_eq!(data.len(), 8192);
    assert!(data[..4096].iter().all(|b| *b == 7));
    assert!(data[4096..].iter().all(|b| *b == 9));
    assert!(!dir.path().join(".movie.mkv.downloading").exists());

    let left: Vec<i64> = db
        .queue()
        .get_all()
        .await
        .unwrap()
        .into_iter()
        .map(|item| item.file_id)
        .collect();
    assert_eq!(left, vec![3, 2]);
}
