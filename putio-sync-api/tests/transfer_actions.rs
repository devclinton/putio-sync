//! Transfer actions map to real Put.io calls (feature 004, US4, FR-016): retry posts
//! `/transfers/retry`, remove posts `/transfers/cancel`. Put.io has no pause or resume,
//! so there are no start/stop routes.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, TestApp};
use mockito::Matcher;
use serde_json::json;

/// A Put.io transfer object
fn transfer(id: i64, status: &str) -> serde_json::Value {
    json!({
        "id": id, "name": "Ubuntu ISO", "size": 1024, "status": status,
        "down_speed": 0, "up_speed": 0, "downloaded": 0, "uploaded": 0,
        "percent_done": 0, "estimated_time": 0, "peers": 0, "availability": 0,
        "current_ratio": 0.0, "seconds_seeding": 0, "error_message": null,
        "created_at": "2026-10-25T12:00:00Z", "finished_at": null,
        "save_parent_id": 0, "file_id": null, "is_private": false
    })
}

async fn post(app: &TestApp, path: &str) -> axum::response::Response {
    app.req(Method::POST, path, Some(&app.token), None).await
}

#[tokio::test]
async fn retry_calls_putio() {
    let mut app = test_app().await;
    let mock = app
        .putio
        .mock("POST", "/transfers/retry")
        .match_body(Matcher::Json(json!({ "id": 42 })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK","transfer":{"id":42}}"#)
        .expect(1)
        .create_async()
        .await;

    let info = app
        .putio
        .mock("GET", "/transfers/42")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "status": "OK", "transfer": transfer(42, "IN_QUEUE") }).to_string())
        .create_async()
        .await;

    let response = post(&app, "/putio/transfers/42/retry").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["id"], 42, "{body}");
    assert_eq!(body["name"], "Ubuntu ISO", "{body}");
    mock.assert_async().await;
    info.assert_async().await;
}

/// Put.io refuses a retry of a transfer that hasn't failed; the error reaches the UI
#[tokio::test]
async fn retry_refused_by_putio_is_an_error() {
    let mut app = test_app().await;
    let mock = app
        .putio
        .mock("POST", "/transfers/retry")
        .with_status(400)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"ERROR","error_type":"BadRequest","error_message":"Transfer is not failed"}"#)
        .expect(1)
        .create_async()
        .await;

    let response = post(&app, "/putio/transfers/7/retry").await;
    assert!(!response.status().is_success(), "{}", response.status());
    let body = body_json(response).await;
    assert!(
        !body["error"].as_str().unwrap_or_default().is_empty(),
        "{body}"
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn remove_cancels_on_putio() {
    let mut app = test_app().await;
    let mock = app
        .putio
        .mock("POST", "/transfers/cancel")
        .match_body(Matcher::Json(json!({ "transfer_ids": "9" })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(1)
        .create_async()
        .await;

    let response = app
        .req(Method::DELETE, "/putio/transfers/9", Some(&app.token), None)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    mock.assert_async().await;
}

/// The old no-op start/stop routes are gone and call nothing on Put.io
#[tokio::test]
async fn start_and_stop_are_gone() {
    let mut app = test_app().await;
    let any = app
        .putio
        .mock("POST", Matcher::Any)
        .expect(0)
        .create_async()
        .await;

    for verb in ["start", "stop"] {
        let response = post(&app, &format!("/putio/transfers/3/{verb}")).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{verb}");
    }
    any.assert_async().await;
}
