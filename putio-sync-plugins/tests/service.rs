//! The plugins service end to end: events from the bus reach the plugins that want them,
//! the store survives a reload, tests reach only the plugin tested, and a broken plugin
//! does not stop the others.

use std::path::Path;

use std::time::{Duration, Instant};

use putio_sync_core::events::{self, EventType, Payload, PluginTest};
use putio_sync_core::plugins::PluginSettings;
use putio_sync_plugins::{PluginState, PluginsOptions, PluginsService, RunResult, TestError};

/// The bus is global, so these tests take turns
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const RECORDER: &str = r#"
local plugin = {}
function plugin.init() putio.log.info("ready") end
function plugin.on_event(event)
  local ids = putio.store.get("ids") or {}
  table.insert(ids, event.id)
  putio.store.set("ids", ids)
  putio.log.info("got " .. event.type, {plugin = event.data.plugin})
end
return plugin
"#;

fn write(dir: &Path, folder: &str, manifest: &str, script: &str) {
    let path = dir.join(folder);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("plugin.toml"), manifest).unwrap();
    std::fs::write(path.join("main.lua"), script).unwrap();
}

fn enabled(name: &str) -> PluginSettings {
    PluginSettings {
        name: name.to_string(),
        enabled: true,
        ..PluginSettings::default()
    }
}

fn payload(t: EventType) -> Payload {
    events::samples()
        .into_iter()
        .find(|p| p.event_type() == t)
        .unwrap()
}

