//! WebAssembly plugins: the same limits, failures and host calls as Lua, from small modules
//! written in the text format.

use std::path::Path;
use std::time::{Duration, Instant};

use putio_sync_core::events::{self, EventType, Payload};
use putio_sync_core::plugins::PluginSettings;
use putio_sync_plugins::{
    PluginRun, PluginState, PluginStatus, PluginsOptions, PluginsService, RunResult,
};

/// The bus is global, so these tests take turns
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A module with a bump allocator, an empty `init` and an `on_event` whose body is `body`.
/// `imports` and `exports` add to the module.
fn module(imports: &str, body: &str, exports: &str) -> Vec<u8> {
    let text = format!(
        r#"(module
  (import "putio" "call" (func $call (param i32 i32) (result i32)))
  (import "putio" "reply" (func $reply (param i32)))
  (import "putio" "fail" (func $fail (param i32 i32)))
  {imports}
  (memory (export "memory") 1)
  (global $next (mut i32) (i32.const 4096))
  (data (i32.const 16) "{{\"fn\":\"log.info\",\"args\":[\"hello from wasm\"]}}")
  (data (i32.const 128) "not today")
  (func (export "putio_alloc") (param $n i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $next))
    (global.set $next (i32.add (global.get $next) (local.get $n)))
    (local.get $at))
  (func (export "init") (param i32 i32) (result i64) (i64.const 0))
  (func (export "on_event") (param i32 i32) (result i64)
    (local $len i32)
    {body}
    (i64.const 0))
  {exports})"#
    );
    wat::parse_str(text).unwrap()
}

fn plugin(dir: &Path, name: &str, wasm: &[u8]) {
    let path = dir.join(name);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("plugin.toml"),
        format!(
            "name = \"{name}\"\nversion = \"1\"\nruntime = \"wasm\"\nentry = \"plugin.wasm\"\nevents = [\"*\"]\n"
        ),
    )
    .unwrap();
    std::fs::write(path.join("plugin.wasm"), wasm).unwrap();
}

fn service(dir: &Path, names: &[&str]) -> PluginsService {
    let plugins = PluginsService::new(PluginsOptions {
        dir: Some(dir.to_path_buf()),
        settings: names
            .iter()
            .map(|n| PluginSettings {
                name: n.to_string(),
                enabled: true,
                timeout_secs: 1,
                memory_mb: 32,
                ..PluginSettings::default()
            })
            .collect(),
        ..PluginsOptions::default()
    });
    plugins.start();
    plugins
}

fn status(plugins: &PluginsService, name: &str) -> PluginStatus {
    plugins
        .status()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap()
}

fn runs(plugins: &PluginsService, name: &str) -> Vec<PluginRun> {
    status(plugins, name).runs
}

async fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn emit(t: EventType) {
    let payload: Payload = events::samples()
        .into_iter()
        .find(|p| p.event_type() == t)
        .unwrap();
    events::emit(payload);
}

/// The second run, after `init`: the event's. A plugin is built on its first event.
async fn event_run(plugins: &PluginsService, name: &str) -> PluginRun {
    emit(EventType::DownloadFailed);
    wait_for("the event run", || runs(plugins, name).len() >= 2).await;
    runs(plugins, name)[1].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_loop_times_out_and_the_module_is_rebuilt() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    plugin(dir.path(), "looper", &module("", "(loop $l (br $l))", ""));
    let plugins = service(dir.path(), &["looper"]);
    let run = event_run(&plugins, "looper").await;
    assert_eq!(run.result, RunResult::Timeout);
    assert_eq!(run.error.as_deref(), Some("time limit reached (1 s)"));
    assert!(
        run.duration_ms >= 1000 && run.duration_ms < 2000,
        "{}",
        run.duration_ms
    );
    // Rebuilt for the next event: init runs again
    emit(EventType::DownloadFinished);
    wait_for("the rebuild", || runs(&plugins, "looper").len() >= 4).await;
    let r = runs(&plugins, "looper");
    assert_eq!(
        (r[2].event_type.as_str(), r[2].result),
        ("init", RunResult::Success)
    );
    assert_eq!(r[3].result, RunResult::Timeout);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn growing_past_the_memory_limit_fails() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let grow =
        "(loop $l (br_if $l (i32.ne (memory.grow (i32.const 16)) (i32.const -1)))) unreachable";
    plugin(dir.path(), "hog", &module("", grow, ""));
    let plugins = service(dir.path(), &["hog"]);
    let run = event_run(&plugins, "hog").await;
    assert_eq!(run.result, RunResult::Failure);
    assert_eq!(run.error.as_deref(), Some("memory limit reached (32 MiB)"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fail_and_proc_exit_end_the_call_with_a_message() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    plugin(
        dir.path(),
        "failer",
        &module("", "(call $fail (i32.const 128) (i32.const 9))", ""),
    );
    plugin(
        dir.path(),
        "exiter",
        &module(
            r#"(import "wasi_snapshot_preview1" "proc_exit" (func $exit (param i32)))"#,
            "(call $exit (i32.const 3))",
            "",
        ),
    );
    let plugins = service(dir.path(), &["failer", "exiter"]);
    let run = event_run(&plugins, "failer").await;
    assert_eq!(run.result, RunResult::Failure);
    assert_eq!(run.error.as_deref(), Some("not today"));
    wait_for("exiter", || runs(&plugins, "exiter").len() >= 2).await;
    let run = runs(&plugins, "exiter")[1].clone();
    assert_eq!(run.result, RunResult::Failure);
    assert_eq!(run.error.as_deref(), Some("exited with code 3"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_calls_reply_through_putio_reply() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    // log.info, then copy the reply ({"ok":null}, 11 bytes) and fail unless it fits
    let body = r#"
    (local.set $len (call $call (i32.const 16) (i32.const 44)))
    (call $reply (i32.const 1024))
    (if (i32.ne (local.get $len) (i32.const 11)) (then unreachable))
    (if (i32.ne (i32.load8_u (i32.const 1024)) (i32.const 123)) (then unreachable))"#;
    plugin(dir.path(), "caller", &module("", body, ""));
    let plugins = service(dir.path(), &["caller"]);
    // A test run keeps its log lines
    let run = plugins.test("caller").await.unwrap();
    assert_eq!(run.result, RunResult::Success, "{:?}", run.error);
    assert!(
        run.logs
            .iter()
            .any(|l| l.message.contains("hello from wasm")),
        "{:?}",
        run.logs
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn modules_the_host_cannot_run_are_broken() {
    let _turn = SERIAL.lock().await;
    let dir = tempfile::tempdir().unwrap();
    plugin(
        dir.path(),
        "importer",
        &module(r#"(import "env" "open" (func (param i32)))"#, "", ""),
    );
    plugin(
        dir.path(),
        "command",
        &module("", "", r#"(func (export "_start"))"#),
    );
    plugin(dir.path(), "garbage", b"\0asm\x01\0\0\0garbage");
    let plugins = service(dir.path(), &["importer", "command", "garbage"]);
    emit(EventType::DownloadFailed);
    for (name, want) in [
        ("importer", "env.open"),
        ("command", "_start"),
        ("garbage", "plugin.wasm"),
    ] {
        wait_for(name, || status(&plugins, name).state == PluginState::Broken).await;
        let error = status(&plugins, name).error.unwrap_or_default();
        assert!(error.contains(want), "{name}: {error}");
    }
}
