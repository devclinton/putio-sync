//! The download client's items against mocked Put.io and *arr APIs (feature 011)

use std::sync::Arc;

use mockito::{Matcher, Mock, ServerGuard};
use putio_sync_arr::{AddOutcome, AddSource, ArrError, ArrService, ItemId};
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::{ArrKind, ArrServer, ArrSettings, PrecheckConfig, PrecheckOverride};
use putio_sync_db::models::DownloadStatus;
use putio_sync_db::{ArrState, BlocklistState, Database};
use putio_sync_download::{DownloadConfig, DownloadManager};

const HASH: &str = "c12fe1c06bba254a9dc9f519b335aa7c1367a88a";
const CATEGORY_FOLDER: i64 = 77;
const TRANSFER: i64 = 900;
const ITEM_FOLDER: i64 = 50;

fn magnet() -> String {
    format!("magnet:?xt=urn:btih:{}&dn=Show.S01E01", HASH.to_uppercase())
}

fn entry(id: i64, name: &str, file_type: &str, parent_id: i64, size: i64) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","size":{size},"file_type":"{file_type}","parent_id":{parent_id},"is_folder":{},"is_shared":false}}"#,
        file_type == "FOLDER"
    )
}

fn transfer(status: &str, percent: i32, file_id: Option<i64>, error: Option<&str>) -> String {
    format!(
        r#"{{"id":{TRANSFER},"name":"Show.S01E01","size":1000,"status":"{status}","percent_done":{percent},"estimated_time":60,"created_at":"2026-10-26T08:00:00","save_parent_id":{CATEGORY_FOLDER},"is_private":false,"file_id":{},"error_message":{}}}"#,
        file_id.map_or("null".to_string(), |id| id.to_string()),
        error.map_or("null".to_string(), |e| format!("\"{e}\"")),
    )
}

struct Harness {
    putio: ServerGuard,
    dir: tempfile::TempDir,
    db: Arc<Database>,
    client: Arc<PutioApiClient>,
    downloads: Arc<DownloadManager>,
}

