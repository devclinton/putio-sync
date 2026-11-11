//! Plugin sync rules in the download manager (feature 028): a filter changes the sync's
//! decision, a folder rule and a rename rule the local path. A fake stands in for the
//! plugins; the *arr's files are never asked about.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use putio_sync_client::{PutioApiClient, RetryPolicy};
use putio_sync_core::config::DownloadConfig as DownloadSettings;
use putio_sync_core::rules::FolderRule;
use putio_sync_core::sync_hooks::{FilterAnswer, Hook, HookFile, HookFuture, SyncHooks};
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadFile, DownloadManager};

const ROOT: &str = r#"{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false}"#;

#[derive(Default)]
struct Fake {
    filter: HashMap<&'static str, FilterAnswer>,
    folder: Option<&'static str>,
    rename: Option<&'static str>,
    asked: Mutex<Vec<(Hook, HookFile)>>,
}

impl Fake {
    fn asked(&self, hook: Hook) -> Vec<HookFile> {
        let asked = self.asked.lock().unwrap();
        asked
            .iter()
            .filter(|(h, _)| *h == hook)
            .map(|(_, f)| f.clone())
            .collect()
    }
}

impl SyncHooks for Fake {
    fn provides(&self, hook: Hook) -> bool {
        match hook {
            Hook::Filter => !self.filter.is_empty(),
            Hook::Folder => self.folder.is_some(),
            Hook::Rename => self.rename.is_some(),
        }
    }

    fn filter<'a>(&'a self, file: &'a HookFile) -> HookFuture<'a, Option<(FilterAnswer, String)>> {
        self.asked
            .lock()
            .unwrap()
            .push((Hook::Filter, file.clone()));
        let answer = self.filter.get(file.name.as_str()).copied();
        Box::pin(async move { answer.map(|a| (a, "fake".to_string())) })
    }

    fn rename<'a>(&'a self, file: &'a HookFile) -> HookFuture<'a, Option<String>> {
        self.asked
            .lock()
            .unwrap()
            .push((Hook::Rename, file.clone()));
        Box::pin(async move { self.rename.map(str::to_string) })
    }

    fn folder<'a>(&'a self, file: &'a HookFile) -> HookFuture<'a, Option<String>> {
        self.asked
            .lock()
            .unwrap()
            .push((Hook::Folder, file.clone()));
        Box::pin(async move { self.folder.map(str::to_string) })
    }
}

fn entry(id: i64, name: &str, parent_id: i64, size: u64) -> String {
    let folder = !name.contains('.');
    format!(
        r#"{{"id":{id},"name":"{name}","size":{size},"file_type":"{}","parent_id":{parent_id},"is_folder":{folder},"is_shared":false}}"#,
        if folder { "FOLDER" } else { "VIDEO" }
    )
}

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

/// Root: `loose.mkv` (1) and `Movies` (10) with `film.mkv` (11), `sample.mkv` (12) and
/// `setup.exe` (13, blocked by default)
async fn mock_tree(server: &mut mockito::ServerGuard) {
    let movies = entry(10, "Movies", 0, 0);
    list(
        server,
        0,
        vec![entry(1, "loose.mkv", 0, 100), movies.clone()],
    )
    .await;
    list(
        server,
        10,
        vec![
            entry(11, "film.mkv", 10, 100),
            entry(12, "sample.mkv", 10, 5),
            entry(13, "setup.exe", 10, 100),
        ],
    )
    .await;
    server
        .mock("GET", "/files/10")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"file":{movies},"status":"OK"}}"#))
        .create_async()
        .await;
    server
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"transfers":[],"status":"OK"}"#)
        .create_async()
        .await;
}

struct Setup {
    server: mockito::ServerGuard,
    dir: tempfile::TempDir,
    db: Arc<Database>,
    manager: DownloadManager,
}

async fn setup(settings: DownloadSettings, hooks: Arc<Fake>) -> Setup {
    let mut server = mockito::Server::new_async().await;
    mock_tree(&mut server).await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(
        PutioApiClient::with_base_url("test_token".to_string(), server.url()).with_retry_policy(
            RetryPolicy {
                max_attempts: 1,
                initial_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(1),
            },
        ),
    );
    let mut config = DownloadConfig::from(&DownloadSettings {
        target_directory: dir.path().to_path_buf(),
        ..settings
    });
    config.max_retries = 0;
    config.retry_initial_delay_secs = 0;
    let manager = DownloadManager::new(client, db.clone(), config);
    manager.set_sync_hooks(hooks);
    Setup {
        server,
        dir,
        db,
        manager,
    }
}

async fn queued(db: &Database, id: i64) -> bool {
    db.queue().get_by_file_id(id).await.unwrap().is_some()
}

