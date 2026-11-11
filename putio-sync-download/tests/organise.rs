//! Organising files kept on Put.io (feature 023): preview, apply, undo, auto, scan and
//! plugin answers, against a mock Put.io.

use putio_sync_client::{PutioApiClient, RetryPolicy};
use putio_sync_core::config::{DownloadConfig as DownloadSettings, SyncMode};
use putio_sync_core::organise::{OrganiseApply, OrganiseConfig};
use putio_sync_core::sync_hooks::{FilterAnswer, Hook, HookFile, HookFuture, SyncHooks};
use putio_sync_db::{Database, MoveStatus, NewMove};
use putio_sync_download::{
    ArrivedFile, ArrivedStep, DownloadConfig, DownloadManager, OrganiseStep, Organiser, StepResult,
};
use std::sync::Arc;
use std::time::Duration;

const ROOT: &str = r#"{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false}"#;
const RELEASE: &str = "Show.Name.S02E05.Title.Here.1080p.WEB-DL.x264-GRP.mkv";
const TARGET: &str = "Show Name - S02E05 - Title Here.mkv";
/// The file and the release folder it sits in
const FILE: i64 = 5;
const RELEASE_FOLDER: i64 = 30;

fn entry(id: i64, name: &str, parent_id: i64) -> String {
    let folder = !name.contains('.');
    format!(
        r#"{{"id":{id},"name":"{name}","size":100,"file_type":"{}","parent_id":{parent_id},"is_folder":{folder},"is_shared":false}}"#,
        if folder { "FOLDER" } else { "VIDEO" }
    )
}

async fn list(server: &mut mockito::ServerGuard, id: i64, entries: &[String]) {
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

async fn details(server: &mut mockito::ServerGuard, id: i64, name: &str, parent: i64) {
    server
        .mock("GET", format!("/files/{id}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"file":{},"status":"OK"}}"#,
            entry(id, name, parent)
        ))
        .create_async()
        .await;
}

async fn gone(server: &mut mockito::ServerGuard, id: i64) {
    server
        .mock("GET", format!("/files/{id}").as_str())
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"ERROR","error_type":"NotFound","error_message":"not found"}"#)
        .create_async()
        .await;
}

async fn make_folder(server: &mut mockito::ServerGuard, name: &str, parent: i64, id: i64) {
    server
        .mock("POST", "/files/create-folder")
        .match_body(mockito::Matcher::Json(
            serde_json::json!({"name": name, "parent_id": parent}),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"file":{},"status":"OK"}}"#,
            entry(id, name, parent)
        ))
        .expect(1)
        .create_async()
        .await;
}

