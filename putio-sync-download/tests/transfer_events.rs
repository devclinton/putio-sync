//! `transfer.*` events from the shared transfer list (feature 027, R6)

use putio_sync_client::{PutioApiClient, RetryPolicy};
use putio_sync_core::events::{self, Event, EventType};
use putio_sync_db::pipeline::TRANSFER_EVENTS_SEEN;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

fn list(transfers: &[(i64, &str)]) -> String {
    let items: Vec<String> = transfers
        .iter()
        .map(|(id, status)| {
            format!(
                r#"{{"id":{id},"name":"t{id}","status":"{status}","size":100,"error_message":{err},"created_at":"2026-01-01T00:00:00","save_parent_id":0,"file_id":{file}}}"#,
                err = if *status == "ERROR" { r#""No seeders""# } else { "null" },
                file = if *status == "COMPLETED" { "55" } else { "null" },
            )
        })
        .collect();
    format!(r#"{{"transfers":[{}],"status":"OK"}}"#, items.join(","))
}

/// The `(type, transfer id, origin)` of every transfer event for ids of 880 000 and up
/// sent within a short wait (the bus is shared with other tests)
async fn drain(rx: &mut broadcast::Receiver<Arc<Event>>) -> Vec<(EventType, i64, String)> {
    let mut out = Vec::new();
    while let Ok(Ok(event)) = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
        let json = serde_json::to_value(&*event).unwrap();
        let id = json["data"]["transfer_id"].as_i64().unwrap_or(0);
        if id >= 880_000 {
            events::validate_against_schema(&json).unwrap();
            out.push((
                event.event_type(),
                id,
                json["data"]["origin"].as_str().unwrap().to_string(),
            ));
        }
    }
    out
}

#[tokio::test]
async fn transitions_emit_events_and_the_map_is_stored() {
    let mut server = mockito::Server::new_async().await;
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
    let manager = DownloadManager::new(
        client,
        db.clone(),
        DownloadConfig {
            target_directory: dir.path().to_path_buf(),
            ..DownloadConfig::default()
        },
    );
    let mut rx = events::subscribe();
    let fresh = Duration::ZERO;

    // No stored map: the first list is the baseline
    let mock = server
        .mock("GET", "/transfers/list")
        .with_body(list(&[(880_001, "DOWNLOADING")]))
        .create_async()
        .await;
    manager.transfers_snapshot(fresh).await.unwrap();
    assert!(drain(&mut rx).await.is_empty());
    assert_eq!(
        db.flags().get_text(TRANSFER_EVENTS_SEEN).await.unwrap().as_deref(),
        Some(r#"{"880001":"a"}"#)
    );
    mock.remove_async().await;

    let mock = server
        .mock("GET", "/transfers/list")
        .with_body(list(&[(880_001, "COMPLETED"), (880_002, "ERROR")]))
        .create_async()
        .await;
    manager.transfers_snapshot(fresh).await.unwrap();
    assert_eq!(
        drain(&mut rx).await,
        vec![
            (EventType::TransferFinished, 880_001, "putio".to_string()),
            (EventType::TransferAdded, 880_002, "putio".to_string()),
            (EventType::TransferFailed, 880_002, "putio".to_string()),
        ]
    );
    // The same list again sends nothing
    manager.transfers_snapshot(fresh).await.unwrap();
    assert!(drain(&mut rx).await.is_empty());
    mock.remove_async().await;

    // A restart reads the stored map, so nothing is sent twice
    let restarted = DownloadManager::new(
        Arc::new(PutioApiClient::with_base_url("tok".into(), server.url())),
        db.clone(),
        DownloadConfig {
            target_directory: dir.path().to_path_buf(),
            ..DownloadConfig::default()
        },
    );
    server
        .mock("GET", "/transfers/list")
        .with_body(list(&[(880_001, "COMPLETED"), (880_002, "COMPLETED")]))
        .create_async()
        .await;
    restarted.transfers_snapshot(fresh).await.unwrap();
    assert_eq!(
        drain(&mut rx).await,
        vec![(EventType::TransferFinished, 880_002, "putio".to_string())]
    );
}
