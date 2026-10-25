//! Executables on Put.io are never queued, and each one is warned about once (FR-008–FR-010).

use putio_sync_client::PutioApiClient;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

const BLOCKED_MSG: &str = "Blocked file not downloaded";

struct CountBlockedWarns(Arc<AtomicUsize>);

struct MessageVisitor(bool);

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" && format!("{value:?}") == BLOCKED_MSG {
            self.0 = true;
        }
    }
}

impl<S: Subscriber> Layer<S> for CountBlockedWarns {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() != Level::WARN {
            return;
        }
        let mut visitor = MessageVisitor(false);
        event.record(&mut visitor);
        if visitor.0 {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

fn file(id: i64, name: &str) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","size":100,"file_type":"VIDEO","parent_id":0,"is_folder":false,"is_shared":false}}"#
    )
}

#[tokio::test(flavor = "current_thread")]
async fn blocked_files_are_never_queued_and_warned_once() {
    let warns = Arc::new(AtomicUsize::new(0));
    let subscriber = tracing_subscriber::registry().with(CountBlockedWarns(warns.clone()));
    let _guard = tracing::subscriber::set_default(subscriber);

    let mut server = mockito::Server::new_async().await;
    let body = format!(
        r#"{{"files":[{},{},{}],"parent":{{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false}},"status":"OK"}}"#,
        file(1, "Show S01E01.mkv"),
        file(2, "Show S01E02.exe"),
        file(3, "setup.MSI"),
    );
    let _list = server
        .mock("GET", "/files/list?parent_id=0")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(2)
        .create_async()
        .await;

    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(PutioApiClient::with_base_url(
        "test_token".to_string(),
        server.url(),
    ));
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        ..DownloadConfig::default()
    };
    let manager = DownloadManager::new(client, db.clone(), config);

    for _ in 0..2 {
        let stats = manager.sync_downloads().await.unwrap();
        assert_eq!(stats.files_blocked, 2);
    }

    assert!(db.queue().get_by_file_id(1).await.unwrap().is_some());
    for id in [2, 3] {
        assert!(db.queue().get_by_file_id(id).await.unwrap().is_none());
        assert!(db.downloads().find_by_file_id(id).await.unwrap().is_none());
    }
    assert_eq!(
        warns.load(Ordering::SeqCst),
        2,
        "one warning per blocked file"
    );
}