#[tokio::test]
async fn a_filter_changes_the_sync_decision_and_is_never_a_delete() {
    let hooks = Arc::new(Fake {
        filter: HashMap::from([
            ("sample.mkv", FilterAnswer::Skip),
            ("setup.exe", FilterAnswer::Download),
            ("loose.mkv", FilterAnswer::Stream),
        ]),
        ..Fake::default()
    });
    let mut s = setup(
        DownloadSettings {
            rules: vec![FolderRule {
                path: "Movies".into(),
                delete_blocked: Some(true),
                ..FolderRule::default()
            }],
            ..DownloadSettings::default()
        },
        hooks.clone(),
    )
    .await;
    let delete = s
        .server
        .mock("POST", "/files/delete")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(0)
        .create_async()
        .await;

    let stats = s.manager.sync_downloads().await.unwrap();
    assert_eq!(stats.files_blocked, 1, "sample.mkv");
    assert_eq!(stats.files_streamed, 1, "loose.mkv");
    assert!(
        queued(&s.db, 11).await,
        "no answer keeps the built-in download"
    );
    assert!(
        queued(&s.db, 13).await,
        "the plugin overrides the blocklist"
    );
    assert!(!queued(&s.db, 12).await);
    assert!(!queued(&s.db, 1).await);
    assert!(s.db.stream_files().get(1).await.unwrap().is_some());
    delete.assert_async().await;

    let asked = hooks.asked(Hook::Filter);
    let exe = asked.iter().find(|f| f.name == "setup.exe").unwrap();
    assert_eq!(exe.decision, "blocked");
    assert_eq!(exe.extension, "exe");
    assert_eq!(exe.folder, "Movies");
    assert!(exe.reason.contains("exe"), "{}", exe.reason);
    let film = asked.iter().find(|f| f.name == "film.mkv").unwrap();
    assert_eq!((film.decision.as_str(), film.size), ("download", 100));
}

#[tokio::test]
async fn the_arrs_files_are_not_filtered() {
    let hooks = Arc::new(Fake {
        filter: HashMap::from([("film.mkv", FilterAnswer::Skip)]),
        ..Fake::default()
    });
    let s = setup(DownloadSettings::default(), hooks.clone()).await;
    s.manager.enqueue_tree_report(10, true).await.unwrap();
    assert!(queued(&s.db, 11).await);
    assert!(hooks.asked(Hook::Filter).is_empty());
}

fn download(
    server: &mockito::ServerGuard,
    fixed_path: bool,
    save_as: Option<&str>,
) -> DownloadFile {
    DownloadFile {
        file_id: 5,
        filename: "Show.S01E02.1080p.mkv".to_string(),
        relative_path: "Put.io folder".to_string(),
        file_size: 4,
        download_url: format!("{}/files/5/download", server.url()),
        crc32: None,
        save_as: save_as.map(str::to_string),
        fixed_path,
        overwrite: true,
    }
}

#[tokio::test]
async fn folder_and_rename_rules_place_a_download() {
    let hooks = Arc::new(Fake {
        folder: Some("TV/Show/Season 01"),
        rename: Some("Show S01E02.mkv"),
        ..Fake::default()
    });
    let mut s = setup(DownloadSettings::default(), hooks.clone()).await;
    let _body = s
        .server
        .mock("GET", "/files/5/download")
        .with_body("data")
        .create_async()
        .await;

    let done = s
        .manager
        .download_file(download(&s.server, false, None), None)
        .await;
    assert!(done.unwrap().success);
    let placed = s.dir.path().join("TV/Show/Season 01/Show S01E02.mkv");
    assert_eq!(std::fs::read(&placed).unwrap(), b"data");
    let asked = hooks.asked(Hook::Rename);
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].folder, "Put.io folder");
    assert_eq!(asked[0].decision, "download");

    // A numbered name from an earlier attempt is kept; the folder rule still applies
    let done = s
        .manager
        .download_file(download(&s.server, false, Some("Show (1).mkv")), None)
        .await;
    assert!(done.unwrap().success);
    assert!(s
        .dir
        .path()
        .join("TV/Show/Season 01/Show (1).mkv")
        .is_file());

    // The *arr's files keep their place
    let done = s
        .manager
        .download_file(download(&s.server, true, None), None)
        .await;
    assert!(done.unwrap().success);
    assert!(s
        .dir
        .path()
        .join("Put.io folder/Show.S01E02.1080p.mkv")
        .is_file());
    assert_eq!(hooks.asked(Hook::Rename).len(), 1);
    assert_eq!(hooks.asked(Hook::Folder).len(), 2);
}
