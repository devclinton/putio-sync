//! Queueing a Put.io folder or file goes through the sync's filters and dedupe
//! (feature 004, R12).

use putio_sync_client::PutioApiClient;
use putio_sync_core::rules::RuleSet;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager, TreeDecision};
use std::sync::Arc;

const FOLDER_ID: i64 = 50;

fn entry(id: i64, name: &str, file_type: &str, parent_id: i64) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","size":100,"file_type":"{file_type}","parent_id":{parent_id},"is_folder":{},"is_shared":false}}"#,
        file_type == "FOLDER"
    )
}

async fn setup() -> (
    mockito::ServerGuard,
    tempfile::TempDir,
    Arc<Database>,
    DownloadManager,
) {
    let mut server = mockito::Server::new_async().await;
    let folder = entry(FOLDER_ID, "Show", "FOLDER", 0);
    server
        .mock("GET", format!("/files/{FOLDER_ID}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"file":{folder},"status":"OK"}}"#))
        .create_async()
        .await;
    server
        .mock("GET", "/files/1")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"file":{},"status":"OK"}}"#,
            entry(1, "a.mkv", "VIDEO", FOLDER_ID)
        ))
        .create_async()
        .await;
    server
        .mock("GET", format!("/files/list?parent_id={FOLDER_ID}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"files":[{},{},{}],"parent":{folder},"status":"OK"}}"#,
            entry(1, "a.mkv", "VIDEO", FOLDER_ID),
            entry(2, "b.exe", "FILE", FOLDER_ID),
            entry(3, "c.mkv", "VIDEO", FOLDER_ID),
        ))
        .create_async()
        .await;
    server
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"transfers":[],"status":"OK"}"#)
        .create_async()
        .await;

    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    db.queue()
        .add(3, "c.mkv", 100, "http://unused", "Show", 0)
        .await
        .unwrap();
    let client = Arc::new(PutioApiClient::with_base_url(
        "test-putio-token".to_string(),
        server.url(),
    ));
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        rules: RuleSet::from_config(&putio_sync_core::config::DownloadConfig {
            blocked_extensions: vec!["exe".to_string()],
            ..Default::default()
        }),
        ..DownloadConfig::default()
    };
    let manager = DownloadManager::new(client, db.clone(), config);
    (server, dir, db, manager)
}

#[tokio::test]
async fn folder_is_queued_through_the_sync_filters() {
    let (_server, _dir, db, manager) = setup().await;

    let stats = manager.enqueue_tree(FOLDER_ID).await.unwrap();
    assert_eq!(stats.total_files, 3);
    assert_eq!(stats.files_downloaded, 1, "queued");
    assert_eq!(stats.files_blocked, 1);
    assert_eq!(stats.files_skipped, 1);
    assert_eq!(stats.files_failed, 0);

    let queued: Vec<_> = db
        .queue()
        .get_all()
        .await
        .unwrap()
        .into_iter()
        .map(|item| (item.filename, item.relative_path))
        .collect();
    assert_eq!(queued.len(), 2);
    assert!(queued.contains(&("a.mkv".to_string(), "Show".to_string())));
    assert!(queued.iter().any(|(name, _)| name == "c.mkv"));
}

#[tokio::test]
async fn single_file_is_queued_under_its_folder() {
    let (_server, _dir, db, manager) = setup().await;

    let stats = manager.enqueue_tree(1).await.unwrap();
    assert_eq!(stats.total_files, 1);
    assert_eq!(stats.files_downloaded, 1);

    let item = db.queue().get_by_file_id(1).await.unwrap().unwrap();
    assert_eq!(item.relative_path, "Show");
}

/// Feature 011: the report says what happened to each file and names the root
#[tokio::test]
async fn report_lists_each_decision() {
    let (_server, _dir, db, manager) = setup().await;
    db.downloads()
        .insert(
            1,
            "a.mkv",
            100,
            "/x/a.mkv",
            putio_sync_db::models::DownloadStatus::Completed,
        )
        .await
        .unwrap();

    let report = manager.enqueue_tree_report(FOLDER_ID).await.unwrap();
    assert_eq!(report.root_name.as_deref(), Some("Show"));
    let mut decisions: Vec<_> = report
        .files
        .iter()
        .map(|f| (f.file_id, f.relative_path.as_str(), f.decision.clone()))
        .collect();
    decisions.sort_by_key(|d| d.0);
    assert_eq!(
        decisions,
        vec![
            (1, "Show", TreeDecision::Known),
            (2, "Show", TreeDecision::Blocked("exe".to_string())),
            (3, "Show", TreeDecision::Known),
        ]
    );
    assert!(report.files[0].decision.accepted());
    assert_eq!(report.stats.files_skipped, 2);

    let single = manager.enqueue_tree_report(1).await.unwrap();
    assert_eq!(single.root_name.as_deref(), Some("a.mkv"));
    assert_eq!(single.files.len(), 1);
}
