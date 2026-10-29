//! The release check for watch folders (042 US4): fakes are renamed `.rejected` instead of
//! uploaded, and magnets that missed the caches are checked once Put.io has them.

use std::path::PathBuf;
use std::sync::Arc;

use mockito::{Mock, ServerGuard};
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::{DownloadConfig, PrecheckConfig, PrecheckOverride};
use putio_sync_core::rules::RuleSet;
use putio_sync_db::{Database, NewCheck};
use putio_sync_file_manager::{TorrentWatchCore, WatchStats};
use putio_sync_precheck::Prechecker;

const MB: u64 = 1024 * 1024;
const HASH: &str = "c12fe1c06bba254a9dc9f519b335aa7c1367a88a";

/// A .torrent with these files
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

fn transfer(id: i64, status: &str, file_id: Option<i64>) -> String {
    format!(
        r#"{{"id":{id},"name":"x","size":1000,"downloaded":5000,"status":"{status}","created_at":"2026-10-26T08:00:00","save_parent_id":0,"is_private":false,"file_id":{}}}"#,
        file_id.map_or("null".to_string(), |id| id.to_string()),
    )
}

fn file(id: i64, name: &str, size: u64) -> String {
    format!(
        r#"{{"file":{{"id":{id},"name":"{name}","size":{size},"file_type":"VIDEO","parent_id":0,"is_folder":false}},"status":"OK"}}"#
    )
}

struct Fixture {
    _dir: tempfile::TempDir,
    watch: PathBuf,
    db: Database,
    putio: ServerGuard,
}

impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let watch = dir.path().join("watch");
        std::fs::create_dir(&watch).unwrap();
        let db = Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap();
        Self {
            _dir: dir,
            watch,
            db,
            putio: mockito::Server::new_async().await,
        }
    }

    fn client(&self) -> PutioApiClient {
        PutioApiClient::with_base_url("test_token".to_string(), self.putio.url())
    }

    /// `expect` uploads answering with transfer 77
    async fn uploads(&mut self, path: &str, expect: usize) -> Mock {
        self.putio
            .mock("POST", path)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{"transfer":{},"status":"OK"}}"#,
                transfer(77, "IN_QUEUE", None)
            ))
            .expect(expect)
            .create_async()
            .await
    }

    async fn post(&mut self, path: &str, expect: usize) -> Mock {
        self.putio
            .mock("POST", path)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"OK"}"#)
            .expect(expect)
            .create_async()
            .await
    }

    async fn get(&mut self, path: &str, body: String) {
        self.putio
            .mock("GET", path)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create_async()
            .await;
    }

    async fn scan(&self, cfg: PrecheckConfig) -> WatchStats {
        let rules = RuleSet::from_config(&DownloadConfig::default());
        let core = TorrentWatchCore::new().with_precheck(Arc::new(Prechecker::new(cfg, rules)));
        core.scan_directories(
            std::slice::from_ref(&self.watch),
            &self.client(),
            None,
            None,
            false,
            Some(&self.db),
        )
        .await
        .unwrap()
    }

    /// (source, stage, state, reason, transfer_id, watch_folder) of each check row
    async fn checks(&self) -> Vec<CheckRow> {
        sqlx::query_as(
            "SELECT source, stage, state, reason, transfer_id, watch_folder FROM release_checks ORDER BY id",
        )
        .fetch_all(self.db.pool())
        .await
        .unwrap()
    }

    /// (source, stage, reason, watch_folder) of each report
    async fn reports(&self) -> Vec<(String, String, String, Option<String>)> {
        sqlx::query_as(
            "SELECT source, stage, reason, watch_folder FROM release_reports ORDER BY id",
        )
        .fetch_all(self.db.pool())
        .await
        .unwrap()
    }

    fn folder(&self) -> Option<String> {
        Some(self.watch.display().to_string())
    }

    fn has(&self, name: &str) -> bool {
        self.watch.join(name).exists()
    }
}

type CheckRow = (
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<String>,
);

fn lookup_off() -> PrecheckConfig {
    PrecheckConfig {
        magnet_lookup: false,
        ..PrecheckConfig::default()
    }
}

#[tokio::test]
async fn fake_torrent_is_renamed_and_real_one_uploaded() {
    let mut f = Fixture::new().await;
    let fake = torrent("Movie.2024", &[("Movie.2024.exe", 1500 * MB)]);
    let real = torrent("Movie.2025", &[("Movie.2025.mkv", 2048 * MB)]);
    std::fs::write(f.watch.join("fake.torrent"), &fake).unwrap();
    std::fs::write(f.watch.join("real.torrent"), &real).unwrap();
    let upload = f.uploads("/files/upload", 1).await;

    let stats = f.scan(PrecheckConfig::default()).await;
    upload.assert_async().await;
    assert_eq!((stats.files_processed, stats.files_rejected), (1, 1));
    assert!(f.has("fake.torrent.rejected"));
    assert!(!f.has("fake.torrent"));
    assert!(f.has("real.torrent"));

    let rows = f.checks().await;
    let rejected = rows.iter().find(|r| r.2 == "rejected").unwrap();
    let passed = rows.iter().find(|r| r.2 == "passed").unwrap();
    assert_eq!(
        rejected,
        &(
            "watch_torrent".into(),
            "before".into(),
            "rejected".into(),
            Some("no_wanted".into()),
            None,
            f.folder()
        )
    );
    assert_eq!(passed.4, Some(77));
    assert_eq!(
        f.reports().await,
        vec![(
            "watch_torrent".into(),
            "before".into(),
            "no_wanted".into(),
            f.folder()
        )]
    );

    // The renamed file isn't picked up again
    let stats = f.scan(PrecheckConfig::default()).await;
    assert_eq!(stats.files_rejected, 0);
    upload.assert_async().await;
}

