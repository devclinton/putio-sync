//! `putio-sync doctor` (feature 010): report and fix modes against a mocked Put.io
//!
//! The config has no Put.io base URL, so these call `commands::doctor` with a doctor
//! pointed at mockito; one test runs the binary for the "could not run" exit code.

use assert_cmd::Command;
use mockito::{Server, ServerGuard};
use putio_sync_cli::commands::doctor::{apply_fix, exit_code, report, EXIT_FAILED};
use putio_sync_client::{PutioApiClient, RetryPolicy};
use putio_sync_core::cleanup::EmptyFolderCleanup;
use putio_sync_core::config::Config;
use putio_sync_db::Database;
use putio_sync_doctor::{Doctor, Report, Status};
use std::io::Cursor;
use std::sync::Arc;

struct Env {
    server: ServerGuard,
    _dir: tempfile::TempDir,
    db: Arc<Database>,
    doctor: Doctor,
}

async fn env() -> Env {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/account/info")
        .with_body(
            r#"{"status":"OK","info":{"user_id":1,"username":"u","mail":"u@example.com",
            "account_active":true,"disk":{"size":1000,"used":10,"avail":990}}}"#,
        )
        .create_async()
        .await;
    server
        .mock("GET", "/transfers/list")
        .with_body(r#"{"status":"OK","transfers":[]}"#)
        .create_async()
        .await;
    server
        .mock("GET", "/files/list?parent_id=0")
        .with_body(
            r#"{"status":"OK","files":[],"parent":{"id":0,"name":"root","size":0,
            "file_type":"FOLDER","parent_id":null,"is_folder":true}}"#,
        )
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.download.target_directory = dir.path().join("downloads");
    std::fs::create_dir_all(&config.download.target_directory).unwrap();
    let client = Arc::new(
        PutioApiClient::with_base_url("t".into(), server.url()).with_retry_policy(RetryPolicy {
            max_attempts: 1,
            ..RetryPolicy::default()
        }),
    );
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let cleanup = Arc::new(EmptyFolderCleanup::from_config(client.clone(), &config));
    let doctor = Doctor::new(config, client, Some(db.clone()), cleanup);
    Env {
        server,
        _dir: dir,
        db,
        doctor,
    }
}

/// Queue row 77 points at a file Put.io no longer has
async fn seed_dead_row(env: &mut Env) {
    env.server
        .mock("GET", "/files/77")
        .with_status(404)
        .with_body(r#"{"status":"ERROR","error_type":"NotFound","status_code":404}"#)
        .create_async()
        .await;
    env.db
        .queue()
        .add(
            77,
            "gone.mkv",
            10,
            "u",
            "Movies",
            0,
            putio_sync_db::models::QueueStatus::Pending,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn json_report_parses_and_sets_the_exit_code() {
    let mut env = env().await;
    let mut out = Vec::new();
    let code = report(&env.doctor, true, &mut out).await.unwrap();
    let parsed: Report = serde_json::from_slice(&out).unwrap();
    assert_eq!(code, exit_code(parsed.overall));
    // The disk-space check reads this machine's real disk, so leave it out
    assert!(parsed
        .checks
        .iter()
        .filter(|c| c.id.as_str() != "disk_space")
        .all(|c| c.status != Status::Problem));

    // An errored transfer is a problem
    env.server
        .mock("GET", "/transfers/list")
        .with_body(
            r#"{"status":"OK","transfers":[{"id":5,"name":"Broken","status":"ERROR",
            "created_at":"2026-01-01T00:00:00","save_parent_id":0,"is_private":false}]}"#,
        )
        .create_async()
        .await;
    let mut out = Vec::new();
    let code = report(&env.doctor, true, &mut out).await.unwrap();
    let parsed: Report = serde_json::from_slice(&out).unwrap();
    assert_eq!(parsed.overall, Status::Problem);
    assert_eq!(code, 2);
}

#[tokio::test]
async fn text_report_groups_checks_and_lists_fixes() {
    let mut env = env().await;
    seed_dead_row(&mut env).await;
    let mut out = Vec::new();
    report(&env.doctor, false, &mut out).await.unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.starts_with("Put.io\n"), "{text}");
    assert!(text.contains("Sync\n"));
    assert!(
        text.contains("  ! Queue entries for deleted files 1 queue entry"),
        "{text}"
    );
    assert!(text.contains("[fix: dead_queue_rows.remove]"));
    assert!(text.contains("Summary: "));
    assert!(text.contains("Fixes available: dead_queue_rows.remove"));
}

#[tokio::test]
async fn dry_run_and_no_change_nothing() {
    let mut env = env().await;
    seed_dead_row(&mut env).await;

    let mut out = Vec::new();
    let code = apply_fix(
        &env.doctor,
        "dead_queue_rows.remove",
        false,
        true,
        false,
        &mut Cursor::new(""),
        &mut out,
    )
    .await
    .unwrap();
    assert_eq!(code, 0);
    assert!(String::from_utf8(out).unwrap().contains("Dry run"));
    assert!(env.db.queue().get_by_file_id(77).await.unwrap().is_some());

    let mut out = Vec::new();
    let code = apply_fix(
        &env.doctor,
        "dead_queue_rows.remove",
        false,
        false,
        false,
        &mut Cursor::new("n\n"),
        &mut out,
    )
    .await
    .unwrap();
    assert_eq!(code, 0);
    assert!(String::from_utf8(out).unwrap().contains("Cancelled"));
    assert!(env.db.queue().get_by_file_id(77).await.unwrap().is_some());
}

#[tokio::test]
async fn yes_removes_the_previewed_rows() {
    let mut env = env().await;
    seed_dead_row(&mut env).await;
    let mut out = Vec::new();
    let code = apply_fix(
        &env.doctor,
        "dead_queue_rows.remove",
        false,
        false,
        true,
        &mut Cursor::new(""),
        &mut out,
    )
    .await
    .unwrap();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Done: 1, skipped: 0, failed: 0"), "{text}");
    assert!(env.db.queue().get_by_file_id(77).await.unwrap().is_none());
}

#[tokio::test]
async fn unknown_fixes_and_unconfirmable_json_exit_3() {
    let env = env().await;
    let mut out = Vec::new();
    let code = apply_fix(
        &env.doctor,
        "nope",
        false,
        false,
        false,
        &mut Cursor::new(""),
        &mut out,
    )
    .await
    .unwrap();
    assert_eq!(code, EXIT_FAILED);
    let code = apply_fix(
        &env.doctor,
        "orphan_partials.delete",
        true,
        false,
        false,
        &mut Cursor::new(""),
        &mut out,
    )
    .await
    .unwrap();
    assert_eq!(code, EXIT_FAILED);
    assert!(out.is_empty());
}

#[test]
fn an_unreadable_config_exits_3() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "this is = = not toml").unwrap();
    Command::cargo_bin("putio-sync")
        .unwrap()
        .args(["--config", config.to_str().unwrap(), "doctor", "--json"])
        .assert()
        .code(EXIT_FAILED)
        .stdout("");
}
