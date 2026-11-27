//! Feature 057: a plugin as a pipeline step (`on_item`), `on_config_changed` after its
//! settings are saved, and `on_unload` before its state is dropped.

use std::path::Path;
use std::time::{Duration, Instant};

use putio_sync_core::events::{self, EventType};
use putio_sync_core::plugins::PluginSettings;
use putio_sync_core::step_plugins::{StepAnswer, StepPlugins};
use putio_sync_plugins::{PluginsOptions, PluginsService, RunResult};
use serde_json::{json, Value};

/// The bus is global, so these tests take turns
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

fn service(dir: &Path, data: &Path, settings: Vec<PluginSettings>) -> PluginsService {
    PluginsService::new(PluginsOptions {
        dir: Some(dir.to_path_buf()),
        data_dir: Some(data.to_path_buf()),
        settings,
        clock: None,
        download_dir: None,
        reach: Some(putio_sync_core::net_guard::Reach::Local),
    })
}

fn stored(data: &Path, plugin: &str) -> Value {
    std::fs::read_to_string(data.join(format!("{plugin}.json")))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

async fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {}", what);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

const STEP: &str = r#"
local plugin = {}
function plugin.on_item(item, stage)
  putio.store.set("seen", {key = item.key, stage = stage, file = item.files[1].relative_path})
  if item.name == "skip-me" then return "skip" end
  if item.name == "fail-me" then return {fail = "too small"} end
  if item.name == "raise" then error("no room") end
  if item.name == "spin" then while true do end end
  if item.name == "odd" then return 7 end
  return "done"
end
return plugin
"#;

fn item(name: &str) -> Value {
    json!({
        "key": "transfer:9",
        "name": name,
        "category": "sonarr",
        "source": "arr",
        "dir": "/data/Show",
        "stage": "post_processing",
        "files": [{"relative_path": "a.mkv", "size": 5, "putio_id": 3}],
        "transfer_id": 9,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_item_answers_done_skip_fail_error_and_timeout() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "sorter",
        "name = \"sorter\"\nversion = \"1\"\nstep = true\n[permissions]\nstore = true\n",
        STEP,
    );
    let mut settings = enabled("sorter");
    settings.timeout_secs = 1;
    let plugins = std::sync::Arc::new(service(dir.path(), data.path(), vec![settings]));
    plugins.start();

    let ask = |name: &'static str| {
        let plugins = plugins.clone();
        async move {
            plugins
                .run_item("sorter", &item(name), "post_processing")
                .await
        }
    };
    assert_eq!(ask("ok").await, StepAnswer::Done);
    assert_eq!(
        stored(data.path(), "sorter")["seen"],
        json!({"key": "transfer:9", "stage": "post_processing", "file": "a.mkv"})
    );
    assert_eq!(ask("skip-me").await, StepAnswer::Skip);
    assert_eq!(
        ask("fail-me").await,
        StepAnswer::Failed("plugin \"sorter\": too small".to_string())
    );
    assert!(matches!(ask("raise").await, StepAnswer::Failed(m) if m.contains("no room")));
    assert!(matches!(ask("odd").await, StepAnswer::Failed(m) if m.contains("returned 7")));
    assert!(matches!(ask("spin").await, StepAnswer::Timeout(m) if m.contains("sorter")));
    // The state was thrown away by the timeout and is built again for the next item
    assert_eq!(ask("ok").await, StepAnswer::Done);

    let status = plugins.status();
    let me = status.iter().find(|p| p.name == "sorter").unwrap();
    assert!(me.has_item);
    assert!(plugins.problem("sorter").is_none());
    // A call that raised or timed out is kept with the runs; one that returned is counted
    // only (the pipeline keeps the step's own result)
    let items: Vec<_> = me.runs.iter().filter(|r| r.event_type == "item").collect();
    assert_eq!(items.len(), 2);
    assert!(items.iter().any(|r| r.result == RunResult::Timeout));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_step_names_a_plugin_that_is_missing_off_broken_or_has_no_on_item() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let events_only = "local p = {}\nfunction p.on_event() end\nreturn p\n";
    write(
        dir.path(),
        "listener",
        "name = \"listener\"\nversion = \"1\"\nevents = [\"download.finished\"]\n",
        events_only,
    );
    write(
        dir.path(),
        "off",
        "name = \"off\"\nversion = \"1\"\nstep = true\n",
        STEP,
    );
    // Says it is a step and has no on_item: broken when it loads
    write(
        dir.path(),
        "liar",
        "name = \"liar\"\nversion = \"1\"\nstep = true\n",
        "return {}\n",
    );
    let plugins = std::sync::Arc::new(service(
        dir.path(),
        data.path(),
        vec![enabled("listener"), enabled("liar")],
    ));
    plugins.start();

    let ask = |plugin: &'static str| {
        let plugins = plugins.clone();
        async move {
            plugins
                .run_item(plugin, &item("ok"), "post_processing")
                .await
        }
    };
    assert_eq!(
        ask("nope").await,
        StepAnswer::Failed("the plugin \"nope\" is not installed".to_string())
    );
    assert_eq!(
        ask("off").await,
        StepAnswer::Failed("the plugin \"off\" is off".to_string())
    );
    assert_eq!(
        ask("listener").await,
        StepAnswer::Failed("the plugin \"listener\" has no on_item".to_string())
    );
    let StepAnswer::Failed(why) = ask("liar").await else {
        panic!("a broken plugin fails the step");
    };
    assert!(
        why.contains("liar") && why.contains("has no on_item"),
        "{why}"
    );

    assert_eq!(
        plugins.problem("nope").as_deref(),
        Some("the plugin \"nope\" is not installed")
    );
    assert_eq!(
        plugins.problem("off").as_deref(),
        Some("the plugin \"off\" is off")
    );
    assert_eq!(
        plugins.problem("listener").as_deref(),
        Some("the plugin \"listener\" has no on_item")
    );
    assert!(plugins.problem("liar").unwrap().contains("is broken"));
}

