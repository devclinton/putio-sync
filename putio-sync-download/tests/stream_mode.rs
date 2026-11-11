//! Stream mode (feature 022): the sync pass keeps files on Put.io, records them once and
//! runs their arrived steps; the *arr and an explicit queue still download.

use putio_sync_client::{PutioApiClient, RetryPolicy};
use putio_sync_core::config::{DownloadConfig as DownloadSettings, SyncMode};
use putio_sync_core::rules::FolderRule;
use putio_sync_db::Database;
use putio_sync_download::{ArrivedStep, DownloadConfig, DownloadManager, Mp4Step, TreeDecision};
use std::sync::Arc;
use std::time::Duration;

const ROOT: &str = r#"{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false}"#;

fn entry(id: i64, name: &str, parent_id: i64) -> String {
    let folder = !name.contains('.');
    format!(
        r#"{{"id":{id},"name":"{name}","size":100,"file_type":"{}","parent_id":{parent_id},"is_folder":{folder},"is_shared":false}}"#,
        if folder { "FOLDER" } else { "VIDEO" }
    )
}

/// (folder id, name, parent, files)
type MockFolder = (i64, &'static str, i64, &'static [(i64, &'static str)]);

const TREE: &[MockFolder] = &[
    (
        10,
        "Movies",
        0,
        &[(11, "film.mkv"), (12, "film.mp4"), (13, "setup.exe")],
    ),
    (15, "Keep", 10, &[(16, "keep.mkv")]),
    (20, "Shows", 0, &[(21, "ep1.avi"), (22, "ep1.srt")]),
];

/// Every file id the sync accepts (not `setup.exe`)
const ACCEPTED: &[i64] = &[1, 11, 12, 16, 21, 22];

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
    let mut root = vec![entry(1, "loose.mkv", 0)];
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

fn config(dir: &std::path::Path, settings: DownloadSettings) -> DownloadConfig {
    let mut config = DownloadConfig::from(&DownloadSettings {
        target_directory: dir.to_path_buf(),
        ..settings
    });
    config.max_retries = 0;
    config.retry_initial_delay_secs = 0;
    config
}

fn stream() -> DownloadSettings {
    DownloadSettings {
        mode: SyncMode::Stream,
        ..DownloadSettings::default()
    }
}

struct Setup {
    server: mockito::ServerGuard,
    dir: tempfile::TempDir,
    db: Arc<Database>,
    manager: DownloadManager,
}

async fn setup(settings: DownloadSettings) -> Setup {
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
    let manager = DownloadManager::new(client.clone(), db.clone(), config(dir.path(), settings));
    let steps: Vec<Arc<dyn ArrivedStep>> = vec![Arc::new(Mp4Step::new(client))];
    manager.set_arrived_steps(steps);
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

/// MP4 mocks for every video, so a pass with the step on never reaches an unmocked path
async fn mp4_ready(server: &mut mockito::ServerGuard, ids: &[i64]) -> Vec<mockito::Mock> {
    let mut mocks = Vec::new();
    for id in ids {
        mocks.push(
            server
                .mock("GET", format!("/files/{id}/mp4").as_str())
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(r#"{"status":"OK","mp4":{"status":"COMPLETED","size":90}}"#)
                .create_async()
                .await,
        );
    }
    mocks
}

#[tokio::test]
async fn stream_pass_keeps_files_on_putio_and_records_them_once() {
    let mut s = setup(DownloadSettings {
        stream_mp4: false,
        ..stream()
    })
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
    for _ in 0..2 {
        let stats = s.manager.sync_downloads().await.unwrap();
        assert_eq!(stats.files_downloaded, 0);
        assert_eq!(stats.files_streamed, ACCEPTED.len());
        assert_eq!(stats.files_blocked, 1, "setup.exe is still blocked");
    }
    for id in ACCEPTED {
        assert!(!queued(&s.db, *id).await, "{id} queued");
        let row = s.db.stream_files().get(*id).await.unwrap().unwrap();
        assert!(row.pending.is_empty(), "MP4 step is off");
    }
    assert_eq!(
        s.db.stream_files().ids().await.unwrap().len(),
        ACCEPTED.len()
    );
    assert!(!queued(&s.db, 13).await);
    // Nothing written to the download folder
    let written: Vec<_> = std::fs::read_dir(s.dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| !e.file_name().to_string_lossy().starts_with(".putiosync"))
        .collect();
    assert!(written.is_empty(), "{written:?}");
    delete.assert_async().await;
}

#[tokio::test]
async fn blocked_files_in_a_stream_folder_are_still_deleted() {
    let mut s = setup(DownloadSettings {
        stream_mp4: false,
        rules: vec![FolderRule {
            path: "Movies".into(),
            delete_blocked: Some(true),
            ..FolderRule::default()
        }],
        ..stream()
    })
    .await;
    let delete = s
        .server
        .mock("POST", "/files/delete")
        .match_body(mockito::Matcher::PartialJsonString(
            r#"{"file_ids":"13"}"#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(1)
        .create_async()
        .await;
    let stats = s.manager.sync_downloads().await.unwrap();
    assert_eq!(stats.files_deleted, 1);
    delete.assert_async().await;
}

#[tokio::test]
async fn switching_back_to_download_queues_and_forgets_the_streamed_files() {
    let s = setup(DownloadSettings {
        stream_mp4: false,
        ..stream()
    })
    .await;
    s.manager.sync_downloads().await.unwrap();
    assert_eq!(
        s.db.stream_files().ids().await.unwrap().len(),
        ACCEPTED.len()
    );

    s.manager
        .update_config(config(s.dir.path(), DownloadSettings::default()))
        .await;
    let stats = s.manager.sync_downloads().await.unwrap();
    assert_eq!(stats.files_streamed, 0);
    assert_eq!(stats.files_downloaded, ACCEPTED.len());
    for id in ACCEPTED {
        assert!(queued(&s.db, *id).await, "{id} not queued");
    }
    assert!(s.db.stream_files().ids().await.unwrap().is_empty());
}

#[tokio::test]
async fn the_arr_and_an_explicit_queue_download_anyway() {
    let s = setup(DownloadSettings {
        stream_mp4: false,
        ..stream()
    })
    .await;
    // The *arr download client
    let report = s.manager.enqueue_tree_report(10, true).await.unwrap();
    let movies: Vec<_> = report
        .files
        .iter()
        .filter(|f| f.decision == TreeDecision::Queued)
        .map(|f| f.file_id)
        .collect();
    assert_eq!(movies, [11, 12, 16]);
    // "Download" on the Put.io page
    let stats = s.manager.enqueue_tree(20).await.unwrap();
    assert_eq!(stats.files_downloaded, 2);
    assert_eq!(stats.files_streamed, 0);
    for id in [11, 12, 16, 21, 22] {
        assert!(queued(&s.db, id).await, "{id} not queued");
    }
    // Neither records streamed files
    assert!(s.db.stream_files().ids().await.unwrap().is_empty());
    // The next sync pass leaves them to their downloads and streams only the rest
    let stats = s.manager.sync_downloads().await.unwrap();
    assert_eq!(stats.files_streamed, 1);
    let ids: Vec<_> =
        s.db.stream_files()
            .ids()
            .await
            .unwrap()
            .into_iter()
            .collect();
    assert_eq!(ids, [1]);
}

#[tokio::test]
async fn a_folder_streams_while_a_deeper_rule_and_the_rest_download() {
    let s = setup(DownloadSettings {
        stream_mp4: false,
        rules: vec![
            FolderRule {
                path: "Movies".into(),
                mode: Some(SyncMode::Stream),
                ..FolderRule::default()
            },
            FolderRule {
                path: "Movies/Keep".into(),
                mode: Some(SyncMode::Download),
                ..FolderRule::default()
            },
        ],
        ..DownloadSettings::default()
    })
    .await;
    let stats = s.manager.sync_downloads().await.unwrap();
    assert_eq!(stats.files_streamed, 2);
    for id in [11, 12] {
        assert!(!queued(&s.db, id).await, "{id} queued");
    }
    for id in [1, 16, 21, 22] {
        assert!(queued(&s.db, id).await, "{id} not queued");
    }
    let mut ids: Vec<_> =
        s.db.stream_files()
            .ids()
            .await
            .unwrap()
            .into_iter()
            .collect();
    ids.sort();
    assert_eq!(ids, [11, 12]);
}

#[tokio::test]
async fn non_mp4_videos_get_one_mp4_request() {
    let mut s = setup(stream()).await;
    let _ready = mp4_ready(&mut s.server, &[16]).await;
    let mut wants = Vec::new();
    for id in [1, 11] {
        s.server
            .mock("GET", format!("/files/{id}/mp4").as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"OK","mp4":{"status":"NOT_AVAILABLE"}}"#)
            .create_async()
            .await;
        wants.push(
            s.server
                .mock("POST", format!("/files/{id}/mp4").as_str())
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(r#"{"status":"OK"}"#)
                .expect(1)
                .create_async()
                .await,
        );
    }
    // Already converting: nothing asked
    s.server
        .mock("GET", "/files/21/mp4")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK","mp4":{"status":"CONVERTING"}}"#)
        .create_async()
        .await;
    let converting = s
        .server
        .mock("POST", "/files/21/mp4")
        .expect(0)
        .create_async()
        .await;
    // .mp4 and .srt are never looked at
    let other = s
        .server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/files/(12|22)/mp4$".into()),
        )
        .expect(0)
        .create_async()
        .await;

    for _ in 0..2 {
        s.manager.sync_downloads().await.unwrap();
    }
    for mock in wants {
        mock.assert_async().await;
    }
    converting.assert_async().await;
    other.assert_async().await;
    assert!(s.db.stream_files().pending(100).await.unwrap().is_empty());
}

#[tokio::test]
async fn mp4_requests_off_ask_nothing() {
    let mut s = setup(DownloadSettings {
        stream_mp4: false,
        ..stream()
    })
    .await;
    let none = s
        .server
        .mock("GET", mockito::Matcher::Regex(r"^/files/\d+/mp4$".into()))
        .expect(0)
        .create_async()
        .await;
    s.manager.sync_downloads().await.unwrap();
    none.assert_async().await;
}

#[tokio::test]
async fn a_failed_mp4_request_is_retried_next_pass() {
    let mut s = setup(stream()).await;
    let _ready = mp4_ready(&mut s.server, &[11, 16, 21]).await;
    let failing = s
        .server
        .mock("GET", "/files/1/mp4")
        .with_status(500)
        .with_body("oops")
        .expect(1)
        .create_async()
        .await;
    s.manager.sync_downloads().await.unwrap();
    failing.assert_async().await;
    let row = s.db.stream_files().get(1).await.unwrap().unwrap();
    assert_eq!(row.pending, ["mp4"]);
    assert_eq!(row.attempts, 1);
    assert!(row.last_error.is_some());

    failing.remove_async().await;
    let _ok = mp4_ready(&mut s.server, &[1]).await;
    s.manager.sync_downloads().await.unwrap();
    let row = s.db.stream_files().get(1).await.unwrap().unwrap();
    assert!(row.pending.is_empty());
    assert_eq!(row.last_error, None);
}
