//! `GET /status`, the one request behind the status page (feature 004, FR-010).

mod common;

use axum::http::StatusCode;
use common::{body_json, body_text, test_app, test_app_without_db, API_TOKEN, PUTIO_TOKEN};
use putio_sync_db::models::QueueStatus;
use serde_json::json;

#[tokio::test]
async fn status_full() {
    let app = test_app().await;
    app.seed_queue(&[
        (1, "a.mkv", QueueStatus::Pending),
        (2, "b.mkv", QueueStatus::Pending),
        (3, "c.mkv", QueueStatus::Completed),
    ])
    .await;

    let response = app.get("/status").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;

    assert_eq!(body["build"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(body["build"]["commit"].is_string(), "{body}");
    assert!(body["uptime_seconds"].as_u64().is_some(), "{body}");
    assert_eq!(
        body["queue"],
        json!({"pending": 2, "downloading": 0, "completed": 1, "failed": 0, "paused": 0, "skipped": 0})
    );
    assert_eq!(body["downloads"], json!([]));
    let configured = app.state.config.read().await.download.workers;
    assert_eq!(body["workers"]["configured"], configured);
    assert_eq!(body["workers"]["busy"], 0);
    assert_eq!(body["watcher"]["configured"], false);
    assert!(body["disk"]["total_bytes"].as_u64().unwrap() > 0, "{body}");
    assert!(body["disk"]["free_bytes"].as_u64().is_some(), "{body}");
    assert_eq!(body["reachability"]["database"], "ok");
    assert_eq!(body["reachability"]["putio"], "unknown");
    assert_eq!(body["sync"]["in_progress"], false);
    assert!(body["sync"]["last_outcome"].is_null(), "{body}");
    assert!(body["sync"]["next_due_at"].is_null(), "{body}");
    assert_eq!(body["auth"], json!({"putio": "token", "api_token": true}));
    assert_eq!(body["unavailable"], json!([]));
}

#[tokio::test]
async fn status_empty_queue_counts_zero() {
    let app = test_app().await;
    let body = body_json(app.get("/status").await).await;
    assert_eq!(
        body["queue"],
        json!({"pending": 0, "downloading": 0, "completed": 0, "failed": 0, "paused": 0, "skipped": 0})
    );
    assert_eq!(body["unavailable"], json!([]));
}

#[tokio::test]
async fn status_requires_token() {
    let app = test_app().await;
    let response = app
        .req(axum::http::Method::GET, "/status", None, None)
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn status_without_database() {
    let app = test_app_without_db().await;
    let response = app.get("/status").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;

    assert!(body["queue"].is_null(), "{body}");
    assert_eq!(body["reachability"]["database"], "unavailable");
    let unavailable = body["unavailable"].as_array().unwrap();
    assert!(
        unavailable
            .iter()
            .any(|u| u["section"] == "queue" && u["reason"] == "database unavailable"),
        "{body}"
    );
    // The rest of the page still renders
    assert_eq!(body["build"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(body["disk"]["total_bytes"].as_u64().is_some(), "{body}");
}

#[tokio::test]
async fn status_never_contains_tokens() {
    let app = test_app().await;
    let text = body_text(app.get("/status").await).await;
    assert!(!text.contains(API_TOKEN), "{text}");
    assert!(!text.contains(PUTIO_TOKEN), "{text}");
}

#[tokio::test]
async fn status_after_sync() {
    let mut app = test_app().await;
    let _list = app
        .mock_files_list(
            0,
            json!({
                "files": [],
                "parent": {"id": 0, "name": "Root", "size": 0, "file_type": "FOLDER",
                           "parent_id": 0, "is_folder": true, "is_shared": false},
                "status": "OK"
            }),
        )
        .await;

    app.state.download_manager.sync_downloads().await.unwrap();

    let body = body_json(app.get("/status").await).await;
    assert_eq!(body["sync"]["last_outcome"]["result"], "success", "{body}");
    assert_eq!(body["sync"]["last_outcome"]["found"], 0);
    assert!(body["sync"]["last_started_at"].is_string(), "{body}");
    assert!(body["sync"]["last_finished_at"].is_string(), "{body}");
    assert!(
        body["sync"]["last_duration_seconds"].as_f64().is_some(),
        "{body}"
    );
    assert_eq!(body["reachability"]["putio"], "ok");
}

#[tokio::test]
async fn status_after_failed_sync() {
    let mut app = test_app().await;
    let _list = app
        .putio
        .mock("GET", "/files/list?parent_id=0")
        // 404 is not retried, so the sync fails at once
        .with_status(404)
        .create_async()
        .await;

    assert!(app.state.download_manager.sync_downloads().await.is_err());

    let body = body_json(app.get("/status").await).await;
    assert_eq!(body["sync"]["last_outcome"]["result"], "failure", "{body}");
    assert!(body["sync"]["last_outcome"]["error"].is_string(), "{body}");
    assert_eq!(body["reachability"]["putio"], "failed");
}