impl Harness {
    async fn new() -> Self {
        let putio = mockito::Server::new_async().await;
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            Database::new(dir.path().join(".putiosync.db"))
                .await
                .unwrap(),
        );
        let client = Arc::new(PutioApiClient::with_base_url(
            "test-putio-token".to_string(),
            putio.url(),
        ));
        let config = DownloadConfig {
            target_directory: dir.path().join("downloads"),
            rules: putio_sync_core::rules::RuleSet::from_config(
                &putio_sync_core::config::DownloadConfig {
                    blocked_extensions: vec!["exe".to_string()],
                    ..Default::default()
                },
            ),
            ..DownloadConfig::default()
        };
        let downloads = Arc::new(DownloadManager::new(client.clone(), db.clone(), config));
        Self {
            putio,
            dir,
            db,
            client,
            downloads,
        }
    }

    fn target(&self) -> std::path::PathBuf {
        self.dir.path().join("downloads")
    }

    fn service(&self, servers: Vec<ArrServer>) -> ArrService {
        let settings = ArrSettings {
            enabled: true,
            username: "putio-sync".to_string(),
            password: Some("pw".to_string()),
            servers,
        };
        ArrService::new(
            self.client.clone(),
            self.db.clone(),
            self.downloads.clone(),
            settings,
            self.target(),
        )
    }

    async fn json(&mut self, method: &str, path: &str, body: String) -> Mock {
        self.putio
            .mock(method, path)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create_async()
            .await
    }

    /// Root listing without the category folder, and its creation
    async fn category_created(&mut self) -> (Mock, Mock) {
        let list = self
            .json(
                "GET",
                "/files/list?parent_id=0",
                format!(
                    r#"{{"files":[{}],"parent":{},"status":"OK"}}"#,
                    entry(5, "other", "FOLDER", 0, 0),
                    entry(0, "root", "FOLDER", 0, 0)
                ),
            )
            .await;
        let create = self
            .putio
            .mock("POST", "/files/create-folder")
            .match_body(Matcher::PartialJsonString(
                r#"{"name":"sonarr","parent_id":0}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{"file":{},"status":"OK"}}"#,
                entry(CATEGORY_FOLDER, "sonarr", "FOLDER", 0, 0)
            ))
            .expect(1)
            .create_async()
            .await;
        (list, create)
    }

    async fn transfer_added(&mut self) -> Mock {
        self.putio
            .mock("POST", "/transfers/add")
            .match_body(Matcher::PartialJsonString(format!(
                r#"{{"save_parent_id":{CATEGORY_FOLDER}}}"#
            )))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{"transfer":{},"status":"OK"}}"#,
                transfer("IN_QUEUE", 0, None, None)
            ))
            .expect(1)
            .create_async()
            .await
    }

    async fn transfers(&mut self, transfers: &[String]) -> Mock {
        self.json(
            "GET",
            "/transfers/list",
            format!(r#"{{"transfers":[{}],"status":"OK"}}"#, transfers.join(",")),
        )
        .await
    }

    /// The completed transfer's folder under `sonarr`, with `files`
    async fn item_folder(&mut self, files: &[String]) {
        let folder = entry(ITEM_FOLDER, "Show.S01E01", "FOLDER", CATEGORY_FOLDER, 0);
        self.json(
            "GET",
            &format!("/files/{ITEM_FOLDER}"),
            format!(r#"{{"file":{folder},"status":"OK"}}"#),
        )
        .await;
        self.json(
            "GET",
            &format!("/files/{CATEGORY_FOLDER}"),
            format!(
                r#"{{"file":{},"status":"OK"}}"#,
                entry(CATEGORY_FOLDER, "sonarr", "FOLDER", 0, 0)
            ),
        )
        .await;
        self.json(
            "GET",
            &format!("/files/list?parent_id={ITEM_FOLDER}"),
            format!(
                r#"{{"files":[{}],"parent":{folder},"status":"OK"}}"#,
                files.join(",")
            ),
        )
        .await;
    }

    async fn added(&mut self, service: &ArrService) {
        self.category_created().await;
        self.transfer_added().await;
        let dir = format!("{}/sonarr", self.target().display());
        service
            .add(AddSource::Magnet(magnet()), Some(&dir))
            .await
            .unwrap();
    }
}

async fn state(db: &Database) -> (ArrState, Option<String>, BlocklistState) {
    let item = db.arr().get_by_hash(HASH).await.unwrap().unwrap();
    (item.state, item.error, item.blocklist)
}

#[tokio::test]
async fn magnet_add_creates_the_category_folder_and_transfer() {
    let mut h = Harness::new().await;
    let (_list, create) = h.category_created().await;
    let add = h.transfer_added().await;
    let service = h.service(vec![]);
    let dir = format!("{}/sonarr/", h.target().display());

    let outcome = service
        .add(AddSource::Magnet(magnet()), Some(&dir))
        .await
        .unwrap();
    let AddOutcome::Added(item) = outcome else {
        panic!("expected a new item");
    };
    assert_eq!(item.hash, HASH);
    assert_eq!(item.name, "Show.S01E01");
    assert_eq!(item.category, "sonarr");
    assert_eq!(item.transfer_id, Some(TRANSFER));
    assert_eq!(item.state, ArrState::Putio);

    // Same hash again: no Put.io call (each mock expects exactly one)
    let again = service
        .add(AddSource::Magnet(magnet()), Some(&dir))
        .await
        .unwrap();
    assert!(matches!(again, AddOutcome::Duplicate(i) if i.id == item.id));
    create.assert_async().await;
    add.assert_async().await;
}

#[tokio::test]
async fn torrent_file_add_uploads_into_the_category() {
    let mut h = Harness::new().await;
    // The category folder already exists
    h.json(
        "GET",
        "/files/list?parent_id=0",
        format!(
            r#"{{"files":[{}],"parent":{},"status":"OK"}}"#,
            entry(CATEGORY_FOLDER, "sonarr", "FOLDER", 0, 0),
            entry(0, "root", "FOLDER", 0, 0)
        ),
    )
    .await;
    let upload = h
        .putio
        .mock("POST", "/files/upload")
        .match_body(Matcher::Regex(format!(
            "name=\"parent_id\"\r\n\r\n{CATEGORY_FOLDER}"
        )))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"transfer":{},"status":"OK"}}"#,
            transfer("IN_QUEUE", 0, None, None)
        ))
        .expect(1)
        .create_async()
        .await;
    let service = h.service(vec![]);

    let torrent =
        b"d4:infod6:lengthi1e4:name9:Movie.mkv12:piece lengthi1e6:pieces20:aaaaaaaaaaaaaaaaaaaaee"
            .to_vec();
    let dir = format!("{}/sonarr", h.target().display());
    let AddOutcome::Added(item) = service
        .add(AddSource::Metainfo(torrent), Some(&dir))
        .await
        .unwrap()
    else {
        panic!("expected a new item");
    };
    assert_eq!(item.name, "Movie.mkv");
    assert_eq!(item.hash.len(), 40);
    upload.assert_async().await;
}

