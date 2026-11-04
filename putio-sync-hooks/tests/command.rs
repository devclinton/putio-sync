//! Command hooks (feature 027, T006).
#![cfg(unix)]

mod common;

use std::time::{Duration, Instant};

use common::*;
use putio_sync_hooks::{command, RunResult};

#[tokio::test]
async fn stdin_and_env_reach_the_script_and_tokens_do_not() {
    std::env::set_var("PUTIO_SYNC_TOKEN", "secret-token-123");
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("event.json");
    let env = dir.path().join("env");
    let program = script(
        dir.path(),
        "hook.sh",
        &format!(
            "cat > {}\nenv > {}\necho done",
            out.display(),
            env.display()
        ),
    );
    let mut hook = command_hook("t", &program);
    hook.env.insert("LIBRARY".to_string(), "/data".to_string());
    let event = test_event();

    let run = command::run(&hook, &event).await;

    assert_eq!(run.result, RunResult::Success, "{:?}", run);
    assert_eq!(run.exit_code, Some(0));
    assert_eq!(run.stdout, "done\n");
    let sent: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(sent["id"], event.id.as_str());
    assert_eq!(sent["type"], "hook.test");
    let env = std::fs::read_to_string(&env).unwrap();
    assert!(env.contains("PUTIO_SYNC_EVENT_TYPE=hook.test"));
    assert!(env.contains(&format!("PUTIO_SYNC_EVENT_ID={}", event.id)));
    assert!(env.contains("PUTIO_SYNC_HOOK_NAME=t"));
    assert!(env.contains("LIBRARY=/data"));
    assert!(env.contains("PATH="));
    assert!(!env.contains("secret-token-123"));
    assert!(!env.contains("PUTIO_SYNC_TOKEN"));
}

#[tokio::test]
async fn a_non_zero_exit_is_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let program = script(dir.path(), "fail.sh", "echo oops >&2\nexit 3");
    let run = command::run(&command_hook("t", &program), &test_event()).await;
    assert_eq!(run.result, RunResult::Failure);
    assert_eq!(run.exit_code, Some(3));
    assert_eq!(run.error.as_deref(), Some("exited with code 3"));
    assert_eq!(run.stderr, "oops\n");
}

#[tokio::test]
async fn a_hanging_script_times_out_and_is_killed() {
    let dir = tempfile::tempdir().unwrap();
    let program = script(dir.path(), "hang.sh", "sleep 30");
    let mut hook = command_hook("t", &program);
    hook.timeout_secs = 1;
    let start = Instant::now();
    let run = command::run(&hook, &test_event()).await;
    assert_eq!(run.result, RunResult::Timeout);
    assert_eq!(run.error.as_deref(), Some("timed out after 1 s"));
    // The sleep was killed with its group, so the pipes closed straight away.
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "{:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn a_missing_program_says_so() {
    let hook = command_hook("t", std::path::Path::new("/no/such/hook"));
    let run = command::run(&hook, &test_event()).await;
    assert_eq!(run.result, RunResult::Failure);
    assert_eq!(
        run.error.as_deref(),
        Some("program not found: /no/such/hook")
    );
}

#[tokio::test]
async fn long_output_is_cut() {
    let dir = tempfile::tempdir().unwrap();
    let program = script(
        dir.path(),
        "loud.sh",
        "head -c 10240 /dev/zero | tr '\\0' 'x'",
    );
    let run = command::run(&command_hook("t", &program), &test_event()).await;
    assert_eq!(run.result, RunResult::Success);
    assert!(
        run.stdout.ends_with("… (6144 bytes cut)"),
        "{}",
        run.stdout.len()
    );
}

#[tokio::test]
async fn a_script_that_ignores_stdin_still_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let program = script(dir.path(), "quiet.sh", "exit 0");
    let run = command::run(&command_hook("t", &program), &test_event()).await;
    assert_eq!(run.result, RunResult::Success);
}
