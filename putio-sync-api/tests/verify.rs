//! Download verification routes (feature 034): the run's status, starting, steering and
//! the problems found.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app_with, test_app_without_db, TestApp};
use putio_sync_db::models::DownloadStatus;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

async fn call(
    app: &TestApp,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let response = app.req(method, path, Some(&app.token), body).await;
    let status = response.status();
    (status, body_json(response).await)
}

fn crc(data: &[u8]) -> String {
    let mut digest = putio_sync_download::Crc32::new();
    digest.update(data);
    digest.hex()
}

/// An app reading at 1 MB/s with `name` downloaded (`bytes` on disk) and listed on Put.io
/// with the CRC32 of `listed`
async fn app_with_file(
    name: &str,
    bytes: &[u8],
    listed: &[u8],
) -> (TestApp, String, mockito::Mock) {
    let mut app = test_app_with(|c| c.download.verify_read_limit_mb_per_sec = 1).await;
    let path = app
        .state
        .config
        .read()
        .await
        .download
        .target_directory
        .join(name);
    std::fs::write(&path, bytes).unwrap();
    let path = path.to_string_lossy().into_owned();
    app.db()
        .downloads()
        .insert(
            1,
            name,
            listed.len() as i64,
            &path,
            DownloadStatus::Completed,
        )
        .await
        .unwrap();
    let file = json!({"id": 1, "name": name, "size": listed.len(), "file_type": "VIDEO",
        "parent_id": 0, "is_folder": false, "is_shared": false, "crc32": crc(listed)});
    let listing = app
        .mock_files_list(
            0,
            json!({"files": [file], "status": "OK", "parent": {"id": 0, "name": "Root",
                "size": 0, "file_type": "FOLDER", "parent_id": 0, "is_folder": true,
                "is_shared": false}}),
        )
        .await;
    (app, path, listing)
}

async fn until_idle(app: &TestApp) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (_, body) = call(app, Method::GET, "/verify", None).await;
        if body["status"]["state"] == "idle" {
            return body;
        }
        assert!(Instant::now() < deadline, "the run did not end: {body}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn get_shows_an_idle_state_first() {
    let app = test_app_with(|_| {}).await;
    let (status, body) = call(&app, Method::GET, "/verify", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["enabled"], true);
    assert_eq!(body["status"]["state"], "idle");
    assert_eq!(body["status"]["counts"]["verified"], 0);
    assert_eq!(body["last_run"], Value::Null);
    assert_eq!(body["problems"], json!([]));
}

#[tokio::test]
async fn a_run_finds_a_mismatch_and_lists_it() {
    let good = vec![7u8; 4096];
    let mut bad = good.clone();
    bad[0] = 0;
    let (app, path, _listing) = app_with_file("a.mkv", &bad, &good).await;

    let (status, body) = call(&app, Method::POST, "/verify", Some(json!({}))).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["scope"], "everything");
    assert_eq!(body["files_total"], 1);

    let body = until_idle(&app).await;
    assert_eq!(body["last_run"]["state"], "finished", "{body}");
    assert_eq!(body["last_run"]["mismatch"], 1, "{body}");
    let problems = body["problems"].as_array().unwrap();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0]["result"], "mismatch");
    assert_eq!(problems[0]["local_path"], path);
    assert_eq!(problems[0]["queued"], true);
    assert!(problems[0]["detail"]
        .as_str()
        .unwrap()
        .starts_with("expected "));
}

#[tokio::test]
async fn an_empty_body_starts_everything() {
    let data = vec![1u8; 1024];
    let (app, _, _listing) = app_with_file("a.mkv", &data, &data).await;
    let (status, body) = call(&app, Method::POST, "/verify", None).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let body = until_idle(&app).await;
    assert_eq!(body["last_run"]["verified"], 1, "{body}");
}

#[tokio::test]
async fn a_second_start_conflicts_and_controls_steer_the_run() {
    let data = vec![3u8; 3_000_000];
    let (app, _, _listing) = app_with_file("big.mkv", &data, &data).await;
    let (status, _) = call(&app, Method::POST, "/verify", Some(json!({}))).await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let (status, body) = call(&app, Method::POST, "/verify", Some(json!({}))).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], "a verify run is already going");

    let (status, body) = call(&app, Method::POST, "/verify/resume", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let (status, body) = call(&app, Method::POST, "/verify/pause", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], "paused");
    let (status, body) = call(&app, Method::POST, "/verify/resume", None).await;
    assert_eq!(
        (status, &body["state"]),
        (StatusCode::OK, &json!("running"))
    );
    let (status, body) = call(&app, Method::POST, "/verify/stop", None).await;
    assert_eq!(
        (status, &body["state"]),
        (StatusCode::OK, &json!("stopping"))
    );

    let body = until_idle(&app).await;
    assert_eq!(body["last_run"]["state"], "stopped", "{body}");
    let (status, _) = call(&app, Method::POST, "/verify/stop", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn paths_that_were_not_downloaded_are_a_bad_request() {
    let app = test_app_with(|_| {}).await;
    let (status, body) = call(
        &app,
        Method::POST,
        "/verify",
        Some(json!({"paths": ["/nowhere/x.mkv"]})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn without_a_database_a_run_cannot_start() {
    let app = test_app_without_db().await;
    let (status, body) = call(&app, Method::GET, "/verify", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _) = call(&app, Method::POST, "/verify", Some(json!({}))).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}
