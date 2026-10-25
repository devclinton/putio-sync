//! Watch files that were already uploaded get the post-upload action instead of sitting in
//! the watch folder forever (41 stuck magnets on the NAS), and idle scans stay quiet.

use putio_sync_client::PutioApiClient;
use putio_sync_db::Database;
use putio_sync_file_manager::{calculate_string_checksum, TorrentWatchCore, WatchStats};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

const MAGNET: &str = "magnet:?xt=urn:btih:abc";

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<(Level, String, String)>>>);

impl Captured {
    fn count(&self, level: Level, target_prefix: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(l, t, _)| *l == level && t.starts_with(target_prefix))
            .count()
    }
}

struct MessageVisitor(String);

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl<S: Subscriber> Layer<S> for Captured {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor(String::new());
        event.record(&mut visitor);
        self.0.lock().unwrap().push((
            *event.metadata().level(),
            event.metadata().target().to_string(),
            visitor.0,
        ));
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    watch: PathBuf,
    archive: PathBuf,
    db: Database,
    server: mockito::ServerGuard,
    upload: mockito::Mock,
}

async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let watch = dir.path().join("watch");
    std::fs::create_dir(&watch).unwrap();
    std::fs::write(watch.join("x.magnet"), MAGNET).unwrap();

    let db = Database::new(dir.path().join(".putiosync.db"))
        .await
        .unwrap();
    db.processed_torrents()
        .record_processed(
            watch.join("x.magnet").to_str().unwrap(),
            &calculate_string_checksum(MAGNET),
            MAGNET.len() as i64,
            Some(MAGNET),
        )
        .await
        .unwrap();

    let mut server = mockito::Server::new_async().await;
    let upload = server
        .mock("POST", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;

    Fixture {
        archive: dir.path().join("archive"),
        _dir: dir,
        watch,
        db,
        server,
        upload,
    }
}

async fn scan(
    f: &Fixture,
    core: &TorrentWatchCore,
    archive: Option<&PathBuf>,
    delete: bool,
) -> WatchStats {
    let client = PutioApiClient::with_base_url("test_token".to_string(), f.server.url());
    core.scan_directories(
        std::slice::from_ref(&f.watch),
        &client,
        None,
        archive,
        delete,
        Some(&f.db),
    )
    .await
    .unwrap()
}

fn exists(p: &Path) -> bool {
    p.exists()
}

#[tokio::test]
async fn already_uploaded_file_is_deleted_not_reuploaded() {
    let f = fixture().await;
    let stats = scan(&f, &TorrentWatchCore::new(), None, true).await;

    assert!(!exists(&f.watch.join("x.magnet")));
    assert_eq!(stats.files_processed, 0);
    assert_eq!(stats.cleaned_up, 1);
    f.upload.assert_async().await;
}

#[tokio::test]
async fn already_uploaded_file_is_archived() {
    let f = fixture().await;
    let stats = scan(&f, &TorrentWatchCore::new(), Some(&f.archive), false).await;

    assert!(!exists(&f.watch.join("x.magnet")));
    assert!(exists(&f.archive.join("x.magnet")));
    assert_eq!(stats.cleaned_up, 1);
    f.upload.assert_async().await;
}

#[tokio::test]
async fn already_uploaded_file_stays_without_an_action() {
    let f = fixture().await;
    let stats = scan(&f, &TorrentWatchCore::new(), None, false).await;

    assert!(exists(&f.watch.join("x.magnet")));
    assert_eq!(stats.cleaned_up, 0);
    assert_eq!(stats.files_skipped, 1);
    f.upload.assert_async().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "current_thread")]
async fn failed_cleanup_warns_once() {
    use std::os::unix::fs::PermissionsExt;

    let captured = Captured::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(captured.clone()));

    let f = fixture().await;
    std::fs::set_permissions(&f.watch, std::fs::Permissions::from_mode(0o555)).unwrap();

    let core = TorrentWatchCore::new();
    scan(&f, &core, None, true).await;
    scan(&f, &core, None, true).await;

    std::fs::set_permissions(&f.watch, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(exists(&f.watch.join("x.magnet")));
    assert_eq!(captured.count(Level::WARN, "putio_sync_file_manager"), 1);
    f.upload.assert_async().await;
}

#[tokio::test(flavor = "current_thread")]
async fn idle_scan_logs_nothing_at_info() {
    let captured = Captured::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(captured.clone()));

    let f = fixture().await;
    let core = TorrentWatchCore::new();
    scan(&f, &core, None, false).await;
    scan(&f, &core, None, false).await;

    assert_eq!(
        captured.count(Level::INFO, "putio_sync_file_manager"),
        0,
        "{:?}",
        captured.0.lock().unwrap()
    );
}
