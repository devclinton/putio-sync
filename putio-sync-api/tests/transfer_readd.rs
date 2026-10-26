//! Stalled transfers and Re-add (feature 007, contracts/api.md): the transfer list carries
//! the tracker's verdict and the magnet link; `POST /putio/transfers/{id}/readd` cancels a
//! transfer and adds its magnet again to the same folder.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, TestApp};
use mockito::Matcher;
use putio_sync_client::Transfer;
use serde_json::json;
use std::time::{Duration, SystemTime};

const MAGNET: &str = "magnet:?xt=urn:btih:0123456789abcdef";

/// A Put.io transfer object, stuck at 97% with availability 92%
fn transfer(id: i64, magnet: Option<&str>) -> serde_json::Value {
    json!({
        "id": id, "name": "Ubuntu ISO", "size": 1000, "status": "DOWNLOADING",
        "down_speed": 0, "up_speed": 0, "downloaded": 970, "uploaded": 0,
        "percent_done": 97, "estimated_time": null, "peers_connected": 4,
        "peers_sending_to_us": 0, "availability": 92, "current_ratio": "0.00",
        "seconds_seeding": 0, "error_message": null,
        "created_at": "2026-10-25T12:00:00Z", "finished_at": null,
        "save_parent_id": 77, "file_id": null, "is_private": false,
        "magneturi": magnet.unwrap_or(""), "source": magnet, "type": "TORRENT"
    })
}

async fn mock_info(app: &mut TestApp, id: i64, magnet: Option<&str>) -> mockito::Mock {
    app.putio
        .mock("GET", format!("/transfers/{id}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "status": "OK", "transfer": transfer(id, magnet) }).to_string())
        .create_async()
        .await
}

async fn mock_cancel(app: &mut TestApp, id: i64, status: usize) -> mockito::Mock {
    let body = if status == 200 {
        r#"{"status":"OK"}"#
    } else {
        r#"{"status":"ERROR","error_type":"BadRequest","error_message":"cannot cancel"}"#
    };
    app.putio
        .mock("POST", "/transfers/cancel")
        .match_body(Matcher::Json(json!({ "transfer_ids": id.to_string() })))
        .with_status(status)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await
}

fn readd_path(id: i64) -> String {
    format!("/putio/transfers/{id}/readd")
}

async fn readd(app: &TestApp, id: i64) -> axum::response::Response {
    app.req(Method::POST, &readd_path(id), Some(&app.token), None)
        .await
}

/// US1: a transfer the tracker holds as stalled carries `stall`, `availability` and
/// `magnet` in the list
#[tokio::test]
async fn list_carries_stall_and_magnet() {
    let mut app = test_app().await;
    let _list = app
        .putio
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({ "status": "OK", "transfers": [transfer(42, Some(MAGNET)), transfer(43, None)] })
                .to_string(),
        )
        .create_async()
        .await;

    // Seed the tracker: seen unchanged 7 hours ago and now
    let seen: Vec<Transfer> = [transfer(42, Some(MAGNET)), transfer(43, None)]
        .into_iter()
        .map(|t| serde_json::from_value(t).unwrap())
        .collect();
    let now = SystemTime::now();
    app.state
        .stall_tracker
        .observe(&seen, now - Duration::from_secs(7 * 3600));
    assert_eq!(app.state.stall_tracker.observe(&seen, now), 2);

    let response = app.get("/putio/transfers").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let first = &body["items"][0];
    assert_eq!(first["magnet"], MAGNET, "{body}");
    assert_eq!(first["availability"], 92, "{body}");
    assert_eq!(first["stall"]["reason"], "availability 92%", "{body}");
    let seconds = first["stall"]["seconds"].as_u64().unwrap();
    assert!((7 * 3600..7 * 3600 + 60).contains(&seconds), "{body}");
    assert!(first["stall"]["since"].as_u64().is_some(), "{body}");

    let second = &body["items"][1];
    assert!(second.get("magnet").is_none(), "{body}");
    assert_eq!(second["stall"]["reason"], "availability 92%", "{body}");
}

/// A transfer that is not stalled has no `stall`
#[tokio::test]
async fn get_without_stall_omits_it() {
    let mut app = test_app().await;
    let _info = mock_info(&mut app, 42, Some(MAGNET)).await;
    let body = body_json(app.get("/putio/transfers/42").await).await;
    assert_eq!(body["id"], 42, "{body}");
    assert!(body.get("stall").is_none(), "{body}");
    assert_eq!(body["magnet"], MAGNET, "{body}");
}

/// US3 scenario 1: cancel once, add the same magnet to the same folder, answer with the
/// new transfer
#[tokio::test]
async fn readd_cancels_and_adds_the_magnet_again() {
    let mut app = test_app().await;
    let info = mock_info(&mut app, 42, Some(MAGNET)).await;
    let cancel = mock_cancel(&mut app, 42, 200).await;
    let mut added = transfer(99, Some(MAGNET));
    added["status"] = json!("IN_QUEUE");
    added["percent_done"] = json!(0);
    let add = app
        .putio
        .mock("POST", "/transfers/add")
        .match_body(Matcher::Json(
            json!({ "url": MAGNET, "save_parent_id": 77 }),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "status": "OK", "transfer": added }).to_string())
        .expect(1)
        .create_async()
        .await;

    let response = readd(&app, 42).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["cancelled_id"], 42, "{body}");
    assert_eq!(body["transfer"]["id"], 99, "{body}");
    assert_eq!(body["transfer"]["status"], "IN_QUEUE", "{body}");
    info.assert_async().await;
    cancel.assert_async().await;
    add.assert_async().await;
}

