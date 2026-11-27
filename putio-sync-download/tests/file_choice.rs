//! Pick files inside a transfer (feature 070): a choice decides its transfer's files in
//! the sync, over the folder rules; other files and an explicit "Download" are untouched.

use putio_sync_client::PutioApiClient;
use putio_sync_core::config::DownloadConfig as DownloadSettings;
use putio_sync_core::rules::FolderRule;
use putio_sync_db::{Choice, Database};
use putio_sync_download::{DownloadConfig, DownloadManager};
use serde_json::json;
use std::sync::Arc;

const ROOT: &str = r#"{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false}"#;

fn entry(id: i64, name: &str, parent_id: i64) -> String {
    let folder = !name.contains('.');
    format!(
        r#"{{"id":{id},"name":"{name}","size":100,"file_type":"{}","parent_id":{parent_id},"is_folder":{folder},"is_shared":false}}"#,
        if folder { "FOLDER" } else { "VIDEO" }
    )
}

/// A mock folder: (folder id, folder name, parent id, files)
type MockFolder = (i64, &'static str, i64, &'static [(i64, &'static str)]);

/// Show (10) holds two files and Season 1 (20); Other (30) belongs to no transfer
const TREE: &[MockFolder] = &[
    (10, "Show", 0, &[(11, "readme.nfo"), (12, "sample.mkv")]),
    (20, "Season 1", 10, &[(21, "e01.mkv"), (22, "e02.mkv")]),
    (30, "Other", 0, &[(31, "film.mkv"), (32, "info.nfo")]),
];

fn transfer(id: i64, file_id: Option<i64>) -> serde_json::Value {
    json!({
        "id": id, "name": format!("T{id}"), "size": 1000, "status": "COMPLETED",
        "down_speed": 0, "up_speed": 0, "downloaded": 1000, "uploaded": 0,
        "percent_done": 100, "estimated_time": null, "peers_connected": 0,
        "peers_sending_to_us": 0, "availability": 100, "current_ratio": "0.00",
        "seconds_seeding": 0, "error_message": null,
        "created_at": "2026-10-25T12:00:00Z", "finished_at": "2026-10-25T13:00:00Z",
        "save_parent_id": 0, "file_id": file_id, "is_private": false,
        "magneturi": "", "source": null, "type": "TORRENT"
    })
}

async fn mock(server: &mut mockito::ServerGuard, path: &str, body: String) {
    server
        .mock("GET", path)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await;
}

async fn mock_tree(server: &mut mockito::ServerGuard, transfers: Vec<serde_json::Value>) {
    let mut root = Vec::new();
    for (id, name, parent, files) in TREE {
        let folder = entry(*id, name, *parent);
        if *parent == 0 {
            root.push(folder.clone());
        }
        let mut entries: Vec<String> = TREE
            .iter()
            .filter(|(_, _, p, _)| p == id)
            .map(|(cid, cname, _, _)| entry(*cid, cname, *id))
            .collect();
        entries.extend(files.iter().map(|(fid, fname)| entry(*fid, fname, *id)));
        let listing = format!(
            r#"{{"files":[{}],"parent":{ROOT},"status":"OK"}}"#,
            entries.join(",")
        );
        mock(server, &format!("/files/list?parent_id={id}"), listing).await;
        let details = format!(r#"{{"file":{folder},"status":"OK"}}"#);
        mock(server, &format!("/files/{id}"), details).await;
    }
    let listing = format!(
        r#"{{"files":[{}],"parent":{ROOT},"status":"OK"}}"#,
        root.join(",")
    );
    mock(server, "/files/list?parent_id=0", listing).await;
    for t in &transfers {
        let body = json!({ "status": "OK", "transfer": t }).to_string();
        mock(server, &format!("/transfers/{}", t["id"]), body).await;
    }
    let body = json!({ "status": "OK", "transfers": transfers }).to_string();
    mock(server, "/transfers/list", body).await;
}

/// Video only in Show, so the rules alone would block readme.nfo
fn video_only() -> Vec<FolderRule> {
    vec![FolderRule {
        path: "Show".to_string(),
        preset: Some("video".to_string()),
        ..FolderRule::default()
    }]
}

async fn setup(
    rules: Vec<FolderRule>,
    transfers: Vec<serde_json::Value>,
) -> (
    mockito::ServerGuard,
    tempfile::TempDir,
    Arc<Database>,
    DownloadManager,
) {
    let mut server = mockito::Server::new_async().await;
    mock_tree(&mut server, transfers).await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(PutioApiClient::with_base_url(
        "test-putio-token".to_string(),
        server.url(),
    ));
    let mut config = DownloadConfig::from(&DownloadSettings {
        target_directory: dir.path().to_path_buf(),
        rules,
        ..DownloadSettings::default()
    });
    config.max_retries = 0;
    config.retry_initial_delay_secs = 0;
    let manager = DownloadManager::new(client, db.clone(), config);
    (server, dir, db, manager)
}

fn choice(
    transfer_id: i64,
    root: Option<i64>,
    others_wanted: bool,
    files: &[(&str, bool)],
) -> Choice {
    Choice {
        transfer_id,
        root_file_id: root,
        others_wanted,
        pending: false,
        files: files.iter().map(|(p, w)| (p.to_string(), *w)).collect(),
    }
}

async fn queued(db: &Database) -> Vec<i64> {
    let mut ids: Vec<i64> = db
        .queue()
        .get_all()
        .await
        .unwrap()
        .into_iter()
        .map(|item| item.file_id)
        .collect();
    ids.sort();
    ids
}

/// "Only these": the ticked files are queued even where a rule blocks their type, the
/// rest of the transfer stays on Put.io with the reason recorded, and files of no
/// transfer follow the rules as before
#[tokio::test]
async fn a_choice_wins_over_the_rules_for_its_transfer_only() {
    let (_server, _dir, db, manager) = setup(video_only(), vec![]).await;
    let picked = choice(
        1,
        Some(10),
        false,
        &[("readme.nfo", true), ("Season 1/e01.mkv", true)],
    );
    db.choices().set(&picked).await.unwrap();

    let stats = manager.sync_downloads().await.unwrap();
    assert_eq!(queued(&db).await, vec![11, 21, 31, 32]);
    assert_eq!(stats.files_blocked, 2, "sample.mkv and e02.mkv left out");
    assert_eq!(stats.files_deleted, 0);

    let decisions = db.rule_decisions().recent(10).await.unwrap();
    let mut left: Vec<i64> = decisions.iter().map(|d| d.file_id).collect();
    left.sort();
    assert_eq!(left, vec![12, 22]);
    assert!(decisions
        .iter()
        .all(|d| d.decision == "skipped" && d.reason == "Left out by your choice"));
}

/// "All but this", and a file that shows up later follows `others`
#[tokio::test]
async fn all_but_one_takes_the_rest() {
    let (_server, _dir, db, manager) = setup(vec![], vec![]).await;
    db.choices()
        .set(&choice(1, Some(10), true, &[("sample.mkv", false)]))
        .await
        .unwrap();
    manager.sync_downloads().await.unwrap();
    assert_eq!(queued(&db).await, vec![11, 21, 22, 31, 32]);
}

/// An explicit "Download" of the folder takes every file, choice or not
#[tokio::test]
async fn an_explicit_download_ignores_the_choice() {
    let (_server, _dir, db, manager) = setup(vec![], vec![]).await;
    db.choices()
        .set(&choice(1, Some(10), false, &[]))
        .await
        .unwrap();
    manager.enqueue_tree(10).await.unwrap();
    assert_eq!(queued(&db).await, vec![11, 12, 21, 22]);
}

/// A choice made before Put.io had the files (a magnet added to choose first) holds
/// its transfer back once the files exist; one whose transfer is gone is forgotten
#[tokio::test]
async fn a_choice_made_early_is_matched_when_the_files_exist() {
    let transfers = vec![transfer(5, Some(30)), transfer(6, None)];
    let (_server, _dir, db, manager) = setup(vec![], transfers).await;
    let waiting = |id| Choice {
        pending: true,
        ..choice(id, None, false, &[])
    };
    for id in [5, 6, 7] {
        db.choices().set(&waiting(id)).await.unwrap();
    }

    manager.sync_downloads().await.unwrap();
    assert_eq!(queued(&db).await, vec![11, 12, 21, 22], "Other waits");
    let stored = db.choices().all().await.unwrap();
    let ids: Vec<(i64, Option<i64>)> = stored
        .iter()
        .map(|c| (c.transfer_id, c.root_file_id))
        .collect();
    assert_eq!(ids, vec![(5, Some(30)), (6, None)], "7 is not on Put.io");
}

/// A choice whose files were deleted from Put.io is forgotten by a full sync
#[tokio::test]
async fn a_choice_is_forgotten_with_its_files() {
    let (_server, _dir, db, manager) = setup(vec![], vec![]).await;
    db.choices()
        .set(&choice(1, Some(999), false, &[]))
        .await
        .unwrap();
    manager.sync_downloads().await.unwrap();
    assert!(db.choices().all().await.unwrap().is_empty());
}

/// The picker's listing names files by their path under the transfer's item, and
/// saving a choice takes newly unticked files out of the queue; ticking them again
/// queues them and drops the "left out" record
#[tokio::test]
async fn saving_a_choice_applies_it_to_the_queue() {
    let (_server, _dir, db, manager) = setup(vec![], vec![transfer(7, Some(10))]).await;
    let listed = manager.transfer_files(7).await.unwrap();
    assert_eq!(listed.name, "T7");
    assert_eq!(listed.root_file_id, Some(10));
    let paths: Vec<(i64, &str)> = listed
        .files
        .iter()
        .map(|f| (f.file_id, f.path.as_str()))
        .collect();
    assert_eq!(
        paths,
        vec![
            (21, "Season 1/e01.mkv"),
            (22, "Season 1/e02.mkv"),
            (11, "readme.nfo"),
            (12, "sample.mkv")
        ]
    );

    manager.sync_downloads().await.unwrap();
    assert_eq!(queued(&db).await, vec![11, 12, 21, 22, 31, 32]);

    let removed = manager
        .set_choice(choice(7, None, true, &[("sample.mkv", false)]))
        .await
        .unwrap();
    assert_eq!(removed, 1);
    assert_eq!(
        db.choices().get(7).await.unwrap().unwrap().root_file_id,
        Some(10)
    );
    // Saving asks for a sync, which runs on its own task
    settle(&db, &[11, 21, 22, 31, 32], 1).await;

    assert!(manager.clear_choice(7).await.unwrap());
    settle(&db, &[11, 12, 21, 22, 31, 32], 0).await;
    assert!(!manager.clear_choice(7).await.unwrap());
}

/// Wait for the requested sync to leave the queue and the recorded decisions like this
async fn settle(db: &Database, queue: &[i64], decisions: usize) {
    for _ in 0..200 {
        let recorded = db.rule_decisions().recent(10).await.unwrap().len();
        if queued(db).await == queue && recorded == decisions {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert_eq!(queued(db).await, queue);
    assert_eq!(
        db.rule_decisions().recent(10).await.unwrap().len(),
        decisions
    );
}