const BATCHER: &str = r#"
local plugin = {}
local held = {}
function plugin.on_event(event) table.insert(held, event.type) end
function plugin.on_unload()
  local sent = putio.store.get("sent") or {}
  table.insert(sent, table.concat(held, ","))
  putio.store.set("sent", sent)
end
function plugin.on_config_changed(config)
  local seen = putio.store.get("configs") or {}
  table.insert(seen, config.title)
  putio.store.set("configs", seen)
end
return plugin
"#;

const BATCHER_TOML: &str = "name = \"batcher\"\nversion = \"1\"\nevents = [\"download.finished\"]\n[permissions]\nstore = true\n[[config]]\nkey = \"title\"\ntype = \"string\"\nlabel = \"Title\"\ndefault = \"one\"\n";

fn emit_finished() {
    events::emit(
        events::samples()
            .into_iter()
            .find(|p| p.event_type() == EventType::DownloadFinished)
            .unwrap(),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_unload_runs_before_a_reload_a_turn_off_and_a_stop() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    write(dir.path(), "batcher", BATCHER_TOML, BATCHER);
    let plugins = service(dir.path(), data.path(), vec![enabled("batcher")]);
    plugins.start();
    let sent = || stored(data.path(), "batcher")["sent"].clone();

    // Reload: what was held is sent before the state is dropped, without a new event
    emit_finished();
    emit_finished();
    assert!(plugins.drain(Duration::from_secs(10)).await);
    plugins.reload();
    wait_for("the reload's on_unload", || {
        sent() == json!(["download.finished,download.finished"])
    })
    .await;

    // Turned off: the new state is unloaded too, once
    emit_finished();
    assert!(plugins.drain(Duration::from_secs(10)).await);
    let mut off = enabled("batcher");
    off.enabled = false;
    plugins.configure(vec![off]);
    wait_for("the turn-off's on_unload", || {
        sent().as_array().is_some_and(|a| a.len() == 2)
    })
    .await;
    assert_eq!(sent()[1], "download.finished");

    // Stopped: shutdown waits for on_unload
    plugins.configure(vec![enabled("batcher")]);
    emit_finished();
    assert!(plugins.drain(Duration::from_secs(10)).await);
    assert!(plugins.shutdown(Duration::from_secs(10)).await);
    assert_eq!(
        sent(),
        json!([
            "download.finished,download.finished",
            "download.finished",
            "download.finished"
        ])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_unload_is_bounded_and_is_not_called_on_a_state_that_broke() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "stuck",
        "name = \"stuck\"\nversion = \"1\"\nevents = [\"download.finished\"]\n",
        "local p = {}\nfunction p.on_event() end\nfunction p.on_unload() while true do end end\nreturn p\n",
    );
    write(
        dir.path(),
        "crasher",
        "name = \"crasher\"\nversion = \"1\"\nevents = [\"download.finished\"]\n[permissions]\nstore = true\n",
        "local p = {}\nfunction p.on_event() while true do end end\nfunction p.on_unload() putio.store.set(\"unloaded\", true) end\nreturn p\n",
    );
    let mut stuck = enabled("stuck");
    stuck.timeout_secs = 1;
    let mut crasher = enabled("crasher");
    crasher.timeout_secs = 1;
    let plugins = service(dir.path(), data.path(), vec![stuck, crasher]);
    plugins.start();
    emit_finished();
    assert!(plugins.drain(Duration::from_secs(10)).await);

    let started = Instant::now();
    assert!(plugins.shutdown(Duration::from_secs(10)).await);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "a stop waits for the plugins' limit, not longer"
    );
    // The crasher's state was dropped by its timeout, so it got no on_unload
    assert_eq!(stored(data.path(), "crasher"), Value::Null);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn on_config_changed_is_called_once_with_the_new_settings() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    write(dir.path(), "batcher", BATCHER_TOML, BATCHER);
    write(
        dir.path(),
        "other",
        "name = \"other\"\nversion = \"1\"\nevents = [\"download.finished\"]\n[permissions]\nstore = true\n",
        "local p = {}\nfunction p.on_event() end\nfunction p.on_config_changed() putio.store.set(\"called\", true) end\nreturn p\n",
    );
    let plugins = service(
        dir.path(),
        data.path(),
        vec![enabled("batcher"), enabled("other")],
    );
    plugins.start();
    let configs = || stored(data.path(), "batcher")["configs"].clone();

    let mut changed = enabled("batcher");
    changed.config.insert("title".to_string(), json!("two"));
    plugins.configure(vec![changed.clone(), enabled("other")]);
    wait_for("on_config_changed", || configs() == json!(["two"])).await;

    // The same settings again: no call. Another plugin's change: no call here.
    plugins.configure(vec![changed.clone(), enabled("other")]);
    let mut other = enabled("other");
    other.timeout_secs = 20;
    plugins.configure(vec![changed, other]);
    wait_for("the other plugin's call", || {
        stored(data.path(), "other")["called"] == json!(true)
    })
    .await;
    assert!(plugins.drain(Duration::from_secs(10)).await);
    assert_eq!(configs(), json!(["two"]));
}