/// US3 scenario 4 / FR-013: no magnet → 400 and nothing cancelled
#[tokio::test]
async fn readd_without_magnet_is_refused_before_cancelling() {
    let mut app = test_app().await;
    let _info = mock_info(&mut app, 42, None).await;
    let cancel = app
        .putio
        .mock("POST", "/transfers/cancel")
        .expect(0)
        .create_async()
        .await;

    let response = readd(&app, 42).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = body_json(response).await;
    assert!(
        body["error"].as_str().unwrap().contains("no magnet link"),
        "{body}"
    );
    cancel.assert_async().await;
}

/// Cancel failing stops before adding anything
#[tokio::test]
async fn readd_stops_when_cancel_fails() {
    let mut app = test_app().await;
    let _info = mock_info(&mut app, 42, Some(MAGNET)).await;
    let _cancel = mock_cancel(&mut app, 42, 400).await;
    let add = app
        .putio
        .mock("POST", "/transfers/add")
        .expect(0)
        .create_async()
        .await;

    let response = readd(&app, 42).await;
    assert!(!response.status().is_success());
    add.assert_async().await;
}

/// US3 scenario 3 / FR-012: add failing after the cancel says so and hands back the magnet
#[tokio::test]
async fn readd_add_failure_reports_the_magnet() {
    let mut app = test_app().await;
    let _info = mock_info(&mut app, 42, Some(MAGNET)).await;
    let _cancel = mock_cancel(&mut app, 42, 200).await;
    let _add = app
        .putio
        .mock("POST", "/transfers/add")
        .with_status(400)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"status":"ERROR","error_type":"BadRequest","error_message":"Invalid magnet"}"#,
        )
        .create_async()
        .await;

    let response = readd(&app, 42).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = body_json(response).await;
    let error = body["error"].as_str().unwrap();
    assert!(error.contains("Cancelled transfer 42"), "{body}");
    assert!(error.contains("couldn't add it again"), "{body}");
    assert_eq!(body["details"], MAGNET, "{body}");
}

/// FR-014: the route needs the API token
#[tokio::test]
async fn readd_requires_auth() {
    let app = test_app().await;
    let response = app.req(Method::POST, &readd_path(42), None, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
