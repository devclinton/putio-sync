//! The host additions of 031 end to end: settings actions, health checks, per-plugin
//! ticks, counters, one store shared by every worker of a plugin, and `putio.path`.

use std::path::Path;
use std::time::{Duration, Instant};

use putio_sync_core::events::{self, EventType};
use putio_sync_core::plugins::PluginSettings;
use putio_sync_plugins::action::HealthStatus;
use putio_sync_plugins::{CallError, PluginsOptions, PluginsService, RunResult};
use serde_json::{json, Map, Value};

const MANIFEST: &str = r#"
name = "media"
version = "1"
events = ["download.finished"]
tick_seconds = 5

[permissions]
store = true

[[config]]
key = "server"
type = "string"

[[config]]
key = "token"
type = "string"
secret = true

[[config]]
key = "library"
type = "string"
choices = "load"

[[config]]
key = "paths"
type = "path_map"

[[actions]]
id = "sign_in"
label = "Sign in"

[[actions]]
id = "poll"
label = "Poll"
hidden = true

[[actions]]
id = "load"
label = "Load"

[[actions]]
id = "bad"
label = "Bad"
"#;

const SCRIPT: &str = r#"
local plugin = {}
function plugin.on_event(event)
  putio.store.set("events", (putio.store.get("events") or 0) + 1)
end
function plugin.on_tick()
  putio.store.set("ticks", (putio.store.get("ticks") or 0) + 1)
end
function plugin.on_action(id, input)
  if id == "sign_in" then
    putio.store.set("client_id", "abc")
    return {
      message = "Enter the code; server " .. tostring(putio.config.server),
      code = "AB12",
      open_url = "https://example.com/link",
      next = {action = "poll", input = {pin = 7}, after_seconds = 1},
    }
  elseif id == "poll" then
    putio.log.info("polling with " .. tostring(putio.config.token))
    return {
      done = true,
      message = "Signed in as " .. putio.store.get("client_id"),
      save = {token = "tok-" .. input.pin .. "-SECRET"},
    }
  elseif id == "load" then
    putio.metrics.count("loads")
    return {choices = {"TV", {value = "2", label = "Movies"}}}
  elseif id == "bad" then
    error("bad action with " .. tostring(putio.config.token))
  end
end
function plugin.health()
  local mapped = putio.path.map("/downloads/TV/x", putio.config.paths)
  return {
    {status = "ok", message = "mapped to " .. mapped},
    {status = putio.config.token and "ok" or "fail",
     message = "token " .. tostring(putio.config.token)},
  }
end
return plugin
"#;

fn write(dir: &Path, name: &str, manifest: &str, script: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(path.join("plugin.toml"), manifest).unwrap();
    std::fs::write(path.join("main.lua"), script).unwrap();
}

fn settings(enabled: bool, token: Option<&str>) -> PluginSettings {
    let mut s = PluginSettings {
        name: "media".to_string(),
        enabled,
        ..PluginSettings::default()
    };
    s.config.insert(
        "paths".to_string(),
        json!([{"from": "/downloads", "to": "D:\\Media"}]),
    );
    if let Some(t) = token {
        s.secrets.insert("token".to_string(), t.to_string());
    }
    s
}

fn service(dir: &Path, data: &Path, s: Vec<PluginSettings>) -> PluginsService {
    PluginsService::new(PluginsOptions {
        dir: Some(dir.to_path_buf()),
        data_dir: Some(data.to_path_buf()),
        settings: s,
        clock: None,
        download_dir: Some("/downloads".into()),
    })
}

