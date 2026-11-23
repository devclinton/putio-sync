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

/// A server that answers the first request at once and holds every later one for `delay`
async fn slow_after_first(delay: Duration) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let mut served = 0;
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let slow = served > 0;
            served += 1;
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                if slow {
                    tokio::time::sleep(delay).await;
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{LIST}",
                    LIST.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    url
}

#[tokio::test]
async fn readers_do_not_wait_for_a_slow_refresh() {
    let url = slow_after_first(Duration::from_secs(3)).await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(
        PutioApiClient::with_base_url("tok".into(), url).with_retry_policy(RetryPolicy {
            max_attempts: 1,
            ..RetryPolicy::default()
        }),
    );
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        ..DownloadConfig::default()
    };
    let manager = DownloadManager::new(client, db, config);
    manager.transfers_snapshot(Duration::ZERO).await.unwrap();

    // The next refresh hangs for 3 s
    let refresh = {
        let manager = manager.clone();
        tokio::spawn(async move { manager.transfers_snapshot(Duration::ZERO).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;

    let stalls = putio_sync_core::stall::StallTracker::default();
    let started = std::time::Instant::now();
    let pipeline = tokio::time::timeout(
        Duration::from_millis(1000),
        manager.pipeline_cached(&stalls, 24),
    )
    .await
    .expect("the cached pipeline waited for the Put.io refresh");
    assert!(pipeline.is_ok());
    // Another caller with an older list gets it too, flagged as it was
    let other = tokio::time::timeout(
        Duration::from_millis(1000),
        manager.transfers_snapshot(Duration::ZERO),
    )
    .await
    .expect("a second refresh caller waited for the first")
    .unwrap();
    assert_eq!(other.transfers.len(), 1);
    assert!(started.elapsed() < Duration::from_millis(1000));

    assert!(!refresh.await.unwrap().unwrap().stale);
}
