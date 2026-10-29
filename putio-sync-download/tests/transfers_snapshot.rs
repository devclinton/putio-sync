//! The shared Put.io transfers snapshot (feature 039, R3): one Put.io call per `max_age`,
//! shared by concurrent callers, and the last list when Put.io fails

use putio_sync_client::{PutioApiClient, RetryPolicy};
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager};
use std::sync::Arc;
use std::time::Duration;

const LIST: &str = r#"{"transfers":[{"id":1,"name":"t1","status":"DOWNLOADING","created_at":"2026-01-01T00:00:00","save_parent_id":0}],"status":"OK"}"#;

async fn setup() -> (mockito::ServerGuard, tempfile::TempDir, DownloadManager) {
    let server = mockito::Server::new_async().await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(
        PutioApiClient::with_base_url("tok".into(), server.url()).with_retry_policy(RetryPolicy {
            max_attempts: 1,
            ..RetryPolicy::default()
        }),
    );
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        ..DownloadConfig::default()
    };
    (server, dir, DownloadManager::new(client, db, config))
}

#[tokio::test]
async fn calls_within_max_age_share_one_put_io_request() {
    let (mut server, _dir, manager) = setup().await;
    let list = server
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(LIST)
        .expect(1)
        .create_async()
        .await;

    let first = manager
        .transfers_snapshot(Duration::from_secs(10))
        .await
        .unwrap();
    let second = manager
        .transfers_snapshot(Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(first.transfers.len(), 1);
    assert!(!first.stale && !second.stale);
    assert!(Arc::ptr_eq(&first.transfers, &second.transfers));
    list.assert_async().await;
}

#[tokio::test]
async fn concurrent_callers_share_one_fetch() {
    let (mut server, _dir, manager) = setup().await;
    let list = server
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(LIST)
        .expect(1)
        .create_async()
        .await;

    let calls = (0..5).map(|_| {
        let manager = manager.clone();
        tokio::spawn(async move { manager.transfers_snapshot(Duration::from_secs(10)).await })
    });
    for call in futures_util::future::join_all(calls).await {
        assert_eq!(call.unwrap().unwrap().transfers.len(), 1);
    }
    list.assert_async().await;
}

#[tokio::test]
async fn an_old_snapshot_is_fetched_again() {
    let (mut server, _dir, manager) = setup().await;
    let list = server
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(LIST)
        .expect(2)
        .create_async()
        .await;
    manager.transfers_snapshot(Duration::ZERO).await.unwrap();
    manager.transfers_snapshot(Duration::ZERO).await.unwrap();
    list.assert_async().await;
}

#[tokio::test]
async fn a_put_io_error_returns_the_last_list_flagged_stale() {
    let (mut server, _dir, manager) = setup().await;
    let ok = server
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(LIST)
        .create_async()
        .await;
    let fresh = manager.transfers_snapshot(Duration::ZERO).await.unwrap();
    ok.remove_async().await;
    server
        .mock("GET", "/transfers/list")
        .with_status(503)
        .create_async()
        .await;

    let stale = manager.transfers_snapshot(Duration::ZERO).await.unwrap();
    assert!(stale.stale);
    assert_eq!(stale.transfers.len(), 1);
    assert_eq!(stale.fetched_at, fresh.fetched_at);
}

#[tokio::test]
async fn a_put_io_error_with_no_list_yet_is_an_error() {
    let (mut server, _dir, manager) = setup().await;
    server
        .mock("GET", "/transfers/list")
        .with_status(503)
        .create_async()
        .await;
    assert!(manager
        .transfers_snapshot(Duration::from_secs(10))
        .await
        .is_err());
}
