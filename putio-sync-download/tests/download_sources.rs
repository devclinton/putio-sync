//! Queueing records where each file came from (feature 006, R4, R11): the transfer's
//! type, tracker host and privacy, how the transfer was created, and the Put.io folder.

use putio_sync_client::{PutioApiClient, RetryPolicy};
use putio_sync_db::models::{Origin, SourceType, TransferOrigin};
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager};
use std::sync::Arc;

fn entry(id: i64, name: &str, file_type: &str, parent_id: i64) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","size":100,"file_type":"{file_type}","parent_id":{parent_id},"is_folder":{},"is_shared":false}}"#,
        file_type == "FOLDER"
    )
}

fn transfer(id: i64, file_id: i64, kind: &str, tracker: &str, private: bool) -> String {
    format!(
        r#"{{"id":{id},"name":"t{id}","status":"COMPLETED","created_at":"2026-01-01T00:00:00","save_parent_id":0,"file_id":{file_id},"type":"{kind}","tracker":"{tracker}","is_private":{private},"source":"magnet:?xt=urn:btih:abc&tr=PASSKEY"}}"#
    )
}

/// Root holds folder "Show" (a private torrent from a watch folder), "single.iso" (a
/// URL transfer) and "upload.txt" (no transfer). "Show" holds "Season 1/e01.mkv".
async fn setup(
    transfers_status: usize,
) -> (
    mockito::ServerGuard,
    tempfile::TempDir,
    Arc<Database>,
    DownloadManager,
    mockito::Mock,
) {
    let mut server = mockito::Server::new_async().await;
    let root = entry(0, "Root", "FOLDER", 0);
    for (parent, files) in [
        (
            0,
            vec![
                entry(50, "Show", "FOLDER", 0),
                entry(60, "single.iso", "FILE", 0),
                entry(70, "upload.txt", "FILE", 0),
            ],
        ),
        (50, vec![entry(51, "Season 1", "FOLDER", 50)]),
        (51, vec![entry(52, "e01.mkv", "VIDEO", 51)]),
    ] {
        server
            .mock("GET", format!("/files/list?parent_id={parent}").as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{"files":[{}],"parent":{root},"status":"OK"}}"#,
                files.join(",")
            ))
            .create_async()
            .await;
    }
    let transfers = server
        .mock("GET", "/transfers/list")
        .with_status(transfers_status)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"transfers":[{},{}],"status":"OK"}}"#,
            transfer(
                900,
                50,
                "TORRENT",
                "https://tracker.example.org/PASSKEY/announce",
                true
            ),
            transfer(901, 60, "URL", "", false),
        ))
        .create_async()
        .await;

    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    db.sources()
        .record_transfer_origin(
            900,
            &TransferOrigin {
                origin: Origin::Watch,
                watch_folder: Some("/watch/tv".into()),
            },
        )
        .await
        .unwrap();
    // One attempt, so the failed listing doesn't wait out the retries
    let client = Arc::new(
        PutioApiClient::with_base_url("test-putio-token".to_string(), server.url())
            .with_retry_policy(RetryPolicy {
                max_attempts: 1,
                ..RetryPolicy::default()
            }),
    );
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        ..DownloadConfig::default()
    };
    let manager = DownloadManager::new(client, db.clone(), config);
    (server, dir, db, manager, transfers)
}

#[tokio::test]
async fn queued_files_record_their_source() {
    let (_server, _dir, db, manager, transfers) = setup(200).await;

    let stats = manager.enqueue_tree(0).await.unwrap();
    assert_eq!(stats.files_downloaded, 3);
    transfers.expect(1).assert_async().await;

    let sources = db.sources();
    let episode = sources.get(52).await.unwrap().unwrap();
    assert_eq!(episode.transfer_id, Some(900));
    assert_eq!(episode.source_type, Some(SourceType::Torrent));
    assert_eq!(episode.tracker_host.as_deref(), Some("tracker.example.org"));
    assert!(episode.is_private);
    assert_eq!(episode.origin, Origin::Watch);
    assert_eq!(episode.watch_folder.as_deref(), Some("/watch/tv"));
    assert_eq!(episode.putio_folder.as_deref(), Some("Show"));

    let single = sources.get(60).await.unwrap().unwrap();
    assert_eq!(single.source_type, Some(SourceType::Url));
    assert_eq!(single.tracker_host, None);
    assert_eq!(single.origin, Origin::Putio);
    assert_eq!(single.putio_folder.as_deref(), Some("(root)"));

    let upload = sources.get(70).await.unwrap().unwrap();
    assert_eq!(upload.transfer_id, None);
    assert_eq!(upload.source_type, None);
    assert_eq!(upload.origin, Origin::Unknown);
}

#[tokio::test]
async fn nothing_queued_lists_no_transfers() {
    let (_server, _dir, db, manager, transfers) = setup(200).await;
    for (id, name) in [(52, "e01.mkv"), (60, "single.iso"), (70, "upload.txt")] {
        db.queue()
            .add(
                id,
                name,
                100,
                "http://unused",
                "",
                0,
                putio_sync_db::models::QueueStatus::Pending,
            )
            .await
            .unwrap();
    }

    let stats = manager.enqueue_tree(0).await.unwrap();
    assert_eq!(stats.files_downloaded, 0);
    transfers.expect(0).assert_async().await;
}

#[tokio::test]
async fn failed_transfer_listing_still_queues() {
    let (_server, _dir, db, manager, _transfers) = setup(500).await;

    let stats = manager.enqueue_tree(0).await.unwrap();
    assert_eq!(stats.files_downloaded, 3);
    let episode = db.sources().get(52).await.unwrap().unwrap();
    assert_eq!(episode.origin, Origin::Unknown);
    assert_eq!(episode.source_type, None);
    assert_eq!(episode.putio_folder.as_deref(), Some("Show"));
}

/// A transfer whose release check waits for Put.io is not queued (feature 042, R8)
#[tokio::test]
async fn pending_release_check_holds_its_transfer() {
    let (_server, _dir, db, manager, transfers) = setup(200).await;
    db.releases()
        .insert_check(&putio_sync_db::NewCheck {
            hash: Some("c12fe1c06bba254a9dc9f519b335aa7c1367a88a"),
            name: "Show",
            source: "magnet",
            stage: "before",
            state: None,
            lookup: Some("not_found"),
            transfer_id: Some(900),
            ..putio_sync_db::NewCheck::default()
        })
        .await
        .unwrap();

    let stats = manager.enqueue_tree(0).await.unwrap();
    assert_eq!(stats.files_downloaded, 2);
    assert_eq!(stats.files_skipped, 1);
    assert!(db.queue().get_by_file_id(52).await.unwrap().is_none());
    assert!(db.queue().get_by_file_id(60).await.unwrap().is_some());
    transfers.expect(1).assert_async().await;
}