#[tokio::test]
async fn bad_input_is_refused_before_putio() {
    let h = Harness::new().await;
    let service = h.service(vec![]);

    let err = service
        .add(AddSource::Magnet(magnet()), Some("/elsewhere/sonarr"))
        .await
        .unwrap_err();
    assert!(matches!(err, ArrError::OutsideTarget(_)));
    assert!(err.to_string().starts_with("download-dir must be inside "));

    let escape = format!("{}/../x", h.target().display());
    assert!(matches!(
        service
            .add(AddSource::Magnet(magnet()), Some(&escape))
            .await,
        Err(ArrError::OutsideTarget(_))
    ));
    assert!(matches!(
        service
            .add(AddSource::Magnet("magnet:?dn=x".into()), None)
            .await,
        Err(ArrError::InvalidMagnet)
    ));
    assert!(matches!(
        service
            .add(AddSource::Metainfo(b"<html>".to_vec()), None)
            .await,
        Err(ArrError::InvalidTorrent)
    ));
    assert!(matches!(
        service
            .add(AddSource::Url("ftp://x/a.torrent".into()), None)
            .await,
        Err(ArrError::InvalidTorrent)
    ));
}

#[tokio::test]
async fn putio_refusal_is_reported() {
    let mut h = Harness::new().await;
    h.category_created().await;
    h.putio
        .mock("POST", "/transfers/add")
        .with_status(400)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error_message":"Invalid magnet","status":"ERROR"}"#)
        .create_async()
        .await;
    let service = h.service(vec![]);
    let dir = format!("{}/sonarr", h.target().display());

    let err = service
        .add(AddSource::Magnet(magnet()), Some(&dir))
        .await
        .unwrap_err();
    assert!(
        err.to_string().starts_with("Put.io refused the transfer: "),
        "{err}"
    );
    assert!(h.db.arr().get_by_hash(HASH).await.unwrap().is_none());
}

