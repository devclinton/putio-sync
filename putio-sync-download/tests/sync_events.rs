//! `sync.completed` (feature 027): the trigger, result and counts of each sync run

use putio_sync_client::{PutioApiClient, RetryPolicy};
use putio_sync_core::events::{self, Event, Payload, SyncCompleted};
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

const ROOT: &str = r#"{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false}"#;

async fn next_sync(rx: &mut broadcast::Receiver<Arc<Event>>) -> SyncCompleted {
    let wait = async {
        loop {
            let event = rx.recv().await.unwrap();
            if let Payload::SyncCompleted(s) = &event.payload {
                events::validate_against_schema(&serde_json::to_value(&*event).unwrap()).unwrap();
                return s.clone();
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .expect("sync.completed")
}

#[tokio::test]
async fn each_run_sends_sync_completed() {
    let mut server = mockito::Server::new_async().await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(
        PutioApiClient::with_base_url("tok_SECRET".into(), server.url()).with_retry_policy(
            RetryPolicy {
                max_attempts: 1,
                ..RetryPolicy::default()
            },
        ),
    );
    let manager = DownloadManager::new(
        client,
        db,
        DownloadConfig {
            target_directory: dir.path().to_path_buf(),
            ..DownloadConfig::default()
        },
    );
    let mut rx = events::subscribe();

    let list = server
        .mock("GET", "/files/list?parent_id=0")
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"files":[{{"id":9,"name":"setup.exe","size":100,"file_type":"FILE","parent_id":0,"is_folder":false,"is_shared":false}}],"parent":{ROOT},"status":"OK"}}"#
        ))
        .create_async()
        .await;
    manager.sync_downloads().await.unwrap();
    let run = next_sync(&mut rx).await;
    assert_eq!(run.trigger, "schedule");
    assert_eq!(run.result, "success");
    assert_eq!(run.error, None);
    assert_eq!(run.files_found, 1);
    assert_eq!(run.files_blocked, 1);
    assert!(run.duration_seconds >= 0.0);

    // "Sync now" without a loop runs on a spawned task
    manager.request_sync().await.unwrap();
    let run = next_sync(&mut rx).await;
    assert_eq!(run.trigger, "manual");
    assert_eq!(run.result, "success");
    list.remove_async().await;

    server
        .mock("GET", "/files/list?parent_id=0")
        .with_status(500)
        .create_async()
        .await;
    assert!(manager.sync_downloads().await.is_err());
    let run = next_sync(&mut rx).await;
    assert_eq!(run.trigger, "schedule", "the manual flag is used once");
    assert_eq!(run.result, "failure");
    let error = run.error.unwrap();
    assert!(!error.is_empty());
    assert!(
        !error.contains("tok_SECRET") && !error.contains('?'),
        "{error}"
    );
    assert_eq!(run.files_found, 0);
}
