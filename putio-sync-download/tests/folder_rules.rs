//! Folder rules applied by the sync (feature 009): type presets, skip, inheritance,
//! delete blocked, preview, and per-folder delete-after-download / skip-existing.

use mockito::Matcher;
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::{DownloadConfig as DownloadSettings, ExistingFile};
use putio_sync_core::rules::{Decision, FolderRule};
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager, TreeDecision};
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

/// The mock tree
const TREE: &[MockFolder] = &[
    (
        10,
        "Movies",
        0,
        &[(11, "film.mkv"), (12, "info.nfo"), (13, "setup.exe")],
    ),
    (20, "Extras", 10, &[(21, "notes.txt")]),
    (30, "Music", 0, &[(31, "song.flac"), (32, "cover.jpg")]),
    (40, "Seeding", 0, &[(41, "x.mkv")]),
    (50, "Software", 0, &[(51, "tool.exe")]),
];

async fn list(server: &mut mockito::ServerGuard, id: i64, entries: Vec<String>) {
    server
        .mock("GET", format!("/files/list?parent_id={id}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"files":[{}],"parent":{ROOT},"status":"OK"}}"#,
            entries.join(",")
        ))
        .create_async()
        .await;
}

async fn mock_tree(server: &mut mockito::ServerGuard) {
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
        list(server, *id, entries).await;
        server
            .mock("GET", format!("/files/{id}").as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(r#"{{"file":{folder},"status":"OK"}}"#))
            .create_async()
            .await;
    }
    list(server, 0, root).await;
    server
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"transfers":[],"status":"OK"}"#)
        .create_async()
        .await;
}

fn rule(path: &str) -> FolderRule {
    FolderRule {
        path: path.to_string(),
        ..FolderRule::default()
    }
}

fn preset(path: &str, preset: &str) -> FolderRule {
    FolderRule {
        preset: Some(preset.to_string()),
        ..rule(path)
    }
}

fn the_rules() -> Vec<FolderRule> {
    vec![
        preset("Movies", "video"),
        preset("Movies/Extras", "everything"),
        FolderRule {
            allow: vec!["jpg".into()],
            ..preset("Music", "audio")
        },
        FolderRule {
            skip: Some(true),
            ..rule("Seeding")
        },
        FolderRule {
            allow: vec!["exe".into()],
            ..rule("Software")
        },
    ]
}

fn config(dir: &std::path::Path, rules: Vec<FolderRule>) -> DownloadConfig {
    let mut config = DownloadConfig::from(&DownloadSettings {
        target_directory: dir.to_path_buf(),
        rules,
        ..DownloadSettings::default()
    });
    config.max_retries = 0;
    config.retry_initial_delay_secs = 0;
    config
}