#[tokio::test]
async fn lifecycle_putio_local_done_and_restart() {
    let mut h = Harness::new().await;
    let service = h.service(vec![]);
    h.added(&service).await;

    // Downloading on Put.io: the first half of the progress
    let list = h
        .transfers(&[transfer("DOWNLOADING", 40, None, None)])
        .await;
    service.refresh().await;
    let torrents = service.torrents(None).await.unwrap();
    assert_eq!(torrents.len(), 1);
    let t = &torrents[0];
    assert_eq!(t["hashString"], HASH);
    assert_eq!(t["status"], 4);
    assert_eq!(t["leftUntilDone"], 800);
    assert_eq!(t["isFinished"], false);
    assert_eq!(t["downloadDir"], format!("{}/sonarr", h.target().display()));
    list.remove_async().await;

    // Complete on Put.io: its files are queued and the item waits for the accepted ones
    h.transfers(&[transfer("COMPLETED", 100, Some(ITEM_FOLDER), None)])
        .await;
    h.item_folder(&[
        entry(1, "Show.S01E01.mkv", "VIDEO", ITEM_FOLDER, 900),
        entry(2, "sample.exe", "FILE", ITEM_FOLDER, 100),
    ])
    .await;
    service.refresh().await;
    let item = h.db.arr().get_by_hash(HASH).await.unwrap().unwrap();
    assert_eq!(item.state, ArrState::Local);
    assert_eq!(item.output_name.as_deref(), Some("Show.S01E01"));
    assert_eq!(item.total_size, 900);
    assert_eq!(h.db.arr().files(HASH).await.unwrap(), vec![(1, 900)]);
    assert!(h.db.queue().get_by_file_id(1).await.unwrap().is_some());
    assert!(h.db.queue().get_by_file_id(2).await.unwrap().is_none());

    let t = service
        .torrents(Some(&[ItemId::Id(item.id)]))
        .await
        .unwrap();
    assert_eq!(t[0]["leftUntilDone"], 450);
    assert_eq!(t[0]["name"], "Show.S01E01");

    // The file lands on disk: finished
    h.db.downloads()
        .insert(
            1,
            "Show.S01E01.mkv",
            900,
            "sonarr/Show.S01E01",
            DownloadStatus::Completed,
        )
        .await
        .unwrap();
    service.refresh().await;
    let t = service
        .torrents(Some(&[ItemId::Hash(HASH.to_uppercase())]))
        .await
        .unwrap();
    assert_eq!(t[0]["isFinished"], true);
    assert_eq!(t[0]["percentDone"], 1.0);
    assert_eq!(t[0]["status"], 0);

    // A new service over the same database still knows the item
    let restarted = h.service(vec![]);
    let t = restarted.torrents(None).await.unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0]["isFinished"], true);
    assert_eq!(t[0]["id"], item.id);
}

#[tokio::test]
async fn putio_error_and_missing_transfer_fail_the_item() {
    let mut h = Harness::new().await;
    let service = h.service(vec![]);
    h.added(&service).await;

    let list = h
        .transfers(&[transfer("ERROR", 10, None, Some("No peers"))])
        .await;
    service.refresh().await;
    assert_eq!(
        state(&h.db).await,
        (
            ArrState::Failed,
            Some("No peers".into()),
            BlocklistState::None
        )
    );
    let t = service.torrents(None).await.unwrap();
    assert_eq!(t[0]["error"], 3);
    assert_eq!(t[0]["errorString"], "No peers");
    list.remove_async().await;

    // Retried on Put.io: back to downloading
    let list = h
        .transfers(&[transfer("DOWNLOADING", 20, None, None)])
        .await;
    service.refresh().await;
    assert_eq!(state(&h.db).await.0, ArrState::Putio);
    list.remove_async().await;

    // Gone from Put.io
    h.transfers(&[]).await;
    service.refresh().await;
    assert_eq!(
        state(&h.db).await,
        (
            ArrState::Failed,
            Some("transfer no longer on Put.io".into()),
            BlocklistState::None
        )
    );
}

