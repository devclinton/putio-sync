//! Limits: a plugin that loops, eats memory, raises errors or falls behind affects only
//! itself, and emitting an event never waits for a plugin.

use std::path::Path;
use std::time::{Duration, Instant};

use putio_sync_core::events::{self, EventType, Payload};
use putio_sync_core::plugins::PluginSettings;
use putio_sync_plugins::manifest::Manifest;
use putio_sync_plugins::worker::{Job, Slot, Spec, MAX_QUEUE};
use putio_sync_plugins::{PluginRun, PluginState, PluginsOptions, PluginsService, RunResult};

/// The bus is global, so these tests take turns
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// `download.failed` misbehaves in the way the script says; `download.finished` is fine
fn misbehaving(dir: &Path, name: &str, bad: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("plugin.toml"),
        format!("name = \"{}\"\nversion = \"1\"\nevents = [\"*\"]\n", name),
    )
    .unwrap();
    let script = format!(
        "local p = {{}}\nfunction p.init() end\nfunction p.on_event(e)\n  if e.type == 'download.failed' then {} end\nend\nreturn p\n",
        bad
    );
    std::fs::write(path.join("main.lua"), script).unwrap();
}

fn settings(name: &str, memory_mb: u64) -> PluginSettings {
    PluginSettings {
        name: name.to_string(),
        enabled: true,
        timeout_secs: 1,
        memory_mb,
        ..PluginSettings::default()
    }
}

fn emit(t: EventType) {
    let payload: Payload = events::samples()
        .into_iter()
        .find(|p| p.event_type() == t)
        .unwrap();
    events::emit(payload);
}

fn runs(plugins: &PluginsService, name: &str) -> Vec<PluginRun> {
    plugins
        .status()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap()
        .runs
}

async fn wait_for(what: &str, secs: u64, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(secs);
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {}", what);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn kinds(runs: &[PluginRun]) -> Vec<(String, RunResult)> {
    runs.iter()
        .map(|r| (r.event_type.clone(), r.result))
        .collect()
}

fn service(dir: &Path, settings: Vec<PluginSettings>) -> PluginsService {
    let plugins = PluginsService::new(PluginsOptions {
        dir: Some(dir.to_path_buf()),
        settings,
        ..PluginsOptions::default()
    });
    plugins.start();
    plugins
}

