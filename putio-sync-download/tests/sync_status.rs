//! Sync status tracking and "sync now" (feature 004, R5).

use putio_sync_client::PutioApiClient;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager, SyncOutcome, SyncRequestError};
use std::sync::Arc;
use std::time::Duration;

const EMPTY_ROOT: &str = r#"{"files":[],"parent":{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false},"status":"OK"}"#;

async fn manager(server: &mockito::ServerGuard, dir: &tempfile::TempDir) -> DownloadManager {
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(PutioApiClient::with_base_url(
        "test-putio-token".to_string(),
        server.url(),
    ));
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        ..DownloadConfig::default()
    };
    DownloadManager::new(client, db, config)
}

async fn wait_for_outcome(manager: &DownloadManager) -> SyncOutcome {
    for _ in 0..100 {
        let status = manager.sync_status().await;
        if !status.in_progress {
            if let Some(outcome) = status.last_outcome {
                return outcome;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("sync did not finish");
}

#[tokio::test(flavor = "multi_thread")]
async fn request_sync_without_loop_runs_a_sync() {
    let mut server = mockito::Server::new_async().await;
    let _list = server
        .mock("GET", "/files/list?parent_id=0")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(EMPTY_ROOT)
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let manager = manager(&server, &dir).await;

    assert!(manager.sync_status().await.last_outcome.is_none());
    manager.request_sync().await.unwrap();

    match wait_for_outcome(&manager).await {
        SyncOutcome::Success { found, .. } => assert_eq!(found, 0),
        other => panic!("expected success, got {:?}", other),
    }
    let status = manager.sync_status().await;
    assert!(status.last_finished_at.is_some());
    assert!(status.next_due_at.is_none(), "no loop, so nothing is due");
}

#[tokio::test(flavor = "multi_thread")]
async fn second_request_while_syncing_is_refused() {
    let mut server = mockito::Server::new_async().await;
    let _list = server
        .mock("GET", "/files/list?parent_id=0")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_chunked_body(|w| {
            std::thread::sleep(Duration::from_millis(500));
            w.write_all(EMPTY_ROOT.as_bytes())
        })
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let manager = manager(&server, &dir).await;

    manager.request_sync().await.unwrap();
    assert!(manager.sync_status().await.in_progress);
    assert!(matches!(
        manager.request_sync().await,
        Err(SyncRequestError::InProgress)
    ));

    assert!(matches!(
        wait_for_outcome(&manager).await,
        SyncOutcome::Success { .. }
    ));
    manager.request_sync().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn request_sync_wakes_the_loop() {
    let mut server = mockito::Server::new_async().await;
    let list = server
        .mock("GET", "/files/list?parent_id=0")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(EMPTY_ROOT)
        .expect(2)
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let manager = manager(&server, &dir).await;

    let looping = manager.clone();
    let task = tokio::spawn(async move { looping.run_sync_loop(3600).await });

    // The loop's first cycle runs at once
    wait_for_outcome(&manager).await;
    let first = manager.sync_status().await;
    assert!(first.next_due_at.is_some());
    assert_eq!(first.interval, Some(Duration::from_secs(3600)));

    manager.request_sync().await.unwrap();
    for _ in 0..100 {
        let status = manager.sync_status().await;
        if !status.in_progress && status.last_finished_at > first.last_finished_at {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    task.abort();
    list.assert_async().await;
}
