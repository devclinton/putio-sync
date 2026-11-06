//! Pausing a transfer still on Put.io holds its NAS download: nothing is sent to Put.io,
//! and its files are queued `paused` when they arrive (feature 039, R9, FR-016-FR-018,
//! SC-008).

use putio_sync_client::PutioApiClient;
use putio_sync_core::stall::StallTracker;
use putio_sync_db::models::QueueStatus;
use putio_sync_db::Database;
use putio_sync_download::pipeline::{ActionResult, ItemResult};
use putio_sync_download::{DownloadConfig, DownloadManager};
use std::sync::Arc;
use std::time::Duration;

fn entry(id: i64, name: &str, file_type: &str, parent_id: i64) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","size":100,"file_type":"{file_type}","parent_id":{parent_id},"is_folder":{},"is_shared":false}}"#,
        file_type == "FOLDER"
    )
}

fn transfers_body(status: &str) -> String {
    format!(
        r#"{{"transfers":[{{"id":900,"name":"Show","size":200,"status":"{status}","created_at":"2026-01-01T00:00:00","finished_at":"2026-01-01T01:00:00","save_parent_id":0,"file_id":50,"percent_done":40,"downloaded":80}}],"status":"OK"}}"#
    )
}

/// Put.io with transfer 900 (folder "Show" holding two files), and mocks that fail the
/// test if anything cancels, cleans, retries or deletes
struct Putio {
    server: mockito::ServerGuard,
    forbidden: Vec<mockito::Mock>,
    list: Option<mockito::Mock>,
}