fn ok(t: &str) -> (String, RunResult) {
    (t.to_string(), RunResult::Success)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_loop_times_out_and_the_next_event_runs() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    misbehaving(dir.path(), "looper", "while true do end");
    let plugins = service(dir.path(), vec![settings("looper", 32)]);
    emit(EventType::DownloadFailed);
    emit(EventType::DownloadFinished);
    wait_for("both runs", 5, || runs(&plugins, "looper").len() == 4).await;
    let r = runs(&plugins, "looper");
    assert_eq!(
        kinds(&r),
        vec![
            ok("init"),
            ("download.failed".to_string(), RunResult::Timeout),
            ok("init"),
            ok("download.finished"),
        ]
    );
    assert!(
        r[1].duration_ms >= 1000 && r[1].duration_ms < 2000,
        "{}",
        r[1].duration_ms
    );
    assert_eq!(r[1].error.as_deref(), Some("time limit reached (1 s)"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_memory_hog_fails_and_the_state_is_rebuilt() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    misbehaving(
        dir.path(),
        "hog",
        "local t = {} for i = 1, 1e8 do t[i] = string.rep('x', 64) .. i end",
    );
    let plugins = service(dir.path(), vec![settings("hog", 4)]);
    emit(EventType::DownloadFailed);
    emit(EventType::DownloadFinished);
    wait_for("both runs", 5, || runs(&plugins, "hog").len() == 4).await;
    let r = runs(&plugins, "hog");
    assert_eq!(
        kinds(&r),
        vec![
            ok("init"),
            ("download.failed".to_string(), RunResult::Failure),
            ok("init"),
            ok("download.finished"),
        ]
    );
    assert_eq!(r[1].error.as_deref(), Some("out of memory"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_error_is_a_failure_and_later_events_run() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    misbehaving(dir.path(), "thrower", "error('nope')");
    let plugins = service(dir.path(), vec![settings("thrower", 32)]);
    emit(EventType::DownloadFailed);
    emit(EventType::DownloadFinished);
    wait_for("both runs", 5, || runs(&plugins, "thrower").len() == 3).await;
    let r = runs(&plugins, "thrower");
    assert_eq!(
        kinds(&r),
        vec![
            ok("init"),
            ("download.failed".to_string(), RunResult::Failure),
            ok("download.finished"),
        ],
        "an error keeps the state"
    );
    assert_eq!(r[1].error.as_deref(), Some("main.lua:4: nope"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_plugin_does_not_delay_another_or_the_bus() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    misbehaving(dir.path(), "slow", "while true do end");
    misbehaving(dir.path(), "quick", "");
    let plugins = service(
        dir.path(),
        vec![settings("slow", 32), settings("quick", 32)],
    );

    let started = Instant::now();
    for _ in 0..3 {
        emit(EventType::DownloadFailed);
    }
    for _ in 0..2000 {
        emit(EventType::StorageLow);
    }
    for _ in 0..3 {
        emit(EventType::DownloadFinished);
    }
    assert!(started.elapsed() < Duration::from_secs(1), "emit waited");
    wait_for("the quick plugin", 3, || {
        runs(&plugins, "quick")
            .iter()
            .filter(|r| r.event_type == "download.finished")
            .count()
            == 3
    })
    .await;
    assert!(started.elapsed() < Duration::from_secs(3));
    let slow = plugins
        .status()
        .into_iter()
        .find(|p| p.name == "slow")
        .unwrap();
    assert!(slow.queued <= MAX_QUEUE, "{}", slow.queued);
}

#[test]
fn a_full_queue_drops_the_oldest_and_keeps_one_tick() {
    let manifest =
        Manifest::parse("name = \"q\"\nversion = \"1\"\nevents = [\"*\"]\n", "q").unwrap();
    let slot = Slot::new(Spec {
        manifest,
        script: String::new(),
        settings: settings("q", 32),
        allowed: Default::default(),
    });
    slot.push(Job::Tick);
    slot.push(Job::Tick);
    assert_eq!(slot.queued(), 1);
    for _ in 0..MAX_QUEUE + 5 {
        slot.push(Job::Tick);
        let payload = events::samples().remove(0);
        slot.push(Job::Event(std::sync::Arc::new(events::Event::new(payload))));
    }
    assert_eq!(slot.queued(), MAX_QUEUE);
    slot.stop();
    assert_eq!(slot.queued(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_load_failure_leaves_the_service_running() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(
        bad.join("plugin.toml"),
        "name = \"bad\"\nversion = \"1\"\nevents = [\"*\"]\n",
    )
    .unwrap();
    std::fs::write(bad.join("main.lua"), "while true do end").unwrap();
    misbehaving(dir.path(), "good", "");
    let plugins = service(dir.path(), vec![settings("bad", 32), settings("good", 32)]);
    emit(EventType::DownloadFinished);
    wait_for("the good plugin", 5, || {
        runs(&plugins, "good")
            .iter()
            .any(|r| r.event_type == "download.finished")
    })
    .await;
    wait_for("the bad plugin to break", 5, || {
        plugins
            .status()
            .iter()
            .any(|p| p.name == "bad" && p.state == PluginState::Broken)
    })
    .await;
    let bad = plugins
        .status()
        .into_iter()
        .find(|p| p.name == "bad")
        .unwrap();
    assert_eq!(bad.error.as_deref(), Some("time limit reached (1 s)"));
    let (found, _) = plugins.reload();
    assert!(found >= 2);
}