async fn rename(server: &mut mockito::ServerGuard, name: &str, hits: usize) -> mockito::Mock {
    server
        .mock("POST", "/files/rename")
        .match_body(mockito::Matcher::Json(
            serde_json::json!({"file_id": FILE, "name": name}),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(hits)
        .create_async()
        .await
}

async fn move_to(server: &mut mockito::ServerGuard, parent: i64, hits: usize) -> mockito::Mock {
    server
        .mock("POST", "/files/move")
        .match_body(mockito::Matcher::Json(
            serde_json::json!({"file_ids": FILE.to_string(), "parent_id": parent}),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(hits)
        .create_async()
        .await
}

/// No rename or move call at all
async fn no_writes(server: &mut mockito::ServerGuard) -> (mockito::Mock, mockito::Mock) {
    let r = server
        .mock("POST", "/files/rename")
        .expect(0)
        .create_async()
        .await;
    let m = server
        .mock("POST", "/files/move")
        .expect(0)
        .create_async()
        .await;
    (r, m)
}

struct Setup {
    organiser: Arc<Organiser>,
    manager: Arc<DownloadManager>,
    db: Arc<Database>,
    _dir: tempfile::TempDir,
}

async fn setup(server: &mockito::ServerGuard, mode: SyncMode, organise: OrganiseConfig) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let client = Arc::new(
        PutioApiClient::with_base_url("test_token".to_string(), server.url()).with_retry_policy(
            RetryPolicy {
                max_attempts: 1,
                initial_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(1),
            },
        ),
    );
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let mut config = DownloadConfig::from(&DownloadSettings {
        target_directory: dir.path().to_path_buf(),
        mode,
        ..Default::default()
    });
    config.max_retries = 0;
    let manager = Arc::new(DownloadManager::new(client.clone(), db.clone(), config));
    let organiser = Arc::new(Organiser::new(client, db.clone(), &manager, organise));
    Setup {
        organiser,
        manager,
        db,
        _dir: dir,
    }
}

fn tv(apply: OrganiseApply) -> OrganiseConfig {
    OrganiseConfig {
        tv: true,
        apply,
        ..Default::default()
    }
}

fn release() -> ArrivedFile {
    ArrivedFile {
        file_id: FILE,
        name: RELEASE.to_string(),
        folder: "Show.Name.S02E05.1080p".to_string(),
        size: 100,
    }
}

/// `TV Shows/Show Name/Season 02` does not exist yet: made as 100, 101, 102
async fn new_folders(server: &mut mockito::ServerGuard) {
    list(server, 0, &[entry(RELEASE_FOLDER, "Show-Name-S02E05", 0)]).await;
    make_folder(server, "TV Shows", 0, 100).await;
    list(server, 100, &[]).await;
    make_folder(server, "Show Name", 100, 101).await;
    list(server, 101, &[]).await;
    make_folder(server, "Season 02", 101, 102).await;
}

#[tokio::test]
async fn a_preview_pass_plans_and_writes_nothing() {
    let mut server = mockito::Server::new_async().await;
    let (r, m) = no_writes(&mut server).await;
    let s = setup(&server, SyncMode::Stream, tv(OrganiseApply::Preview)).await;
    let step = OrganiseStep::new(s.organiser.clone());
    assert_eq!(step.name(), "organise");
    assert!(step.enabled(&s.manager.get_config().await));

    assert_eq!(step.run(&release()).await, StepResult::Done);
    let moves = s.organiser.list().await.unwrap();
    assert_eq!(moves.len(), 1);
    let planned = &moves[0];
    assert_eq!(planned.status, MoveStatus::Planned);
    assert_eq!(planned.to_folder, "TV Shows/Show Name/Season 02");
    assert_eq!(planned.to_name, TARGET);
    assert!(!planned.by_plugin);

    // Not an episode: nothing planned
    let readme = ArrivedFile {
        file_id: 6,
        name: "README.txt".to_string(),
        ..release()
    };
    assert_eq!(step.run(&readme).await, StepResult::Done);
    assert_eq!(s.organiser.list().await.unwrap().len(), 1);
    r.assert_async().await;
    m.assert_async().await;

    // Off: the step is not recorded for new files
    s.organiser.configure(OrganiseConfig::default());
    assert!(!step.enabled(&s.manager.get_config().await));
}

#[tokio::test]
async fn apply_makes_the_folders_then_renames_and_moves() {
    let mut server = mockito::Server::new_async().await;
    details(&mut server, FILE, RELEASE, RELEASE_FOLDER).await;
    new_folders(&mut server).await;
    list(&mut server, 102, &[]).await;
    let r = rename(&mut server, TARGET, 1).await;
    let m = move_to(&mut server, 102, 1).await;
    let s = setup(&server, SyncMode::Stream, tv(OrganiseApply::Preview)).await;
    s.organiser.plan(&release()).await.unwrap().unwrap();

    let out = s.organiser.apply(&[FILE, 999]).await;
    assert_eq!(out[0].status, "applied", "{:?}", out[0].error);
    assert_eq!(out[1].status, "missing");
    r.assert_async().await;
    m.assert_async().await;
    let moved = s.db.organise_moves().get(FILE).await.unwrap().unwrap();
    assert_eq!(moved.status, MoveStatus::Applied);
    assert_eq!(moved.from_parent_id, Some(RELEASE_FOLDER));
    assert_eq!(moved.from_name, RELEASE);

    // Applying again does nothing
    assert_eq!(s.organiser.apply(&[FILE]).await[0].status, "skipped");
    // An applied move is not dismissed
    assert_eq!(s.organiser.dismiss(&[FILE]).await[0].status, "skipped");
}

#[tokio::test]
async fn a_name_already_taken_fails_and_changes_nothing() {
    let mut server = mockito::Server::new_async().await;
    details(&mut server, FILE, RELEASE, RELEASE_FOLDER).await;
    list(&mut server, 0, &[entry(100, "TV Shows", 0)]).await;
    list(&mut server, 100, &[entry(101, "Show Name", 100)]).await;
    list(&mut server, 101, &[entry(102, "Season 02", 101)]).await;
    list(&mut server, 102, &[entry(9, TARGET, 102)]).await;
    let (r, m) = no_writes(&mut server).await;
    let s = setup(&server, SyncMode::Stream, tv(OrganiseApply::Preview)).await;
    s.organiser.plan(&release()).await.unwrap();

    let out = s.organiser.apply(&[FILE]).await;
    assert_eq!(out[0].status, "failed");
    assert!(out[0].error.as_deref().unwrap().contains("already exists"));
    r.assert_async().await;
    m.assert_async().await;
    let failed = s.db.organise_moves().get(FILE).await.unwrap().unwrap();
    assert_eq!(failed.status, MoveStatus::Failed);

    // A failed move can be dismissed
    assert_eq!(s.organiser.dismiss(&[FILE]).await[0].status, "dismissed");
    assert!(s.organiser.list().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_target_outside_stream_mode_is_refused() {
    let mut server = mockito::Server::new_async().await;
    let (r, m) = no_writes(&mut server).await;
    let s = setup(&server, SyncMode::Download, tv(OrganiseApply::Preview)).await;
    s.organiser.plan(&release()).await.unwrap();

    let out = s.organiser.apply(&[FILE]).await;
    assert_eq!(out[0].status, "failed");
    assert!(out[0]
        .error
        .as_deref()
        .unwrap()
        .contains("not in stream mode"));
    r.assert_async().await;
    m.assert_async().await;
}

#[tokio::test]
async fn a_file_gone_from_put_io_fails() {
    let mut server = mockito::Server::new_async().await;
    gone(&mut server, FILE).await;
    let (r, m) = no_writes(&mut server).await;
    let s = setup(&server, SyncMode::Stream, tv(OrganiseApply::Preview)).await;
    s.organiser.plan(&release()).await.unwrap();

    let out = s.organiser.apply(&[FILE]).await;
    assert_eq!(out[0].status, "failed");
    assert_eq!(out[0].error.as_deref(), Some("no longer on Put.io"));
    r.assert_async().await;
    m.assert_async().await;
}

/// A move already applied from the release folder
async fn applied(s: &Setup) {
    let repo = s.db.organise_moves();
    repo.plan(&NewMove {
        file_id: FILE,
        kind: "tv".to_string(),
        from_folder: "Show.Name.S02E05.1080p".to_string(),
        from_name: RELEASE.to_string(),
        to_folder: "TV Shows/Show Name/Season 02".to_string(),
        to_name: TARGET.to_string(),
        by_plugin: false,
    })
    .await
    .unwrap();
    repo.applied(FILE, RELEASE, RELEASE_FOLDER).await.unwrap();
}

#[tokio::test]
async fn undo_moves_and_renames_back() {
    let mut server = mockito::Server::new_async().await;
    details(&mut server, RELEASE_FOLDER, "Show-Name-S02E05", 0).await;
    details(&mut server, FILE, TARGET, 102).await;
    list(&mut server, RELEASE_FOLDER, &[]).await;
    let m = move_to(&mut server, RELEASE_FOLDER, 1).await;
    let r = rename(&mut server, RELEASE, 1).await;
    let s = setup(&server, SyncMode::Stream, tv(OrganiseApply::Preview)).await;
    applied(&s).await;

    let out = s.organiser.undo(&[FILE, 999]).await;
    assert_eq!(out[0].status, "undone", "{:?}", out[0].error);
    assert_eq!(out[1].status, "missing");
    m.assert_async().await;
    r.assert_async().await;
    let undone = s.db.organise_moves().get(FILE).await.unwrap().unwrap();
    assert_eq!(undone.status, MoveStatus::Undone);
    // Undoing twice does nothing; an undone move can be dismissed
    assert_eq!(s.organiser.undo(&[FILE]).await[0].status, "skipped");
    assert_eq!(s.organiser.dismiss(&[FILE]).await[0].status, "dismissed");
}

#[tokio::test]
async fn undo_with_the_old_folder_gone_changes_nothing() {
    let mut server = mockito::Server::new_async().await;
    gone(&mut server, RELEASE_FOLDER).await;
    let (r, m) = no_writes(&mut server).await;
    let s = setup(&server, SyncMode::Stream, tv(OrganiseApply::Preview)).await;
    applied(&s).await;

    let out = s.organiser.undo(&[FILE]).await;
    assert_eq!(out[0].status, "failed");
    assert!(out[0].error.as_deref().unwrap().contains("is gone"));
    r.assert_async().await;
    m.assert_async().await;
    let kept = s.db.organise_moves().get(FILE).await.unwrap().unwrap();
    assert_eq!(kept.status, MoveStatus::Applied);
    assert!(kept.error.is_some());
}

#[tokio::test]
async fn auto_moves_in_the_pass() {
    let mut server = mockito::Server::new_async().await;
    details(&mut server, FILE, RELEASE, RELEASE_FOLDER).await;
    new_folders(&mut server).await;
    list(&mut server, 102, &[]).await;
    let r = rename(&mut server, TARGET, 1).await;
    let m = move_to(&mut server, 102, 1).await;
    let s = setup(&server, SyncMode::Stream, tv(OrganiseApply::Auto)).await;

    let step = OrganiseStep::new(s.organiser.clone());
    assert_eq!(step.run(&release()).await, StepResult::Done);
    r.assert_async().await;
    m.assert_async().await;
    let moved = s.db.organise_moves().get(FILE).await.unwrap().unwrap();
    assert_eq!(moved.status, MoveStatus::Applied);
}

#[tokio::test]
async fn auto_retries_the_step_when_put_io_fails() {
    let mut server = mockito::Server::new_async().await;
    details(&mut server, FILE, RELEASE, RELEASE_FOLDER).await;
    new_folders(&mut server).await;
    list(&mut server, 102, &[]).await;
    server
        .mock("POST", "/files/rename")
        .with_status(500)
        .with_body("oops")
        .create_async()
        .await;
    let s = setup(&server, SyncMode::Stream, tv(OrganiseApply::Auto)).await;

    let step = OrganiseStep::new(s.organiser.clone());
    assert!(matches!(step.run(&release()).await, StepResult::Retry(_)));
    // Planned afresh on the retry
    assert!(s.db.organise_moves().get(FILE).await.unwrap().is_none());
}

#[tokio::test]
async fn scan_plans_streamed_files_once() {
    let server = mockito::Server::new_async().await;
    let s = setup(&server, SyncMode::Stream, tv(OrganiseApply::Preview)).await;
    let streamed = s.db.stream_files();
    streamed
        .add(FILE, RELEASE, "Show.Name.S02E05.1080p", 100, &[])
        .await
        .unwrap();
    streamed
        .add(6, "Movie.2019.1080p.mkv", "", 100, &[])
        .await
        .unwrap();

    assert_eq!(s.organiser.scan().await.unwrap(), 1);
    assert_eq!(s.organiser.scan().await.unwrap(), 0);
    assert_eq!(s.organiser.list().await.unwrap().len(), 1);
}

/// Plugins that place every file in `Shows/Custom` as `custom.mkv`
struct Fake;

impl SyncHooks for Fake {
    fn provides(&self, hook: Hook) -> bool {
        matches!(hook, Hook::Folder | Hook::Rename)
    }
    fn filter<'a>(&'a self, _: &'a HookFile) -> HookFuture<'a, Option<(FilterAnswer, String)>> {
        Box::pin(async { None })
    }
    fn rename<'a>(&'a self, _: &'a HookFile) -> HookFuture<'a, Option<String>> {
        Box::pin(async { Some("custom.mkv".to_string()) })
    }
    fn folder<'a>(&'a self, _: &'a HookFile) -> HookFuture<'a, Option<String>> {
        Box::pin(async { Some("/Shows/Custom/".to_string()) })
    }
}

#[tokio::test]
async fn plugin_answers_win_unless_turned_off() {
    let server = mockito::Server::new_async().await;
    let s = setup(&server, SyncMode::Stream, tv(OrganiseApply::Preview)).await;
    s.manager.set_sync_hooks(Arc::new(Fake));

    // Even a file the built-in reader skips
    let readme = ArrivedFile {
        file_id: 6,
        name: "README.txt".to_string(),
        ..release()
    };
    let placed = s.organiser.plan(&readme).await.unwrap().unwrap();
    assert_eq!(placed.to_folder, "Shows/Custom");
    assert_eq!(placed.to_name, "custom.mkv");
    assert!(placed.by_plugin);

    s.organiser.configure(OrganiseConfig {
        use_plugins: false,
        ..tv(OrganiseApply::Preview)
    });
    let built_in = s.organiser.plan(&release()).await.unwrap().unwrap();
    assert_eq!(built_in.to_folder, "TV Shows/Show Name/Season 02");
    assert!(!built_in.by_plugin);
}
