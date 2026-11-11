//! Sync hooks through the plugins service (feature 028): the order plugins are asked in,
//! falling back on errors, slow plugins and bad answers, the pause after failures, and
//! which manifests load.

use std::path::Path;
use std::time::{Duration, Instant};

use putio_sync_core::plugins::PluginSettings;
use putio_sync_core::sync_hooks::{FilterAnswer, Hook, HookFile, SyncHooks};
use putio_sync_plugins::hooks::MAX_FAILURES;
use putio_sync_plugins::{PluginState, PluginsOptions, PluginsService, RunResult};

fn filter_plugin(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("plugin.toml"),
        format!("name = \"{name}\"\nversion = \"1.0.0\"\nhooks = [\"filter\"]\n"),
    )
    .unwrap();
    std::fs::write(
        path.join("main.lua"),
        format!("return {{ filter = function(file) {body} end }}"),
    )
    .unwrap();
}

fn enabled(name: &str, limit_ms: u64) -> PluginSettings {
    PluginSettings {
        name: name.to_string(),
        enabled: true,
        hook_timeout_ms: limit_ms,
        ..PluginSettings::default()
    }
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

fn file() -> HookFile {
    HookFile {
        id: 7,
        name: "sample.mkv".to_string(),
        size: 1024,
        extension: "mkv".to_string(),
        folder: String::new(),
        decision: "download".to_string(),
        reason: String::new(),
    }
}

async fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_first_answer_in_name_order_wins() {
    let dir = tempfile::tempdir().unwrap();
    filter_plugin(dir.path(), "c-download", "return \"download\"");
    filter_plugin(dir.path(), "a-nothing", "return nil");
    filter_plugin(
        dir.path(),
        "b-skip",
        "return file.size < 2048 and \"skip\" or nil",
    );
    let plugins = service(
        dir.path(),
        vec![
            enabled("a-nothing", 2000),
            enabled("b-skip", 2000),
            enabled("c-download", 2000),
        ],
    );
    wait_for("filter", || plugins.provides(Hook::Filter)).await;
    assert_eq!(
        plugins.filter(&file()).await,
        Some((FilterAnswer::Skip, "b-skip".to_string()))
    );
    let mut big = file();
    big.size = 1 << 30;
    assert_eq!(
        plugins.filter(&big).await,
        Some((FilterAnswer::Download, "c-download".to_string()))
    );
    // Nothing provides the others
    assert!(!plugins.provides(Hook::Rename));
    assert_eq!(plugins.rename(&file()).await, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn errors_slow_plugins_and_bad_answers_fall_back() {
    let dir = tempfile::tempdir().unwrap();
    filter_plugin(dir.path(), "a-error", "error(\"broken on purpose\")");
    filter_plugin(dir.path(), "b-bad", "return \"maybe\"");
    filter_plugin(dir.path(), "c-slow", "while true do end");
    let plugins = service(
        dir.path(),
        vec![
            enabled("a-error", 2000),
            enabled("b-bad", 2000),
            enabled("c-slow", 100),
        ],
    );
    wait_for("filter", || plugins.provides(Hook::Filter)).await;
    let started = Instant::now();
    assert_eq!(plugins.filter(&file()).await, None);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );

    let status = plugins.status();
    let runs = |name: &str| status.iter().find(|s| s.name == name).unwrap().runs.clone();
    for name in ["a-error", "b-bad", "c-slow"] {
        let runs = runs(name);
        assert_eq!(runs.len(), 1, "{name}: {runs:?}");
        assert_eq!(runs[0].event_type, "filter");
        assert_ne!(runs[0].result, RunResult::Success);
    }
    assert!(runs("a-error")[0]
        .error
        .as_deref()
        .unwrap()
        .contains("broken on purpose"));
    assert!(runs("b-bad")[0].error.as_deref().unwrap().contains("maybe"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failures_in_a_row_pause_the_hooks() {
    let dir = tempfile::tempdir().unwrap();
    filter_plugin(
        dir.path(),
        "flaky",
        "if file.id == 0 then return \"skip\" end error(\"no\")",
    );
    let plugins = service(dir.path(), vec![enabled("flaky", 2000)]);
    wait_for("filter", || plugins.provides(Hook::Filter)).await;

    // A success resets the count
    for _ in 1..MAX_FAILURES {
        assert_eq!(plugins.filter(&file()).await, None);
    }
    let mut ok = file();
    ok.id = 0;
    assert!(plugins.filter(&ok).await.is_some());
    for _ in 1..MAX_FAILURES {
        assert_eq!(plugins.filter(&file()).await, None);
    }
    assert!(plugins.provides(Hook::Filter));

    assert_eq!(plugins.filter(&file()).await, None);
    assert!(!plugins.provides(Hook::Filter), "paused");
    assert_eq!(plugins.filter(&ok).await, None);

    // A reload starts it afresh
    plugins.configure(vec![enabled("flaky", 2000)]);
    plugins.reload();
    wait_for("filter again", || plugins.provides(Hook::Filter)).await;
    assert!(plugins.filter(&ok).await.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manifests_with_hooks_load_or_break() {
    let dir = tempfile::tempdir().unwrap();
    filter_plugin(dir.path(), "only-hooks", "return nil");
    let path = dir.path().join("no-rename");
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("plugin.toml"),
        "name = \"no-rename\"\nversion = \"1.0.0\"\nhooks = [\"filter\", \"rename\"]\n",
    )
    .unwrap();
    std::fs::write(path.join("main.lua"), "return { filter = function() end }").unwrap();
    let plugins = service(
        dir.path(),
        vec![enabled("only-hooks", 2000), enabled("no-rename", 2000)],
    );
    // Workers build on their first job
    wait_for("filter", || plugins.provides(Hook::Filter)).await;
    assert_eq!(plugins.filter(&file()).await, None);
    wait_for("the broken plugin", || {
        plugins
            .status()
            .iter()
            .any(|s| s.name == "no-rename" && s.state == PluginState::Broken)
    })
    .await;
    let status = plugins.status();
    let broken = status.iter().find(|s| s.name == "no-rename").unwrap();
    assert!(
        broken
            .error
            .as_deref()
            .unwrap()
            .contains("no rename function"),
        "{:?}",
        broken.error
    );
    let fine = status.iter().find(|s| s.name == "only-hooks").unwrap();
    assert_eq!(fine.state, PluginState::Enabled, "{:?}", fine.error);
    assert_eq!(fine.hooks, vec!["filter".to_string()]);
    assert!(fine.events.is_empty());
}
