//! The config file watcher publishes valid hand edits to the hub, refuses invalid ones
//! (keeping the running config), ignores writes equal to the running config, and takes a
//! file only once two reads agree.

use putio_sync_core::config::Config;
use putio_sync_core::config_hub::{spawn_file_watcher, ConfigHub, Source};
use putio_sync_core::error::CoreResult;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

const TICK: Duration = Duration::from_millis(30);

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
    for _ in 0..100 {
        if done() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    done()
}

fn validated(config: Config) -> CoreResult<Config> {
    config.validate()?;
    Ok(config)
}

fn start(path: &Path) -> (Arc<ConfigHub>, tokio::task::JoinHandle<()>) {
    let hub = Arc::new(ConfigHub::new(Config::read(path).unwrap()));
    let watcher = spawn_file_watcher(path.to_path_buf(), TICK, hub.clone(), validated);
    (hub, watcher)
}

#[tokio::test(flavor = "current_thread")]
async fn a_valid_hand_edit_is_published() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[watch]\nrefresh_cycle = 60\n").unwrap();
    let (hub, watcher) = start(&path);
    tokio::time::sleep(TICK * 2).await;

    std::fs::write(&path, "[watch]\nrefresh_cycle = 15\n").unwrap();
    assert!(wait_until(|| hub.snapshot().generation == 1).await);
    let snapshot = hub.snapshot();
    assert_eq!(snapshot.source, Source::File);
    assert_eq!(snapshot.config.watch.refresh_cycle, 15);
    assert!(hub.status().last_error.is_none());
    watcher.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn an_invalid_edit_is_refused_and_logged_once() {
    let errors = Errors::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(errors.clone()));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[watch]\nrefresh_cycle = 60\n").unwrap();
    let (hub, watcher) = start(&path);
    tokio::time::sleep(TICK * 2).await;

    // Not TOML
    std::fs::write(&path, "[watch\nrefresh_cycle = ").unwrap();
    assert!(wait_until(|| errors.count() == 1).await);
    assert!(hub.status().last_error.is_some());
    // TOML that fails validation
    std::fs::write(&path, "[watch]\nrefresh_cycle = 0\n").unwrap();
    assert!(wait_until(|| errors.count() == 2).await);
    assert!(hub
        .status()
        .last_error
        .unwrap()
        .contains("refresh_cycle must be greater than 0"));
    tokio::time::sleep(TICK * 8).await;
    assert_eq!(errors.count(), 2, "each distinct content logs once");
    assert_eq!(hub.snapshot().generation, 0, "the running config stays");
    assert_eq!(hub.current().watch.refresh_cycle, 60);

    // A good edit publishes and clears the error
    std::fs::write(&path, "[watch]\nrefresh_cycle = 20\n").unwrap();
    assert!(wait_until(|| hub.snapshot().generation == 1).await);
    assert!(hub.status().last_error.is_none());
    assert!(!watcher.is_finished());
    watcher.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn a_missing_file_is_logged_once_and_polling_goes_on() {
    let errors = Errors::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(errors.clone()));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[watch]\nrefresh_cycle = 60\n").unwrap();
    let (hub, watcher) = start(&path);
    tokio::time::sleep(TICK * 2).await;

    std::fs::remove_file(&path).unwrap();
    assert!(wait_until(|| errors.count() == 1).await);
    tokio::time::sleep(TICK * 8).await;
    assert_eq!(errors.count(), 1);
    assert!(hub.status().last_error.is_some());

    // Back with the running settings: nothing to apply, and the error is gone
    std::fs::write(&path, "[watch]\nrefresh_cycle = 60\n").unwrap();
    assert!(wait_until(|| hub.status().last_error.is_none()).await);
    assert_eq!(hub.snapshot().generation, 0);
    assert!(!watcher.is_finished());
    watcher.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn a_write_equal_to_the_running_config_publishes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[watch]\nrefresh_cycle = 60\n").unwrap();
    let (hub, watcher) = start(&path);
    tokio::time::sleep(TICK * 2).await;

    // What an API save does: the hub already holds the value when the file changes
    let mut saved = (*hub.current()).clone();
    saved.watch.refresh_cycle = 25;
    hub.publish(saved, Source::Api);
    std::fs::write(&path, "# saved\n[watch]\nrefresh_cycle = 25\n").unwrap();
    tokio::time::sleep(TICK * 10).await;
    assert_eq!(hub.snapshot().generation, 1);
    assert_eq!(hub.snapshot().source, Source::Api);
    watcher.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn a_file_still_being_written_is_not_taken() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[watch]\nrefresh_cycle = 60\n").unwrap();
    let (hub, watcher) = start(&path);
    tokio::time::sleep(TICK * 2).await;

    // Writes come faster than ticks, so two reads in a row never agree until they stop
    for cycle in 31..46 {
        std::fs::write(&path, format!("[watch]\nrefresh_cycle = {cycle}\n")).unwrap();
        tokio::time::sleep(TICK / 3).await;
        assert_eq!(hub.snapshot().generation, 0, "cycle {cycle} taken early");
    }
    assert!(wait_until(|| hub.snapshot().generation == 1).await);
    assert_eq!(hub.current().watch.refresh_cycle, 45);
    watcher.abort();
}
