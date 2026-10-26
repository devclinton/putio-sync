//! A watcher upload records that its transfer came from a watch folder, so the Stats
//! page can show downloads by origin (feature 006, R11).

use putio_sync_client::PutioApiClient;
use putio_sync_db::models::Origin;
use putio_sync_db::Database;
use putio_sync_file_manager::TorrentWatchCore;

#[tokio::test]
async fn upload_records_watch_origin() {
    let dir = tempfile::tempdir().unwrap();
    let watch = dir.path().join("watch");
    std::fs::create_dir(&watch).unwrap();
    std::fs::write(watch.join("x.magnet"), "magnet:?xt=urn:btih:abc").unwrap();
    let db = Database::new(dir.path().join(".putiosync.db"))
        .await
        .unwrap();

    let mut server = mockito::Server::new_async().await;
    let upload = server
        .mock("POST", "/transfers/add")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"transfer":{"id":77,"name":"x","status":"IN_QUEUE","created_at":"2026-01-01T00:00:00","save_parent_id":0},"status":"OK"}"#,
        )
        .create_async()
        .await;
    let client = PutioApiClient::with_base_url("test_token".to_string(), server.url());

    let stats = TorrentWatchCore::new()
        .scan_directories(
            std::slice::from_ref(&watch),
            &client,
            None,
            None,
            false,
            Some(&db),
        )
        .await
        .unwrap();
    assert_eq!(stats.files_processed, 1);
    upload.assert_async().await;

    let origin = db.sources().transfer_origin(77).await.unwrap().unwrap();
    assert_eq!(origin.origin, Origin::Watch);
    assert_eq!(origin.watch_folder, Some(watch.display().to_string()));
}