#[tokio::test]
async fn unreachable_putio_keeps_states() {
    let mut h = Harness::new().await;
    let service = h.service(vec![]);
    h.added(&service).await;
    h.putio
        .mock("GET", "/transfers/list")
        .with_status(401)
        .with_body(r#"{"error_message":"bad token","status":"ERROR"}"#)
        .create_async()
        .await;
    service.refresh().await;
    assert_eq!(state(&h.db).await.0, ArrState::Putio);
}

#[tokio::test]
async fn blocked_only_release_fails_and_is_blocklisted() {
    let mut h = Harness::new().await;
    let mut arr = mockito::Server::new_async().await;
    let queue = arr
        .mock("GET", "/api/v3/queue/details")
        .match_header("x-api-key", "sonarr-key")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"[{{"id":3,"downloadId":"OTHER"}},{{"id":9,"downloadId":"{}"}}]"#,
            HASH.to_uppercase()
        ))
        .expect(1)
        .create_async()
        .await;
    let delete = arr
        .mock(
            "DELETE",
            "/api/v3/queue/9?removeFromClient=true&blocklist=true",
        )
        .match_header("x-api-key", "sonarr-key")
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    let service = h.service(vec![ArrServer {
        kind: ArrKind::Sonarr,
        url: arr.url(),
        api_key: Some("sonarr-key".to_string()),
        category: "sonarr".to_string(),
    }]);
    h.added(&service).await;

    h.transfers(&[transfer("COMPLETED", 100, Some(ITEM_FOLDER), None)])
        .await;
    h.item_folder(&[
        entry(1, "Show.S01E01.exe", "FILE", ITEM_FOLDER, 900),
        entry(2, "readme.exe", "FILE", ITEM_FOLDER, 10),
    ])
    .await;
    service.refresh().await;

    assert_eq!(
        state(&h.db).await,
        (
            ArrState::Failed,
            Some("no files to import: blocked extensions .exe".into()),
            BlocklistState::Done
        )
    );
    queue.assert_async().await;
    delete.assert_async().await;

    // Done: the next refresh makes no *arr call (the mocks expect one each)
    service.refresh().await;
    queue.assert_async().await;
}

#[tokio::test]
async fn blocklist_gives_up_after_five_failures() {
    let mut h = Harness::new().await;
    let mut arr = mockito::Server::new_async().await;
    let queue = arr
        .mock("GET", "/api/v3/queue/details")
        .with_status(401)
        .expect(5)
        .create_async()
        .await;
    let service = h.service(vec![ArrServer {
        kind: ArrKind::Sonarr,
        url: arr.url(),
        api_key: Some("wrong".to_string()),
        category: "sonarr".to_string(),
    }]);
    h.added(&service).await;
    h.transfers(&[]).await;

    for _ in 0..6 {
        service.refresh().await;
    }
    let item = h.db.arr().get_by_hash(HASH).await.unwrap().unwrap();
    assert_eq!(item.blocklist, BlocklistState::GaveUp);
    assert_eq!(item.blocklist_attempts, 5);
    queue.assert_async().await;
}