#[tokio::test]
async fn check_off_for_the_folder_uploads_as_before() {
    let mut f = Fixture::new().await;
    std::fs::write(
        f.watch.join("fake.torrent"),
        torrent("Movie.2024", &[("Movie.2024.exe", 1500 * MB)]),
    )
    .unwrap();
    let upload = f.uploads("/files/upload", 1).await;
    let mut cfg = PrecheckConfig::default();
    cfg.watch.insert(
        f.watch.display().to_string(),
        PrecheckOverride {
            enabled: Some(false),
            ..PrecheckOverride::default()
        },
    );

    let stats = f.scan(cfg).await;
    upload.assert_async().await;
    assert_eq!((stats.files_processed, stats.files_rejected), (1, 0));
    assert!(f.checks().await.is_empty());
}

#[tokio::test]
async fn dry_run_uploads_the_fake() {
    let mut f = Fixture::new().await;
    std::fs::write(
        f.watch.join("fake.torrent"),
        torrent("Movie.2024", &[("Movie.2024.exe", 1500 * MB)]),
    )
    .unwrap();
    let upload = f.uploads("/files/upload", 1).await;

    let stats = f
        .scan(PrecheckConfig {
            dry_run: true,
            ..PrecheckConfig::default()
        })
        .await;
    upload.assert_async().await;
    assert_eq!(stats.files_rejected, 0);
    assert!(f.has("fake.torrent"));
}

#[tokio::test]
async fn missed_magnet_is_uploaded_with_its_check_pending() {
    let mut f = Fixture::new().await;
    std::fs::write(
        f.watch.join("x.magnet"),
        format!("magnet:?xt=urn:btih:{HASH}&dn=Movie.2024"),
    )
    .unwrap();
    let upload = f.uploads("/transfers/add", 1).await;

    let stats = f.scan(lookup_off()).await;
    upload.assert_async().await;
    assert_eq!(stats.files_processed, 1);
    assert_eq!(
        f.checks().await,
        vec![(
            "watch_magnet".into(),
            "before".into(),
            "pending".into(),
            None,
            Some(77),
            f.folder()
        )]
    );
}

/// A watch magnet waiting for its check on Put.io on `transfer_id`
async fn pending(f: &Fixture, transfer_id: i64) {
    let folder = f.folder();
    f.db.releases()
        .insert_check(&NewCheck {
            hash: Some(HASH),
            name: "Movie.2024",
            source: "watch_magnet",
            watch_folder: folder.as_deref(),
            stage: "before",
            lookup: Some("off"),
            transfer_id: Some(transfer_id),
            ..NewCheck::default()
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn pending_magnets_are_checked_on_putio() {
    let mut f = Fixture::new().await;
    // 1 is a fake, 2 is real, 3 is still downloading, 4 errored and 5 is gone
    for id in 1..=5 {
        pending(&f, id).await;
    }
    let listing = [
        transfer(1, "COMPLETED", Some(101)),
        transfer(2, "SEEDING", Some(102)),
        transfer(3, "DOWNLOADING", None),
        transfer(4, "ERROR", None),
    ];
    f.get(
        "/transfers/list",
        format!(r#"{{"transfers":[{}],"status":"OK"}}"#, listing.join(",")),
    )
    .await;
    f.get("/files/101", file(101, "Movie.2024.exe", 1500 * MB))
        .await;
    f.get("/files/102", file(102, "Movie.2024.mkv", 2048 * MB))
        .await;
    let delete = f.post("/files/delete", 1).await;
    let cancel = f.post("/transfers/cancel", 1).await;
    let clean = f.post("/transfers/clean", 1).await;

    f.scan(PrecheckConfig::default()).await;
    delete.assert_async().await;
    cancel.assert_async().await;
    clean.assert_async().await;

    let states: Vec<(String, String, Option<String>)> = f
        .checks()
        .await
        .into_iter()
        .map(|r| (r.1, r.2, r.3))
        .collect();
    let row = |stage: &str, state: &str, reason: Option<&str>| {
        (
            stage.to_string(),
            state.to_string(),
            reason.map(str::to_string),
        )
    };
    assert_eq!(
        states,
        vec![
            row("putio", "rejected", Some("no_wanted")),
            row("putio", "passed", None),
            row("before", "pending", None),
            row("putio", "rejected", Some("gone")),
            row("putio", "rejected", Some("gone")),
        ]
    );
    assert_eq!(
        f.reports().await,
        vec![(
            "watch_magnet".into(),
            "putio".into(),
            "no_wanted".into(),
            f.folder()
        )]
    );
}

#[tokio::test]
async fn pending_magnet_dry_run_changes_nothing_on_putio() {
    let mut f = Fixture::new().await;
    pending(&f, 1).await;
    f.get(
        "/transfers/list",
        format!(
            r#"{{"transfers":[{}],"status":"OK"}}"#,
            transfer(1, "COMPLETED", Some(101))
        ),
    )
    .await;
    f.get("/files/101", file(101, "Movie.2024.exe", 1500 * MB))
        .await;
    let delete = f.post("/files/delete", 0).await;

    f.scan(PrecheckConfig {
        dry_run: true,
        ..PrecheckConfig::default()
    })
    .await;
    delete.assert_async().await;
    let rows = f.checks().await;
    assert_eq!(
        (rows[0].1.as_str(), rows[0].2.as_str()),
        ("putio", "passed")
    );
}
