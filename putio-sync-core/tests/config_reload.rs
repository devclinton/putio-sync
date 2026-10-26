//! The config poller passes a changed `[logging]` section to its callback, and logs bad
//! or missing files once without stopping (US4, T033).

use putio_sync_core::config::spawn_reload_poller;
use putio_sync_core::logging::LoggingConfig;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

#[derive(Clone, Default)]
struct Errors(Arc<Mutex<usize>>);

impl<S: Subscriber> Layer<S> for Errors {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() == Level::ERROR {
            *self.0.lock().unwrap() += 1;
        }
    }
}

impl Errors {
    fn count(&self) -> usize {
        *self.0.lock().unwrap()
    }
}

async fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..50 {
        if done() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    done()
}

#[tokio::test(flavor = "current_thread")]
async fn reloads_on_change_and_survives_bad_files() {
    let errors = Errors::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(errors.clone()));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[logging]\nlevel = \"info\"\n").unwrap();

    let seen: Arc<Mutex<Vec<LoggingConfig>>> = Arc::default();
    let record = seen.clone();
    let poller = spawn_reload_poller(path.clone(), Duration::from_millis(50), move |l| {
        record.lock().unwrap().push(l)
    });
    // Let the poller take its starting stamp.
    tokio::time::sleep(Duration::from_millis(100)).await;

    std::fs::write(&path, "[logging]\nlevel = \"debug\"\n").unwrap();
    assert!(
        wait_until(|| seen.lock().unwrap().len() == 1).await,
        "no callback within 500 ms"
    );
    assert_eq!(seen.lock().unwrap()[0].level, "debug");

    std::fs::write(&path, "[logging\nlevel = ").unwrap();
    assert!(wait_until(|| errors.count() == 1).await);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(errors.count(), 1, "invalid TOML logs once");
    assert_eq!(
        seen.lock().unwrap().len(),
        1,
        "no callback for invalid TOML"
    );

    std::fs::remove_file(&path).unwrap();
    assert!(wait_until(|| errors.count() == 2).await);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(errors.count(), 2, "missing file logs once");
    assert!(!poller.is_finished());

    poller.abort();
}
