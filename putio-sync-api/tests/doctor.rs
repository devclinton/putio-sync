//! Doctor routes (feature 010): run, report, preview and apply

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, TestApp};
use putio_sync_db::models::QueueStatus;
use serde_json::{json, Value};

/// A healthy Put.io, plus queue row 77 for a file Put.io no longer has
async fn seeded() -> TestApp {
    let mut app = test_app().await;
    app.putio
        .mock("GET", "/account/info")
        .with_header("content-type", "application/json")
        .with_body(
            json!({ "status": "OK", "info": { "user_id": 1, "username": "u",
                "mail": "u@example.com", "account_active": true,
                "disk": { "size": 1000, "used": 10, "avail": 990 } } })
            .to_string(),
        )
        .create_async()
        .await;
    app.putio
        .mock("GET", "/transfers/list")
        .with_header("content-type", "application/json")
        .with_body(json!({ "status": "OK", "transfers": [] }).to_string())
        .create_async()
        .await;
    let root = json!({ "id": 0, "name": "root", "size": 0, "file_type": "FOLDER",
        "parent_id": -1, "is_folder": true });
    app.mock_files_list(0, json!({ "status": "OK", "files": [], "parent": root }))
        .await;
    app.putio
        .mock("GET", "/files/77")
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(json!({ "status": "ERROR", "error_type": "NotFound" }).to_string())
        .create_async()
        .await;
    app.seed_queue(&[(77, "gone.mkv", QueueStatus::Pending)])
        .await;
    app
}

fn check<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap_or_else(|| panic!("no {id} in {report}"))
}

#[tokio::test]
async fn needs_a_token() {
    let app = test_app().await;
    for (method, path) in [
        (Method::GET, "/doctor"),
        (Method::POST, "/doctor/run"),
        (Method::POST, "/doctor/fixes/transfers.retry/preview"),
        (Method::POST, "/doctor/fixes/transfers.retry/apply"),
    ] {
        let response = app.req(method, path, None, None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn run_then_get_returns_the_report() {
    let app = seeded().await;
    let response = app.get("/doctor").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(response).await["error"], "no doctor run yet");

    let response = app.post_json("/doctor/run", json!({})).await;
    assert_eq!(response.status(), StatusCode::OK);
    let report = body_json(response).await;
    assert_eq!(check(&report, "dead_queue_rows")["status"], "warning");
    assert_eq!(check(&report, "auth")["status"], "ok");

    let response = app.get("/doctor").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["started_at"],
        report["started_at"]
    );
}

#[tokio::test]
async fn preview_and_apply_remove_dead_rows() {
    let app = seeded().await;
    app.post_json("/doctor/run", json!({})).await;
    let path = "/doctor/fixes/dead_queue_rows.remove";

    let response = app.post_json(&format!("{path}/preview"), json!({})).await;
    assert_eq!(response.status(), StatusCode::OK);
    let preview = body_json(response).await;
    assert_eq!(preview["destructive"], true);
    assert_eq!(preview["item_count"], 1);
    assert_eq!(preview["items"][0]["key"], "77");

    // Deleting needs confirm
    let response = app
        .post_json(&format!("{path}/apply"), json!({ "items": ["77"] }))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(app.db().queue().get_by_file_id(77).await.unwrap().is_some());

    let response = app
        .post_json(
            &format!("{path}/apply"),
            json!({ "items": ["77", "999"], "confirm": true }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let outcome = body_json(response).await;
    assert_eq!(outcome["done"].as_array().unwrap().len(), 1);
    assert_eq!(outcome["skipped"][0]["item"], "999");
    assert_eq!(outcome["check"]["status"], "ok");
    assert!(app.db().queue().get_by_file_id(77).await.unwrap().is_none());

    // The stored report has the re-run check
    let report = body_json(app.get("/doctor").await).await;
    assert_eq!(check(&report, "dead_queue_rows")["status"], "ok");
}

#[tokio::test]
async fn bad_requests() {
    let app = seeded().await;
    let response = app.post_json("/doctor/fixes/nope/preview", json!({})).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = app
        .post_json("/doctor/fixes/nope/apply", json!({ "items": ["1"] }))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = app
        .post_json(
            "/doctor/fixes/transfers.retry/apply",
            json!({ "items": [] }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = app
        .post_json("/doctor/fixes/transfers.retry/apply", json!({ "nope": 1 }))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
