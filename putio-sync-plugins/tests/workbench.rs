//! The plugin workbench (feature 055) end to end: dry and real runs, the store copy,
//! error lines, unsaved text, saves, starters and a WebAssembly plugin.

use std::path::{Path, PathBuf};

use putio_sync_core::plugins::PluginSettings;
use putio_sync_plugins::source::{SaveRequest, SourceError, STARTERS};
use putio_sync_plugins::workbench::{Call, Reply, RunRequest, WorkbenchError};
use putio_sync_plugins::{PluginsOptions, PluginsService, RunResult};
use serde_json::{json, Value};

const SECRET: &str = "s3cret-token-value";

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins")
}

fn settings(name: &str, config: Value, secrets: &[(&str, &str)]) -> PluginSettings {
    PluginSettings {
        name: name.to_string(),
        enabled: true,
        config: serde_json::from_value(config).unwrap(),
        secrets: secrets
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        ..PluginSettings::default()
    }
}

fn service(dir: &Path, settings: Vec<PluginSettings>, data: Option<&Path>) -> PluginsService {
    let plugins = PluginsService::new(PluginsOptions {
        dir: Some(dir.to_path_buf()),
        data_dir: data.map(Path::to_path_buf),
        download_dir: Some(dir.to_path_buf()),
        ..PluginsOptions::default()
    });
    let checked = plugins.check(&settings).unwrap();
    plugins.configure(checked);
    plugins
}

fn request(name: &str, call: Call) -> RunRequest {
    RunRequest {
        name: name.to_string(),
        call,
        real: false,
        reply: Reply::default(),
        manifest: None,
        script: None,
    }
}

fn finished() -> Call {
    Call::Event {
        event_type: "download.finished".to_string(),
        data: Value::Null,
    }
}

fn plugin(dir: &Path, name: &str, manifest_extra: &str, script: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("plugin.toml"),
        format!("name = \"{name}\"\nversion = \"1\"\nevents = [\"*\"]\n{manifest_extra}"),
    )
    .unwrap();
    std::fs::write(path.join("main.lua"), script).unwrap();
}

