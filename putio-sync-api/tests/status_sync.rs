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
async fn status_reports_the_nas_switch() {
    let app = test_app().await;
    let body = body_json(app.get("/status").await).await;
    assert_eq!(body["nas_paused"], false);
    app.post_json("/local/queue/pause", json!({})).await;
    let body = body_json(app.get("/status").await).await;
    assert_eq!(body["nas_paused"], true);
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

#[tokio::test]
async fn sync_now_202_then_409() {
    let mut app = test_app().await;
    let listing = json!({
        "files": [],
        "parent": {"id": 0, "name": "Root", "size": 0, "file_type": "FOLDER",
                   "parent_id": 0, "is_folder": true, "is_shared": false},
        "status": "OK"
    })
    .to_string();
    // A slow listing keeps the first sync running while the second request arrives
    let _list = app
        .putio
        .mock("GET", "/files/list?parent_id=0")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_chunked_body(move |w| {
            std::thread::sleep(std::time::Duration::from_millis(500));
            w.write_all(listing.as_bytes())
        })
        .create_async()
        .await;

    let first = app.post_json("/local/sync", json!({})).await;
    assert_eq!(first.status(), StatusCode::ACCEPTED);
    let body = body_json(first).await;
    assert_eq!(body["in_progress"], true, "{body}");
    assert!(body["last_started_at"].is_string(), "{body}");

    let second = app.post_json("/local/sync", json!({})).await;
    assert_eq!(second.status(), StatusCode::CONFLICT);
    let body = body_json(second).await;
    assert_eq!(body["error"], "A sync is already in progress", "{body}");

    // The spawned sync finishes and /status shows its outcome
    let mut status = json!(null);
    for _ in 0..100 {
        status = body_json(app.get("/status").await).await;
        if status["sync"]["in_progress"] == false {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(status["sync"]["in_progress"], false, "{status}");
    assert_eq!(
        status["sync"]["last_outcome"]["result"], "success",
        "{status}"
    );
    assert_eq!(status["reachability"]["putio"], "ok");
}

#[tokio::test]
async fn sync_now_503_without_db() {
    let app = test_app_without_db().await;
    let response = app.post_json("/local/sync", json!({})).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(response).await;
    assert_eq!(body["error"], "Database unavailable", "{body}");
}

#[tokio::test]
async fn sync_now_requires_token() {
    let app = test_app().await;
    let response = app
        .req(axum::http::Method::POST, "/local/sync", None, None)
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// The Active transfers card (feature 039, FR-030): totals and the busiest items, from
/// the last Put.io list; `/status` itself never asks Put.io
#[tokio::test]
async fn status_active_summarises_the_pipeline() {
    let mut app = test_app().await;
    let transfer = |id: i64, status: &str, speed: i64| {
        json!({
            "id": id, "name": format!("T{id}"), "size": 1000, "status": status,
            "down_speed": speed, "downloaded": 250, "percent_done": 25, "estimated_time": 15,
            "error_message": null, "created_at": "2026-10-25T12:00:00", "finished_at": null,
            "save_parent_id": 0
        })
    };
    let list = app
        .putio
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({"status": "OK", "transfers": [
                transfer(1, "IN_QUEUE", 0),
                transfer(2, "DOWNLOADING", 10),
                transfer(3, "DOWNLOADING", 90),
                transfer(4, "ERROR", 0),
            ]})
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    app.seed_queue(&[(50, "loose.mkv", QueueStatus::Failed)])
        .await;

    // Before any list is fetched: NAS items only
    let body = body_json(app.get("/status").await).await;
    assert_eq!(body["active"]["total"], 1, "{body}");
    assert_eq!(body["active"]["nas"], 1);

    app.get("/active").await;
    let body = body_json(app.get("/status").await).await;
    let active = &body["active"];
    assert_eq!(active["total"], 5, "{active}");
    assert_eq!(active["putio"], 4);
    assert_eq!(active["nas"], 1);
    assert_eq!(active["failed"], 2);
    let top = active["top"].as_array().unwrap();
    assert_eq!(top[0]["key"], "transfer:3", "{active}");
    assert_eq!(top[1]["key"], "transfer:2");
    assert_eq!(top[2]["key"], "transfer:1");
    for key in ["name", "stage", "state", "percent", "eta_seconds"] {
        assert!(top[0].get(key).is_some(), "{key} missing: {active}");
    }
    assert!(body["downloads"].is_array(), "downloads stays: {body}");
    list.assert_async().await;
}