#[tokio::test]
async fn remove_finished_item_with_data() {
    let mut h = Harness::new().await;
    let service = h.service(vec![]);
    h.added(&service).await;
    h.transfers(&[transfer("COMPLETED", 100, Some(ITEM_FOLDER), None)])
        .await;
    h.item_folder(&[entry(1, "Show.S01E01.mkv", "VIDEO", ITEM_FOLDER, 900)])
        .await;
    service.refresh().await;
    h.db.downloads()
        .insert(1, "Show.S01E01.mkv", 900, "x", DownloadStatus::Completed)
        .await
        .unwrap();
    service.refresh().await;
    assert_eq!(state(&h.db).await.0, ArrState::Done);

    let output = h.target().join("sonarr").join("Show.S01E01");
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(output.join("Show.S01E01.mkv"), b"x").unwrap();
    let keep = h.target().join("sonarr").join("Other");
    std::fs::create_dir_all(&keep).unwrap();

    let cancel = h
        .json("POST", "/transfers/cancel", r#"{"status":"OK"}"#.into())
        .await;
    let delete = h
        .putio
        .mock("POST", "/files/delete")
        .match_body(Matcher::Regex(ITEM_FOLDER.to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(1)
        .create_async()
        .await;

    service.remove(&HASH.to_uppercase(), true).await.unwrap();
    assert!(h.db.arr().get_by_hash(HASH).await.unwrap().is_none());
    assert!(h.db.arr().files(HASH).await.unwrap().is_empty());
    assert!(!output.exists());
    assert!(keep.exists());
    cancel.assert_async().await;
    delete.assert_async().await;

    // Unknown hashes are a no-op
    service.remove(HASH, true).await.unwrap();
}

#[tokio::test]
async fn remove_unfinished_item_keeps_putio_files_and_drops_queue() {
    let mut h = Harness::new().await;
    let service = h.service(vec![]);
    h.added(&service).await;
    h.transfers(&[transfer("COMPLETED", 100, Some(ITEM_FOLDER), None)])
        .await;
    h.item_folder(&[entry(1, "Show.S01E01.mkv", "VIDEO", ITEM_FOLDER, 900)])
        .await;
    service.refresh().await;
    assert_eq!(state(&h.db).await.0, ArrState::Local);
    assert!(h.db.queue().get_by_file_id(1).await.unwrap().is_some());

    // Put.io failing must not keep the item
    h.putio
        .mock("POST", "/transfers/cancel")
        .with_status(401)
        .create_async()
        .await;
    let delete = h
        .putio
        .mock("POST", "/files/delete")
        .expect(0)
        .create_async()
        .await;

    service.remove(HASH, false).await.unwrap();
    assert!(h.db.arr().get_by_hash(HASH).await.unwrap().is_none());
    assert!(h.db.queue().get_by_file_id(1).await.unwrap().is_none());
    delete.assert_async().await;
}

// --- Release pre-check (feature 042) ---

const MB: u64 = 1024 * 1024;

/// A multi-file .torrent named `name` with `files` as (path, size)
fn torrent(name: &str, files: &[(&str, u64)]) -> Vec<u8> {
    let mut list = String::new();
    for (path, size) in files {
        list.push_str(&format!("d6:lengthi{size}e4:pathl{}:{path}ee", path.len()));
    }
    format!(
        "d4:infod5:filesl{list}e4:name{}:{name}12:piece lengthi16384e6:pieces20:aaaaaaaaaaaaaaaaaaaaee",
        name.len()
    )
    .into_bytes()
}

fn hash_of(bytes: &[u8]) -> String {
    putio_sync_precheck::metainfo::info_hash(bytes).unwrap().0
}

/// (source, stage, state, reason) of each `release_checks` row, oldest first
async fn checks(db: &Database) -> Vec<(String, String, String, Option<String>)> {
    sqlx::query_as("SELECT source, stage, state, reason FROM release_checks ORDER BY id")
        .fetch_all(db.pool())
        .await
        .unwrap()
}

impl Harness {
    fn checked(&self, servers: Vec<ArrServer>, cfg: PrecheckConfig) -> ArrService {
        self.service(servers).with_precheck(
            cfg,
            putio_sync_core::rules::RuleSet::from_config(
                &putio_sync_core::config::DownloadConfig::default(),
            ),
        )
    }

    /// The `sonarr` folder exists, and one upload into it is expected
    async fn upload_expected(&mut self) -> Mock {
        self.json(
            "GET",
            "/files/list?parent_id=0",
            format!(
                r#"{{"files":[{}],"parent":{},"status":"OK"}}"#,
                entry(CATEGORY_FOLDER, "sonarr", "FOLDER", 0, 0),
                entry(0, "root", "FOLDER", 0, 0)
            ),
        )
        .await;
        self.putio
            .mock("POST", "/files/upload")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{"transfer":{},"status":"OK"}}"#,
                transfer("IN_QUEUE", 0, None, None)
            ))
            .expect(1)
            .create_async()
            .await
    }
}

fn sonarr(url: String) -> ArrServer {
    ArrServer {
        kind: ArrKind::Sonarr,
        url,
        api_key: Some("sonarr-key".to_string()),
        category: "sonarr".to_string(),
    }
}