async fn setup(
    rules: Vec<FolderRule>,
) -> (
    mockito::ServerGuard,
    tempfile::TempDir,
    Arc<Database>,
    DownloadManager,
) {
    let mut server = mockito::Server::new_async().await;
    mock_tree(&mut server).await;
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
    let manager = DownloadManager::new(client, db.clone(), config(dir.path(), rules));
    (server, dir, db, manager)
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

#[tokio::test]
async fn presets_skip_and_inheritance_decide_the_queue() {
    let (_server, _dir, db, manager) = setup(the_rules()).await;

    let stats = manager.enqueue_tree(0).await.unwrap();
    assert_eq!(stats.total_files, 8);
    assert_eq!(queued(&db).await, vec![11, 21, 31, 32, 51]);
    assert_eq!(
        stats.files_blocked, 3,
        "nfo and exe in Movies, Seeding skipped"
    );
    assert_eq!(stats.files_deleted, 0);

    let decisions = db.rule_decisions().recent(10).await.unwrap();
    assert_eq!(decisions.len(), 3);
    let nfo = decisions.iter().find(|d| d.file_id == 12).unwrap();
    assert_eq!(nfo.decision, "blocked");
    assert_eq!(nfo.rule.as_deref(), Some("Movies"));
    assert_eq!(nfo.path, "Movies");
    assert_eq!(nfo.reason, "extension nfo is not allowed by Video only");
    let skipped = decisions.iter().find(|d| d.file_id == 41).unwrap();
    assert_eq!(skipped.decision, "skipped");
    assert_eq!(skipped.rule.as_deref(), Some("Seeding"));
}

/// The tree report (feature 011) carries rule decisions: blocked with the extension,
/// skipped folders as skipped
#[tokio::test]
async fn the_tree_report_shows_rule_decisions() {
    let (_server, _dir, _db, manager) = setup(the_rules()).await;
    let report = manager.enqueue_tree_report(0, false).await.unwrap();
    let decision = |id: i64| {
        report
            .files
            .iter()
            .find(|f| f.file_id == id)
            .map(|f| f.decision.clone())
            .unwrap()
    };
    assert_eq!(decision(11), TreeDecision::Queued);
    assert_eq!(decision(12), TreeDecision::Blocked("nfo".to_string()));
    assert_eq!(decision(41), TreeDecision::Skipped);
    assert!(!decision(41).accepted());
}

#[tokio::test]
async fn a_file_queued_later_loses_its_decision() {
    let (server, _dir, db, manager) = setup(vec![preset("Movies", "video")]).await;
    manager.enqueue_tree(10).await.unwrap();
    assert!(db
        .rule_decisions()
        .recent(10)
        .await
        .unwrap()
        .iter()
        .any(|d| d.file_id == 12));

    // The rule is cleared: the nfo is queued and its decision removed
    manager.update_config(config(_dir.path(), vec![])).await;
    manager.enqueue_tree(10).await.unwrap();
    assert!(queued(&db).await.contains(&12));
    let left: Vec<i64> = db
        .rule_decisions()
        .recent(10)
        .await
        .unwrap()
        .iter()
        .map(|d| d.file_id)
        .collect();
    assert_eq!(left, vec![13], "only setup.exe is still blocked");
    drop(server);
}

#[tokio::test]
async fn delete_blocked_deletes_only_unqueued_type_blocked_files() {
    let mut rules = the_rules();
    rules[0].delete_blocked = Some(true);
    // Skipped files are never deleted, even with delete_blocked on
    rules[3].delete_blocked = Some(true);
    let (mut server, _dir, db, manager) = setup(rules).await;
    // setup.exe was queued by hand before the rule: it is left alone
    db.queue()
        .add(13, "setup.exe", 100, "http://unused", "Movies", 0)
        .await
        .unwrap();
    let delete = server
        .mock("POST", "/files/delete")
        .match_body(Matcher::PartialJsonString(r#"{"file_ids":"12"}"#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(1)
        .create_async()
        .await;

    let stats = manager.enqueue_tree(0).await.unwrap();
    delete.assert_async().await;
    assert_eq!(stats.files_deleted, 1);
    let decisions = db.rule_decisions().recent(10).await.unwrap();
    let nfo = decisions.iter().find(|d| d.file_id == 12).unwrap();
    assert_eq!(nfo.decision, "deleted");
    assert_eq!(nfo.rule.as_deref(), Some("Movies"));
    let skipped = decisions.iter().find(|d| d.file_id == 41).unwrap();
    assert_eq!(skipped.decision, "skipped");
}

#[tokio::test]
async fn preview_changes_nothing() {
    let mut rules = the_rules();
    rules[0].delete_blocked = Some(true);
    let (mut server, dir, db, manager) = setup(vec![]).await;
    let delete = server
        .mock("POST", "/files/delete")
        .expect(0)
        .create_async()
        .await;

    let candidate = config(dir.path(), rules);
    let preview = manager.preview_rules(10, &candidate).await.unwrap();
    assert_eq!(preview.files, 4);
    assert_eq!(preview.download, 2, "film.mkv and Extras/notes.txt");
    assert_eq!(preview.blocked, 2);
    assert_eq!(preview.would_delete, 2);
    assert_eq!(preview.skipped, 0);
    assert!(!preview.truncated);
    let names: Vec<&str> = preview.items.iter().map(|i| i.name.as_str()).collect();
    assert!(
        names.contains(&"info.nfo") && names.contains(&"setup.exe"),
        "{names:?}"
    );
    assert!(matches!(
        &preview.items[0].decision,
        Decision::Blocked { rule: Some(r), .. } if r == "Movies"
    ));

    delete.assert_async().await;
    assert!(queued(&db).await.is_empty());
    assert!(db.rule_decisions().recent(10).await.unwrap().is_empty());
}

#[tokio::test]
async fn folder_path_of_a_folder() {
    let (_server, _dir, _db, manager) = setup(vec![]).await;
    assert_eq!(manager.putio_folder_path(0).await.unwrap(), "");
    assert_eq!(
        manager.putio_folder_path(20).await.unwrap(),
        "Movies/Extras"
    );
}

/// Download the queued `Movies/Extras/notes.txt` (id 21). Returns whether Put.io was
/// asked to delete it and the local file's contents.
async fn download_extras(
    rules: Vec<FolderRule>,
    delete_after_download: bool,
    existing: bool,
) -> (bool, Vec<u8>) {
    let mut server = mockito::Server::new_async().await;
    mock_tree(&mut server).await;
    server
        .mock("GET", "/files/21")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"file":{},"status":"OK"}}"#,
            entry(21, "notes.txt", 20)
        ))
        .create_async()
        .await;
    server
        .mock("GET", "/files/21/download")
        .with_status(200)
        .with_body(vec![7u8; 100])
        .create_async()
        .await;
    let delete = server
        .mock("POST", "/files/delete")
        .match_body(Matcher::PartialJsonString(r#"{"file_ids":"21"}"#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .create_async()
        .await;

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
    let mut cfg = DownloadConfig::from(&DownloadSettings {
        target_directory: dir.path().to_path_buf(),
        delete_after_download,
        rules,
        ..DownloadSettings::default()
    });
    cfg.max_retries = 0;
    let manager = DownloadManager::new(client, db.clone(), cfg);

    let target = dir.path().join("Movies/Extras/notes.txt");
    if existing {
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        // Same size and no Put.io checksum: complete, as far as "resume" can tell
        std::fs::write(&target, [1u8; 100]).unwrap();
    }
    db.queue()
        .add(21, "notes.txt", 100, "http://unused", "Movies/Extras", 0)
        .await
        .unwrap();
    assert!(manager.process_queue().await.unwrap());
    (
        delete.matched_async().await,
        std::fs::read(&target).unwrap(),
    )
}

#[tokio::test]
async fn delete_after_download_per_folder() {
    let on = |path: &str, value: bool| FolderRule {
        delete_after_download: Some(value),
        ..rule(path)
    };
    // Global off, Movies on: inherited by Movies/Extras
    assert!(
        download_extras(vec![on("Movies", true)], false, false)
            .await
            .0
    );
    // Global on, Movies/Extras off
    assert!(
        !download_extras(vec![on("Movies/Extras", false)], true, false)
            .await
            .0
    );
    // Global on, a rule elsewhere
    assert!(
        download_extras(vec![on("Music", false)], true, false)
            .await
            .0
    );
    // No rules, global off
    assert!(!download_extras(vec![], false, false).await.0);
}

#[tokio::test]
async fn on_existing_per_folder() {
    let with = |path: &str, choice: ExistingFile| FolderRule {
        on_existing: Some(choice),
        ..rule(path)
    };
    // Global resume: the complete-looking file stays
    assert_eq!(download_extras(vec![], false, true).await.1, [1u8; 100]);
    // Movies says overwrite, inherited by Movies/Extras: replaced
    assert_eq!(
        download_extras(vec![with("Movies", ExistingFile::Overwrite)], false, true)
            .await
            .1,
        [7u8; 100]
    );
    // The nearest rule wins
    let rules = vec![
        with("Movies", ExistingFile::Overwrite),
        with("Movies/Extras", ExistingFile::Resume),
    ];
    assert_eq!(download_extras(rules, false, true).await.1, [1u8; 100]);
    // A rule with only the old `skip_existing = false` acts as overwrite
    let old = FolderRule {
        skip_existing: Some(false),
        ..rule("Movies")
    };
    assert_eq!(download_extras(vec![old], false, true).await.1, [7u8; 100]);
}