fn stored_ids(data: &Path) -> Vec<String> {
    std::fs::read_to_string(data.join("recorder.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| serde_json::from_value(v["ids"].clone()).ok())
        .unwrap_or_default()
}

async fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {}", what);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn service(dir: &Path, data: &Path, settings: Vec<PluginSettings>) -> PluginsService {
    PluginsService::new(PluginsOptions {
        dir: Some(dir.to_path_buf()),
        data_dir: Some(data.to_path_buf()),
        settings,
        clock: None,
        download_dir: None,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn events_reach_the_plugin_and_the_store_survives_a_reload() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "recorder",
        "name = \"recorder\"\nversion = \"1\"\nevents = [\"download.finished\"]\n[permissions]\nstore = true\n",
        RECORDER,
    );
    let plugins = service(dir.path(), data.path(), vec![enabled("recorder")]);
    plugins.start();
    assert!(plugins.wants(EventType::DownloadFinished));
    assert!(!plugins.wants(EventType::SyncCompleted));

    let mut sent = Vec::new();
    for _ in 0..3 {
        sent.push(
            events::emit(payload(EventType::DownloadFinished))
                .id
                .clone(),
        );
        events::emit(payload(EventType::SyncCompleted));
    }
    wait_for("three ids", || stored_ids(data.path()).len() == 3).await;
    assert_eq!(stored_ids(data.path()), sent);

    let (found, broken) = plugins.reload();
    assert!(found >= 1 && broken == 0);
    sent.push(
        events::emit(payload(EventType::DownloadFinished))
            .id
            .clone(),
    );
    wait_for("four ids", || stored_ids(data.path()).len() == 4).await;
    assert_eq!(stored_ids(data.path()), sent);

    let status = plugins.status();
    let me = status.iter().find(|p| p.name == "recorder").unwrap();
    assert_eq!(me.state, PluginState::Enabled);
    assert_eq!(me.source, "folder");
    let events: Vec<&str> = me.runs.iter().map(|r| r.event_type.as_str()).collect();
    assert_eq!(events.iter().filter(|e| **e == "init").count(), 2);
    assert_eq!(
        events.iter().filter(|e| **e == "download.finished").count(),
        4
    );
    assert!(me.runs.iter().all(|r| r.result == RunResult::Success));
    assert!(me.runs.iter().all(|r| r.logs.is_empty()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_events_reach_only_the_plugin_tested() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "recorder",
        "name = \"recorder\"\nversion = \"1\"\nevents = [\"*\"]\n[permissions]\nstore = true\n",
        RECORDER,
    );
    let plugins = service(dir.path(), data.path(), vec![enabled("recorder")]);
    plugins.start();

    // On the bus, plugin.test is ignored, even by "*".
    events::emit(Payload::PluginTest(PluginTest {
        plugin: "recorder".to_string(),
        message: "x".to_string(),
    }));
    let run = plugins.test("recorder").await.unwrap();
    assert_eq!(run.event_type, "plugin.test");
    assert_eq!(run.result, RunResult::Success);
    assert_eq!(run.logs.len(), 1);
    assert_eq!(run.logs[0].message, "got plugin.test plugin=recorder");
    assert_eq!(stored_ids(data.path()), vec![run.event_id.clone()]);

    assert_eq!(plugins.test("nope").await.unwrap_err(), TestError::NotFound);

    // A disabled plugin is tested on a worker of its own.
    plugins.configure(vec![PluginSettings {
        enabled: false,
        ..enabled("recorder")
    }]);
    assert!(!plugins.wants(EventType::DownloadFinished));
    let run = plugins.test("recorder").await.unwrap();
    assert_eq!(run.result, RunResult::Success);
    let state = plugins
        .status()
        .into_iter()
        .find(|p| p.name == "recorder")
        .unwrap()
        .state;
    assert_eq!(state, PluginState::Disabled, "testing does not enable");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_broken_plugin_is_listed_and_the_others_run() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "recorder",
        "name = \"recorder\"\nversion = \"1\"\nevents = [\"download.finished\"]\n[permissions]\nstore = true\n",
        RECORDER,
    );
    write(
        dir.path(),
        "syntax",
        "name = \"syntax\"\nversion = \"1\"\nevents = [\"*\"]\n",
        "return {on_event = function( end}",
    );
    write(dir.path(), "manifest", "name = \"manifest\"\n", "");
    write(
        dir.path(),
        "initfail",
        "name = \"initfail\"\nversion = \"1\"\nevents = [\"*\"]\n",
        "return {init = function() error('no config') end, on_event = function() end}",
    );
    let settings = ["recorder", "syntax", "manifest", "initfail"]
        .into_iter()
        .map(enabled)
        .collect();
    let plugins = service(dir.path(), data.path(), settings);
    plugins.start();

    let id = events::emit(payload(EventType::DownloadFinished))
        .id
        .clone();
    wait_for("the recorder", || {
        stored_ids(data.path()) == vec![id.clone()]
    })
    .await;
    wait_for("both load errors", || {
        plugins
            .status()
            .iter()
            .filter(|p| p.state == PluginState::Broken)
            .count()
            == 3
    })
    .await;

    let status = plugins.status();
    let error = |name: &str| {
        let p = status.iter().find(|p| p.name == name).unwrap();
        assert_eq!(p.state, PluginState::Broken, "{}", name);
        p.error.clone().unwrap()
    };
    assert!(
        error("syntax").starts_with("main.lua:1:"),
        "{}",
        error("syntax")
    );
    assert!(
        error("manifest").contains("plugin.toml"),
        "{}",
        error("manifest")
    );
    assert_eq!(error("initfail"), "init: main.lua:1: no config");

    match plugins.test("manifest").await {
        Err(TestError::Broken(e)) => assert!(e.contains("plugin.toml")),
        other => panic!("{:?}", other),
    }
    let run = plugins.test("syntax").await.unwrap();
    assert_eq!(run.result, RunResult::Failure);
    assert!(run.error.unwrap().starts_with("main.lua:1:"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn check_approves_needed_hosts_and_refuses_bad_values() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "caller",
        concat!(
            "name = \"caller\"\nversion = \"1\"\nevents = [\"*\"]\n",
            "[permissions.http]\nhosts = [\"api.example.com\"]\nhost_from_config = [\"server\"]\n",
            "[[config]]\nkey = \"server\"\ntype = \"url\"\nrequired = true\n",
        ),
        "return {on_event = function() end}",
    );
    let plugins = service(dir.path(), dir.path(), vec![]);
    let mut s = enabled("caller");
    s.config
        .insert("server".into(), "https://ntfy.example.org/x".into());
    let checked = plugins.check(&[s.clone()]).unwrap();
    assert_eq!(
        checked[0].approved_hosts,
        vec!["api.example.com", "ntfy.example.org"]
    );
    let state = || {
        plugins
            .status()
            .into_iter()
            .find(|p| p.name == "caller")
            .unwrap()
            .state
    };
    plugins.configure(vec![s.clone()]);
    assert_eq!(state(), PluginState::NeedsApproval);
    plugins.configure(checked);
    assert_eq!(state(), PluginState::Enabled);
    plugins.configure(vec![enabled("caller")]);
    assert_eq!(state(), PluginState::MissingConfig);

    s.config.insert("server".into(), 5.into());
    assert!(plugins
        .check(&[s])
        .unwrap_err()
        .contains("plugins \"caller\".config.server"));
}