fn store(data: &Path) -> Value {
    std::fs::read_to_string(data.join("media.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sign_in_flow_saves_a_secret_it_never_shows() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    write(dir.path(), "media", MANIFEST, SCRIPT);
    let plugins = service(dir.path(), data.path(), vec![settings(false, None)]);

    let status = plugins.status();
    let media = status.iter().find(|p| p.name == "media").unwrap();
    let actions: Vec<_> = media
        .actions
        .iter()
        .map(|a| (a.id.as_str(), a.hidden))
        .collect();
    assert_eq!(
        actions,
        vec![
            ("sign_in", false),
            ("poll", true),
            ("load", false),
            ("bad", false)
        ]
    );

    let mut overrides = Map::new();
    overrides.insert("server".to_string(), json!("http://plex:32400"));
    overrides.insert("token".to_string(), json!("ignored"));
    let first = plugins
        .action("media", "sign_in", json!({}), overrides)
        .await
        .unwrap();
    assert_eq!(
        first.result.message.as_deref(),
        Some("Enter the code; server http://plex:32400")
    );
    assert_eq!(first.result.code.as_deref(), Some("AB12"));
    let next = first.result.next.clone().unwrap();
    assert_eq!((next.action.as_str(), next.after_seconds), ("poll", 1));
    assert!(first.settings.is_none());
    assert_eq!(first.run.event_type, "action");
    assert_eq!(store(data.path())["client_id"], "abc");

    let second = plugins
        .action("media", &next.action, next.input, Map::new())
        .await
        .unwrap();
    assert!(second.result.done);
    assert_eq!(second.result.message.as_deref(), Some("Signed in as abc"));
    let saved = second.settings.clone().unwrap();
    assert_eq!(saved.secrets["token"], "tok-7-SECRET");
    assert!(!saved.enabled);
    let shown = serde_json::to_string(&second.result).unwrap();
    assert!(!shown.contains("tok-7-SECRET"), "{}", shown);
    assert!(!format!("{:?}", second.run).contains("tok-7-SECRET"));
    // Actions stay out of the run history
    let status = plugins.status();
    assert!(status
        .iter()
        .find(|p| p.name == "media")
        .unwrap()
        .runs
        .is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn choices_counters_failures_and_unknown_actions() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    write(dir.path(), "media", MANIFEST, SCRIPT);
    let plugins = service(
        dir.path(),
        data.path(),
        vec![settings(false, Some("tk_SECRET1"))],
    );

    let load = plugins
        .action("media", "load", json!({}), Map::new())
        .await
        .unwrap();
    let choices = load.result.choices.unwrap();
    assert_eq!(
        choices
            .iter()
            .map(|c| (c.value.as_str(), c.label.as_str()))
            .collect::<Vec<_>>(),
        vec![("TV", "TV"), ("2", "Movies")]
    );
    let metrics = putio_sync_telemetry::global().encode();
    assert!(
        metrics.contains("putio_sync_plugin_counter_total{name=\"loads\",plugin=\"media\"} 1"),
        "{}",
        metrics
    );

    match plugins.action("media", "bad", json!({}), Map::new()).await {
        Err(CallError::Failed(run)) => {
            assert_eq!(run.result, RunResult::Failure);
            let e = run.error.unwrap();
            assert!(e.contains("bad action with ********"), "{}", e);
        }
        other => panic!("{:?}", other.map(|a| a.result)),
    }
    assert!(matches!(
        plugins.action("media", "nope", json!({}), Map::new()).await,
        Err(CallError::NoAction)
    ));
    assert!(matches!(
        plugins.action("other", "load", json!({}), Map::new()).await,
        Err(CallError::NotFound)
    ));
    let mut bad = Map::new();
    bad.insert("paths".to_string(), json!([{"from": "x", "to": "/y"}]));
    match plugins.action("media", "load", json!({}), bad).await {
        Err(CallError::Broken(e)) => assert!(e.contains("rule 1: from"), "{}", e),
        other => panic!("{:?}", other.map(|a| a.result)),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_items_path_mapping_and_no_health() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    write(dir.path(), "media", MANIFEST, SCRIPT);
    write(
        dir.path(),
        "plain",
        "name = \"plain\"\nversion = \"1\"\nevents = [\"*\"]\n",
        "return {on_event = function() end}",
    );
    let plugins = service(
        dir.path(),
        data.path(),
        vec![settings(true, Some("tk_SECRET1"))],
    );
    let items = plugins.health("media").await.unwrap().unwrap();
    assert_eq!(items[0].status, HealthStatus::Ok);
    assert_eq!(items[0].message, "mapped to D:\\Media\\TV\\x");
    assert_eq!(items[1].message, "token ********");
    assert_eq!(plugins.health("plain").await.unwrap(), None);

    let plugins = service(dir.path(), data.path(), vec![settings(true, None)]);
    let items = plugins.health("media").await.unwrap().unwrap();
    assert_eq!(items[1].status, HealthStatus::Fail);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_call_per_plugin_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let slow = r#"
local plugin = {}
function plugin.on_event() end
function plugin.on_action(id)
  local t = putio.time.now()
  while putio.time.now() - t < 0.5 do end
  return {message = "slow"}
end
return plugin
"#;
    write(
        dir.path(),
        "media",
        "name = \"media\"\nversion = \"1\"\nevents = [\"*\"]\n[[actions]]\nid = \"wait\"\nlabel = \"Wait\"\n",
        slow,
    );
    let plugins = std::sync::Arc::new(service(dir.path(), data.path(), Vec::new()));
    let first = {
        let p = plugins.clone();
        tokio::spawn(async move { p.action("media", "wait", json!({}), Map::new()).await })
    };
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(matches!(
        plugins.action("media", "wait", json!({}), Map::new()).await,
        Err(CallError::Busy)
    ));
    assert!(first.await.unwrap().is_ok());
    assert!(plugins
        .action("media", "wait", json!({}), Map::new())
        .await
        .is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ticks_follow_tick_seconds_and_share_the_store_with_actions() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    write(dir.path(), "media", MANIFEST, SCRIPT);
    let plugins = service(
        dir.path(),
        data.path(),
        vec![settings(true, Some("tk_SECRET1"))],
    );
    plugins.start();
    // The running worker builds its state (and learns of on_tick) on its first job
    let until = Instant::now() + Duration::from_secs(10);
    while store(data.path())["events"].is_null() {
        assert!(Instant::now() < until, "event never ran");
        let sample = events::samples()
            .into_iter()
            .find(|p| p.event_type() == EventType::DownloadFinished)
            .unwrap();
        events::emit(sample);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    plugins.tick();
    while store(data.path())["ticks"] != 1 {
        assert!(Instant::now() < until, "first tick never ran");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // Not due yet: the slot's last tick is its start
    plugins.tick_due();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(store(data.path())["ticks"], 1);

    // An action writes through the same store the running worker holds
    plugins
        .action("media", "sign_in", json!({}), Map::new())
        .await
        .unwrap();
    plugins.tick();
    let until = Instant::now() + Duration::from_secs(10);
    while store(data.path())["ticks"] != 2 {
        assert!(Instant::now() < until, "second tick never ran");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let s = store(data.path());
    assert_eq!(s["client_id"], "abc", "{}", s);

    tokio::time::sleep(Duration::from_millis(4600)).await;
    plugins.tick_due();
    let until = Instant::now() + Duration::from_secs(10);
    while store(data.path())["ticks"] != 3 {
        assert!(Instant::now() < until, "due tick never ran");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn counter_names_are_capped() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let script = r#"
local plugin = {}
function plugin.on_event() end
function plugin.on_action(id)
  for i = 1, 16 do putio.metrics.count("c" .. i) end
  putio.metrics.count("c1", 2)
  local ok, err = pcall(putio.metrics.count, "c17")
  return {message = tostring(ok) .. " " .. tostring(err)}
end
return plugin
"#;
    write(
        dir.path(),
        "counted",
        "name = \"counted\"\nversion = \"1\"\nevents = [\"*\"]\n[[actions]]\nid = \"go\"\nlabel = \"Go\"\n",
        script,
    );
    let plugins = service(dir.path(), data.path(), Vec::new());
    let a = plugins
        .action("counted", "go", json!({}), Map::new())
        .await
        .unwrap();
    let m = a.result.message.unwrap();
    assert!(m.starts_with("false") && m.contains("at most 16"), "{}", m);
}