#[tokio::test]
async fn fake_torrents_never_reach_putio() {
    let mut h = Harness::new().await;
    let upload = h.upload_expected().await;
    let arr = mockito::Server::new_async().await;
    let service = h.checked(vec![sonarr(arr.url())], PrecheckConfig::default());
    let dir = format!("{}/sonarr", h.target().display());

    let exe = torrent(
        "Show.S01E03.1080p",
        &[("Show.S01E03.exe", 1500 * MB), ("readme.txt", 1000)],
    );
    let seven = torrent("Show.S01E04.1080p", &[("Show.S01E04.7z", 900 * MB)]);
    let real = torrent("Show.S01E05.1080p", &[("Show.S01E05.mkv", 2048 * MB)]);

    for (bytes, error) in [
        (
            &exe,
            "rejected before Put.io: no video file (found .exe, .txt)",
        ),
        (&seven, "rejected before Put.io: no video file (found .7z)"),
    ] {
        let AddOutcome::Added(item) = service
            .add(AddSource::Metainfo(bytes.clone()), Some(&dir))
            .await
            .unwrap()
        else {
            panic!("expected a new item");
        };
        assert_eq!(item.hash, hash_of(bytes));
        assert_eq!(item.transfer_id, None);
        assert_eq!(item.state, ArrState::Failed);
        assert_eq!(item.error.as_deref(), Some(error));
        assert_eq!(item.blocklist, BlocklistState::Pending);
    }
    let AddOutcome::Added(item) = service
        .add(AddSource::Metainfo(real.clone()), Some(&dir))
        .await
        .unwrap()
    else {
        panic!("expected a new item");
    };
    assert_eq!(item.transfer_id, Some(TRANSFER));
    upload.assert_async().await;

    let rows = checks(&h.db).await;
    assert_eq!(
        rows,
        vec![
            (
                "torrent".into(),
                "before".into(),
                "rejected".into(),
                Some("no_wanted".into())
            ),
            (
                "torrent".into(),
                "before".into(),
                "rejected".into(),
                Some("no_wanted".into())
            ),
            ("torrent".into(), "before".into(), "passed".into(), None),
        ]
    );
    let (types, hosts, count): (String, String, i64) = sqlx::query_as(
        "SELECT types, tracker_hosts, file_count FROM release_reports ORDER BY id LIMIT 1",
    )
    .fetch_one(h.db.pool())
    .await
    .unwrap();
    assert!(types.starts_with(r#"[{"ext":"exe","count":1,"#), "{types}");
    assert_eq!(hosts, "[]");
    assert_eq!(count, 2);

    // A rejected item has no transfer to follow, and removing it calls no Put.io API
    service.remove(&hash_of(&exe), true).await.unwrap();
    assert!(h
        .db
        .arr()
        .get_by_hash(&hash_of(&exe))
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn rejected_item_is_blocklisted_once() {
    let mut h = Harness::new().await;
    let exe = torrent("Show.S01E03.1080p", &[("Show.S01E03.exe", 1500 * MB)]);
    let hash = hash_of(&exe);
    let mut arr = mockito::Server::new_async().await;
    let queue = arr
        .mock("GET", "/api/v3/queue/details")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"[{{"id":9,"downloadId":"{}"}}]"#,
            hash.to_uppercase()
        ))
        .expect(1)
        .create_async()
        .await;
    let delete = arr
        .mock(
            "DELETE",
            "/api/v3/queue/9?removeFromClient=true&blocklist=true",
        )
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    // No Put.io call at all: neither a transfer list nor an upload
    let list = h.transfers(&[]).await.expect(0);
    let service = h.checked(vec![sonarr(arr.url())], PrecheckConfig::default());
    let dir = format!("{}/sonarr", h.target().display());
    service
        .add(AddSource::Metainfo(exe), Some(&dir))
        .await
        .unwrap();

    service.refresh().await;
    service.refresh().await;
    let item = h.db.arr().get_by_hash(&hash).await.unwrap().unwrap();
    assert_eq!(item.blocklist, BlocklistState::Done);
    queue.assert_async().await;
    delete.assert_async().await;
    list.assert_async().await;
}

#[tokio::test]
async fn url_is_fetched_and_checked() {
    let h = Harness::new().await;
    let mut indexer = mockito::Server::new_async().await;
    let exe = torrent("Movie.2026.1080p", &[("Movie.2026.exe", 1500 * MB)]);
    let _t = indexer
        .mock("GET", "/download?apikey=secret")
        .with_body(exe.clone())
        .create_async()
        .await;
    let service = h.checked(vec![], PrecheckConfig::default());
    let AddOutcome::Added(item) = service
        .add(
            AddSource::Url(format!("{}/download?apikey=secret", indexer.url())),
            None,
        )
        .await
        .unwrap()
    else {
        panic!("expected a new item");
    };
    assert_eq!(item.hash, hash_of(&exe));
    assert_eq!(item.state, ArrState::Failed);
    assert_eq!(
        checks(&h.db).await,
        vec![(
            "url".into(),
            "before".into(),
            "rejected".into(),
            Some("no_wanted".into())
        )]
    );
}

#[tokio::test]
async fn small_video_and_unreadable_list_are_rejected() {
    let h = Harness::new().await;
    let service = h.checked(vec![], PrecheckConfig::default());
    let small = torrent("Sample", &[("sample.mkv", 12 * MB)]);
    let AddOutcome::Added(item) = service.add(AddSource::Metainfo(small), None).await.unwrap()
    else {
        panic!("expected a new item");
    };
    assert_eq!(
        item.error.as_deref(),
        Some("rejected before Put.io: only a small video (12 MB, minimum 50 MB)")
    );

    // The info dict hashes, but has no file list
    let no_files =
        b"d4:infod4:name3:abc12:piece lengthi1e6:pieces20:aaaaaaaaaaaaaaaaaaaaee".to_vec();
    let AddOutcome::Added(item) = service
        .add(AddSource::Metainfo(no_files), None)
        .await
        .unwrap()
    else {
        panic!("expected a new item");
    };
    assert_eq!(
        item.error.as_deref(),
        Some("rejected before Put.io: not a valid .torrent")
    );
}

#[tokio::test]
async fn category_with_the_check_off_goes_to_putio() {
    let mut h = Harness::new().await;
    let upload = h.upload_expected().await;
    let mut cfg = PrecheckConfig::default();
    cfg.categories.insert(
        "sonarr".to_string(),
        PrecheckOverride {
            enabled: Some(false),
            ..PrecheckOverride::default()
        },
    );
    let service = h.checked(vec![], cfg);
    let dir = format!("{}/sonarr", h.target().display());
    let exe = torrent("Tool", &[("setup.exe", 1500 * MB)]);
    let AddOutcome::Added(item) = service
        .add(AddSource::Metainfo(exe), Some(&dir))
        .await
        .unwrap()
    else {
        panic!("expected a new item");
    };
    assert_eq!(item.transfer_id, Some(TRANSFER));
    assert!(checks(&h.db).await.is_empty());
    upload.assert_async().await;
}

#[tokio::test]
async fn dry_run_logs_but_sends_to_putio() {
    let mut h = Harness::new().await;
    let upload = h.upload_expected().await;
    let cfg = PrecheckConfig {
        dry_run: true,
        ..PrecheckConfig::default()
    };
    let service = h.checked(vec![], cfg);
    let dir = format!("{}/sonarr", h.target().display());
    let exe = torrent("Show.S01E03.1080p", &[("Show.S01E03.exe", 1500 * MB)]);
    let AddOutcome::Added(item) = service
        .add(AddSource::Metainfo(exe), Some(&dir))
        .await
        .unwrap()
    else {
        panic!("expected a new item");
    };
    assert_eq!(item.transfer_id, Some(TRANSFER));
    assert_eq!(item.state, ArrState::Putio);
    upload.assert_async().await;
}