impl Putio {
    async fn new(status: &str) -> Self {
        let mut server = mockito::Server::new_async().await;
        let root = entry(0, "Root", "FOLDER", 0);
        for (parent, files) in [
            (0, vec![entry(50, "Show", "FOLDER", 0)]),
            (
                50,
                vec![
                    entry(52, "e01.mkv", "VIDEO", 50),
                    entry(53, "e02.mkv", "VIDEO", 50),
                ],
            ),
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
        let mut forbidden = Vec::new();
        for path in [
            "/transfers/cancel",
            "/transfers/clean",
            "/transfers/retry",
            "/files/delete",
        ] {
            forbidden.push(
                server
                    .mock("POST", path)
                    .with_status(200)
                    .with_body(r#"{"status":"OK"}"#)
                    .expect(0)
                    .create_async()
                    .await,
            );
        }
        let mut putio = Self {
            server,
            forbidden,
            list: None,
        };
        putio.set_status(status).await;
        putio
    }

    async fn set_status(&mut self, status: &str) {
        if let Some(old) = self.list.take() {
            old.remove_async().await;
        }
        self.list = Some(
            self.server
                .mock("GET", "/transfers/list")
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(transfers_body(status))
                .create_async()
                .await,
        );
    }

    async fn assert_nothing_sent(&self) {
        for mock in &self.forbidden {
            mock.assert_async().await;
        }
    }
}

async fn open(dir: &std::path::Path, url: &str) -> (Arc<Database>, DownloadManager) {
    let db = Arc::new(Database::new(dir.join(".putiosync.db")).await.unwrap());
    let client = Arc::new(PutioApiClient::with_base_url("tok".into(), url.to_string()));
    let config = DownloadConfig {
        target_directory: dir.to_path_buf(),
        ..DownloadConfig::default()
    };
    (db.clone(), DownloadManager::new(client, db, config))
}

fn keys(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

fn results(results: Vec<ItemResult>) -> Vec<(String, ActionResult)> {
    results.into_iter().map(|r| (r.key, r.result)).collect()
}

#[tokio::test]
async fn holding_a_putio_stage_transfer_sends_nothing_to_putio() {
    let putio = Putio::new("DOWNLOADING").await;
    let dir = tempfile::tempdir().unwrap();
    let (db, manager) = open(dir.path(), &putio.server.url()).await;
    let stalls = StallTracker::default();

    let paused = manager
        .pause_items(&keys(&["transfer:900", "transfer:999"]), &stalls, 24)
        .await
        .unwrap();
    assert_eq!(
        results(paused),
        vec![
            ("transfer:900".to_string(), ActionResult::Held),
            ("transfer:999".to_string(), ActionResult::NotFound),
        ]
    );
    assert!(db.holds().is_held(900).await.unwrap());

    // Holding again changes nothing
    let again = manager
        .pause_items(&keys(&["transfer:900"]), &stalls, 24)
        .await
        .unwrap();
    assert_eq!(results(again)[0].1, ActionResult::Already);

    // The item shows on hold, still in the Put.io stage
    let pipeline = manager
        .pipeline(Duration::from_secs(10), &stalls, 24)
        .await
        .unwrap();
    let item = &pipeline.items[0];
    assert!(item.held && item.paused);
    assert_eq!(pipeline.counts.putio.on_hold, 1);
    putio.assert_nothing_sent().await;
}

#[tokio::test]
async fn files_of_a_held_transfer_arrive_paused_and_resume_releases_them() {
    let mut putio = Putio::new("DOWNLOADING").await;
    let dir = tempfile::tempdir().unwrap();
    let (db, manager) = open(dir.path(), &putio.server.url()).await;
    let stalls = StallTracker::default();
    manager
        .pause_items(&keys(&["transfer:900"]), &stalls, 24)
        .await
        .unwrap();

    // Put.io finishes it and the sync queues its files
    putio.set_status("COMPLETED").await;
    let stats = manager.enqueue_tree(0).await.unwrap();
    assert_eq!(stats.files_downloaded, 2);
    for id in [52, 53] {
        let row = db.queue().get_by_file_id(id).await.unwrap().unwrap();
        assert_eq!(row.status, "paused", "file {id} arrives paused");
    }
    assert!(
        db.queue().claim_next_pending().await.unwrap().is_none(),
        "no worker can claim a held file"
    );

    // Resume deletes the hold and resumes its queued rows
    let resumed = manager
        .resume_items(&keys(&["transfer:900", "transfer:999"]), &stalls, 24)
        .await
        .unwrap();
    assert_eq!(
        results(resumed),
        vec![
            ("transfer:900".to_string(), ActionResult::Resumed),
            ("transfer:999".to_string(), ActionResult::NotFound),
        ]
    );
    assert!(!db.holds().is_held(900).await.unwrap());
    for id in [52, 53] {
        let row = db.queue().get_by_file_id(id).await.unwrap().unwrap();
        assert_eq!(row.status, "pending", "file {id} is back in the queue");
    }
    putio.assert_nothing_sent().await;
}

#[tokio::test]
async fn a_hold_survives_reopening_the_database() {
    let mut putio = Putio::new("IN_QUEUE").await;
    let dir = tempfile::tempdir().unwrap();
    {
        let (_db, manager) = open(dir.path(), &putio.server.url()).await;
        manager
            .pause_items(&keys(&["transfer:900"]), &StallTracker::default(), 24)
            .await
            .unwrap();
    }

    let (db, manager) = open(dir.path(), &putio.server.url()).await;
    assert!(db.holds().is_held(900).await.unwrap());
    // The restarted manager still queues its files paused
    putio.set_status("COMPLETED").await;
    manager.enqueue_tree(0).await.unwrap();
    let row = db.queue().get_by_file_id(52).await.unwrap().unwrap();
    assert_eq!(row.status, "paused");
}

/// A transfer finished on Put.io and not yet synced can be held too (waiting for sync)
#[tokio::test]
async fn a_transfer_waiting_for_sync_can_be_held() {
    let putio = Putio::new("COMPLETED").await;
    let dir = tempfile::tempdir().unwrap();
    let (db, manager) = open(dir.path(), &putio.server.url()).await;

    let paused = manager
        .pause_items(&keys(&["transfer:900"]), &StallTracker::default(), 24)
        .await
        .unwrap();
    assert_eq!(results(paused)[0].1, ActionResult::Held);
    assert!(db.holds().is_held(900).await.unwrap());
}

/// A transfer in the NAS stage pauses its files, not the transfer; a folder row with no
/// transfer is paused the same way and is never held
#[tokio::test]
async fn pausing_a_nas_item_pauses_its_files_and_resume_puts_them_back() {
    let putio = Putio::new("COMPLETED").await;
    let dir = tempfile::tempdir().unwrap();
    let (db, manager) = open(dir.path(), &putio.server.url()).await;
    let stalls = StallTracker::default();
    manager.enqueue_tree(0).await.unwrap();
    // A folder upload with no transfer
    db.queue()
        .add(70, "up.mkv", 10, "", "Uploads", 0, QueueStatus::Pending)
        .await
        .unwrap();
    db.queue()
        .update_status(53, QueueStatus::Failed)
        .await
        .unwrap();

    let all = keys(&["transfer:900", "folder:Uploads", "file:1"]);
    let paused = manager.pause_items(&all, &stalls, 24).await.unwrap();
    assert_eq!(
        results(paused),
        vec![
            ("transfer:900".to_string(), ActionResult::Paused),
            ("folder:Uploads".to_string(), ActionResult::Paused),
            ("file:1".to_string(), ActionResult::NotFound),
        ]
    );
    let status = |id| {
        let db = db.clone();
        async move { db.queue().get_by_file_id(id).await.unwrap().unwrap().status }
    };
    assert_eq!(status(52).await, "paused");
    assert_eq!(status(53).await, "failed", "a failed file is not paused");
    assert_eq!(status(70).await, "paused");
    assert!(
        !db.holds().is_held(900).await.unwrap(),
        "no hold in the NAS stage"
    );

    // Pausing again changes nothing
    let again = manager.pause_items(&all[..2], &stalls, 24).await.unwrap();
    assert!(results(again)
        .iter()
        .all(|(_, r)| *r == ActionResult::Already));

    let resumed = manager.resume_items(&all, &stalls, 24).await.unwrap();
    assert_eq!(
        results(resumed).iter().map(|r| r.1).collect::<Vec<_>>(),
        vec![
            ActionResult::Resumed,
            ActionResult::Resumed,
            ActionResult::NotFound
        ]
    );
    assert_eq!(status(52).await, "pending");
    assert_eq!(status(70).await, "pending");
    assert_eq!(status(53).await, "failed");
    let none = manager.resume_items(&all[..1], &stalls, 24).await.unwrap();
    assert_eq!(results(none)[0].1, ActionResult::Already);
    putio.assert_nothing_sent().await;
}