#[test]
fn the_examples_are_the_starters() {
    for (id, _, _, manifest, script) in STARTERS {
        let folder = examples().join(id);
        let read = |f: &str| std::fs::read_to_string(folder.join(f)).unwrap();
        assert_eq!(read("plugin.toml"), *manifest, "{id}/plugin.toml");
        assert_eq!(read("main.lua"), *script, "{id}/main.lua");
        assert!(folder.join("README.md").is_file(), "{id}/README.md");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dry_run_records_and_sends_nothing() {
    let mut server = mockito::Server::new_async().await;
    let hook = server.mock("POST", "/hook").expect(0).create_async().await;
    let url = format!("{}/hook?key={SECRET}", server.url());
    let plugins = service(
        &examples(),
        vec![settings("webhook-notifier", json!({}), &[("url", &url)])],
        None,
    );
    let run = plugins
        .workbench_run(request("webhook-notifier", finished()))
        .await
        .unwrap();
    assert_eq!(run.run.result, RunResult::Success, "{:?}", run.run.error);
    assert_eq!(run.requests.len(), 1);
    let sent = &run.requests[0];
    assert!(!sent.sent);
    assert_eq!(sent.status, Some(200));
    assert!(!sent.url.contains(SECRET), "{}", sent.url);
    assert!(sent.body.contains("download.finished"));
    assert!(run.run.logs.iter().any(|l| l.message.contains("posting")));
    assert!(run.memory_bytes.unwrap() > 0);
    hook.assert_async().await;

    // A failing reply fails the plugin, as a real one would
    let mut failing = request("webhook-notifier", finished());
    failing.reply = Reply {
        status: 500,
        body: String::new(),
    };
    let run = plugins.workbench_run(failing).await.unwrap();
    assert_eq!(run.run.result, RunResult::Failure);
    assert!(run.run.error.unwrap().contains("HTTP 500"));

    // Runs are kept, newest first, and run again
    let kept = plugins.workbench_runs();
    assert_eq!(kept.len(), 2);
    assert!(kept[0].id > kept[1].id);
    let again = plugins.workbench_rerun(kept[1].id).await.unwrap();
    assert_eq!(again.run.result, RunResult::Success);
    assert!(matches!(
        plugins.workbench_rerun(999).await,
        Err(WorkbenchError::NotFound)
    ));
    // They are not the plugin's runs
    let status = plugins.status();
    let s = status
        .iter()
        .find(|s| s.name == "webhook-notifier")
        .unwrap();
    assert!(s.runs.is_empty(), "{:?}", s.runs);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_real_run_reaches_the_approved_host() {
    let mut server = mockito::Server::new_async().await;
    let hook = server
        .mock("POST", "/hook")
        .with_status(204)
        .expect(1)
        .create_async()
        .await;
    let url = format!("{}/hook", server.url());
    let plugins = service(
        &examples(),
        vec![settings("webhook-notifier", json!({}), &[("url", &url)])],
        None,
    );
    let mut real = request("webhook-notifier", finished());
    real.real = true;
    let run = plugins.workbench_run(real).await.unwrap();
    assert_eq!(run.run.result, RunResult::Success, "{:?}", run.run.error);
    let sent = &run.requests[0];
    assert!(sent.sent);
    assert_eq!(sent.status, Some(204));
    assert!(sent.duration_ms.is_some());
    hook.assert_async().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dry_run_changes_a_copy_of_the_store() {
    let data = tempfile::tempdir().unwrap();
    let plugins = service(
        &examples(),
        vec![settings("store-counter", json!({}), &[])],
        Some(data.path()),
    );
    let run = plugins
        .workbench_run(request("store-counter", finished()))
        .await
        .unwrap();
    assert_eq!(run.run.result, RunResult::Success, "{:?}", run.run.error);
    assert_eq!(run.store_before, Some(json!({})));
    let after = run.store_after.unwrap();
    assert_eq!(after["per_day"].as_object().unwrap().len(), 1);
    assert!(!data.path().join("store-counter.json").exists());

    let mut real = request("store-counter", finished());
    real.real = true;
    plugins.workbench_run(real).await.unwrap();
    assert!(data.path().join("store-counter.json").exists());
    let health = plugins
        .workbench_run(request("store-counter", Call::Health))
        .await
        .unwrap();
    assert_eq!(health.store_before, health.store_after);
    assert_eq!(health.value.unwrap()[0]["message"], "1 finished today");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn errors_name_the_line_and_unsaved_text_runs() {
    let dir = tempfile::tempdir().unwrap();
    plugin(
        dir.path(),
        "broken",
        "",
        "local p = {}\nfunction p.on_event(e)\n  error(\"boom\")\nend\nreturn p\n",
    );
    let plugins = service(dir.path(), vec![], None);
    let run = plugins
        .workbench_run(request("broken", finished()))
        .await
        .unwrap();
    assert_eq!(run.run.result, RunResult::Failure);
    let error = run.run.error.unwrap();
    assert!(error.starts_with("main.lua:3:"), "{error}");

    let mut unsaved = request("broken", finished());
    unsaved.manifest =
        Some(std::fs::read_to_string(dir.path().join("broken/plugin.toml")).unwrap());
    unsaved.script = Some(
        "local p = {}\nfunction p.on_event(e)\n  putio.log.info(\"fixed\")\nend\nreturn p\n"
            .to_string(),
    );
    let run = plugins.workbench_run(unsaved.clone()).await.unwrap();
    assert!(run.unsaved);
    assert_eq!(run.run.result, RunResult::Success, "{:?}", run.run.error);
    assert!(run.run.logs.iter().any(|l| l.message == "fixed"));

    unsaved.script = Some("return {\n".to_string());
    let run = plugins.workbench_run(unsaved).await.unwrap();
    assert_eq!(run.problems[0].line, Some(2), "{:?}", run.problems);
    assert_eq!(run.run.result, RunResult::Failure);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_tick_poller_ticks_against_the_reply() {
    let data = tempfile::tempdir().unwrap();
    let plugins = service(
        &examples(),
        vec![settings(
            "tick-poller",
            json!({"url": "http://192.0.2.1/health"}),
            &[],
        )],
        Some(data.path()),
    );
    let mut tick = request("tick-poller", Call::Tick);
    tick.reply.status = 503;
    let run = plugins.workbench_run(tick).await.unwrap();
    assert_eq!(run.run.result, RunResult::Success, "{:?}", run.run.error);
    assert_eq!(run.requests[0].url, "http://192.0.2.1/health");
    assert_eq!(run.store_after, Some(json!({"status": 503})));
    assert_eq!(run.run.logs[0].message, "status nil -> 503");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_run_stops_at_the_time_limit() {
    let dir = tempfile::tempdir().unwrap();
    plugin(
        dir.path(),
        "spin",
        "",
        "return {on_event = function() while true do end end}\n",
    );
    let mut spin = settings("spin", json!({}), &[]);
    spin.timeout_secs = 1;
    let plugins = service(dir.path(), vec![spin], None);
    let run = plugins
        .workbench_run(request("spin", finished()))
        .await
        .unwrap();
    assert_eq!(run.run.result, RunResult::Timeout, "{:?}", run.run.error);
    // The plugin can run again afterwards
    let again = plugins
        .workbench_run(request("spin", Call::Health))
        .await
        .unwrap();
    assert!(!again.defined);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dry_write_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    plugin(
        dir.path(),
        "writer",
        "[permissions.files]\nwrite = [\"{download_dir}\"]\n",
        "return {on_event = function() assert(putio.fs.write(putio.download_dir .. \"/note.txt\", \"hi\")) end}\n",
    );
    let plugins = service(dir.path(), vec![settings("writer", json!({}), &[])], None);
    let run = plugins
        .workbench_run(request("writer", finished()))
        .await
        .unwrap();
    assert_eq!(run.run.result, RunResult::Success, "{:?}", run.run.error);
    assert_eq!(run.writes.len(), 1);
    assert_eq!(run.writes[0].bytes, 2);
    assert!(!dir.path().join("note.txt").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_save_and_stale_saves() {
    let dir = tempfile::tempdir().unwrap();
    let plugins = service(dir.path(), vec![], None);
    plugins.start();
    plugins
        .workbench_new("webhook-notifier", "my-hook")
        .unwrap();
    assert!(matches!(
        plugins.workbench_new("webhook-notifier", "my-hook"),
        Err(SourceError::Taken(_))
    ));
    assert!(matches!(
        plugins.workbench_new("webhook-notifier", "ntfy"),
        Err(SourceError::Taken(_))
    ));
    assert!(matches!(
        plugins.workbench_new("nope", "x"),
        Err(SourceError::NotFound)
    ));
    assert!(matches!(
        plugins.workbench_new("webhook-notifier", "../x"),
        Err(SourceError::BadName(_))
    ));
    let source = plugins.workbench_source("my-hook", true).unwrap();
    assert!(source.editable);
    assert_eq!(source.files.len(), 2);
    let save = SaveRequest {
        manifest: source.files[0].text.clone(),
        script: source.files[1].text.replace("posting", "sending"),
        versions: source
            .files
            .iter()
            .map(|f| (f.path.clone(), f.version.clone()))
            .collect(),
        force: false,
    };
    assert!(plugins.workbench_save("my-hook", &save).unwrap().is_empty());
    let folder = dir.path().join("my-hook");
    assert!(folder.join("main.lua.bak").is_file());
    assert!(std::fs::read_to_string(folder.join("main.lua"))
        .unwrap()
        .contains("sending"));
    // The versions read before the save are stale now
    assert!(matches!(
        plugins.workbench_save("my-hook", &save),
        Err(SourceError::Stale(_))
    ));
    let forced = SaveRequest {
        script: "return {\n".to_string(),
        force: true,
        ..save
    };
    let problems = plugins.workbench_save("my-hook", &forced).unwrap();
    assert_eq!(problems[0].line, Some(2));

    // Bundled plugins are read-only until copied
    let ntfy = plugins.workbench_source("ntfy", true).unwrap();
    assert!(!ntfy.editable);
    plugins.workbench_copy("ntfy").unwrap();
    let ntfy = plugins.workbench_source("ntfy", true).unwrap();
    assert_eq!(ntfy.source, "folder");
    assert!(ntfy.editable);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_webassembly_hook_runs_the_same_way() {
    let plugins = service(&examples(), vec![], None);
    let source = plugins.workbench_source("rust-clean-names", true).unwrap();
    assert_eq!(source.runtime, "wasm");
    assert!(!source.editable);
    assert_eq!(source.files.len(), 1);
    let call = Call::Hook {
        hook: "rename".to_string(),
        file: json!({
            "id": 1, "name": "Show.S01E02.1080p.WEB.x264-GRP.mkv", "size": 1,
            "extension": "mkv", "folder": "", "decision": "download", "reason": ""
        }),
    };
    let run = plugins
        .workbench_run(request("rust-clean-names", call))
        .await
        .unwrap();
    assert_eq!(run.run.result, RunResult::Success, "{:?}", run.run.error);
    assert_eq!(run.answer.as_deref(), Some("Show S01E02.mkv"));
    assert!(run.memory_bytes.unwrap() > 0);
}
