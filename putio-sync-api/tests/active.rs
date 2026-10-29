//! `GET /active`: the Active transfers pipeline (feature 039, contracts/api.md)

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_app_without_db, TestApp};
use putio_sync_db::models::{DownloadSource, Origin, QueueStatus};
use serde_json::{json, Value};
use std::time::Duration;

/// A Put.io transfer object
fn transfer(id: i64, name: &str, status: &str) -> Value {
    json!({
        "id": id, "name": name, "size": 1000, "status": status,
        "down_speed": 50, "up_speed": 0, "downloaded": 250, "uploaded": 0,
        "percent_done": 25, "estimated_time": 15, "peers": 3, "availability": 100,
        "current_ratio": 0.0, "seconds_seeding": 0, "error_message": null,
        "created_at": "2026-10-25T12:00:00", "finished_at": null,
        "save_parent_id": 0, "file_id": null, "is_private": false
    })
}

async fn mock_transfers(app: &mut TestApp, transfers: Vec<Value>) -> mockito::Mock {
    app.putio
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "status": "OK", "transfers": transfers }).to_string())
        .create_async()
        .await
}

/// Queue `file_id` as a file of `transfer_id`
async fn queue_file(app: &TestApp, file_id: i64, transfer_id: i64, status: QueueStatus) {
    app.seed_queue(&[(file_id, &format!("f{file_id}.mkv"), status)])
        .await;
    app.db()
        .sources()
        .record(
            file_id,
            &DownloadSource {
                transfer_id: Some(transfer_id),
                source_type: None,
                tracker_host: None,
                is_private: false,
                origin: Origin::Putio,
                watch_folder: None,
                putio_folder: None,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn active_returns_the_contract_shape() {
    let mut app = test_app().await;
    let _list = mock_transfers(
        &mut app,
        vec![
            transfer(1, "Downloading film", "DOWNLOADING"),
            transfer(2, "Show S01", "COMPLETED"),
        ],
    )
    .await;
    queue_file(&app, 20, 2, QueueStatus::Pending).await;
    queue_file(&app, 21, 2, QueueStatus::Pending).await;

    let response = app.get("/active").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;

    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 2, "{body}");
    let putio = items.iter().find(|i| i["key"] == "transfer:1").unwrap();
    assert_eq!(putio["stage"], "putio", "{putio}");
    assert_eq!(putio["state"], "downloading");
    assert_eq!(putio["speed_bps"], 50);
    assert_eq!(putio["links"]["transfers"], "#/transfers?id=1");
    assert_eq!(putio["links"]["queue"], "#/queue?transfer=1");
    let nas = items.iter().find(|i| i["key"] == "transfer:2").unwrap();
    assert_eq!(nas["stage"], "nas", "{nas}");
    assert_eq!(nas["state"], "queued");
    assert_eq!(nas["files"]["total"], 2);
    assert_eq!(nas["putio_status"], "COMPLETED");

    for key in ["held", "paused", "size", "done_bytes", "percent", "seeding"] {
        assert!(putio.get(key).is_some(), "{key} missing: {putio}");
    }
    let counts = &body["counts"];
    assert_eq!(counts["putio"]["downloading"], 1, "{counts}");
    assert_eq!(counts["putio"]["stalled"], 0, "zeros are listed: {counts}");
    assert_eq!(counts["putio"]["on_hold"], 0, "{counts}");
    assert_eq!(counts["nas"]["queued"], 1, "{counts}");
    assert_eq!(counts["nas"]["failed"], 0, "{counts}");
    assert_eq!(body["nas_paused"], false);
    assert_eq!(body["putio_stale"], false);
    assert!(
        body["putio_as_of"].as_str().unwrap().ends_with('Z'),
        "{body}"
    );
    assert!(body["recently_finished"].is_array(), "{body}");
}

#[tokio::test]
async fn active_requires_auth() {
    let app = test_app().await;
    let response = app.req(Method::GET, "/active", None, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn active_without_a_database_is_503() {
    let app = test_app_without_db().await;
    let response = app.get("/active").await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(response).await;
    assert!(
        !body["error"].as_str().unwrap_or_default().is_empty(),
        "{body}"
    );
}

/// Put.io failing keeps the last list, marked stale, while NAS figures stay current.
/// A 400 stands in for the outage: the client retries a 503 with backoff for ~30 s.
#[tokio::test]
async fn putio_down_serves_the_last_list_marked_stale() {
    let mut app = test_app().await;
    let list = mock_transfers(&mut app, vec![transfer(2, "Show S01", "COMPLETED")]).await;
    queue_file(&app, 20, 2, QueueStatus::Pending).await;
    let body = body_json(app.get("/active").await).await;
    assert_eq!(body["putio_stale"], false, "{body}");
    let as_of = body["putio_as_of"].clone();

    list.remove_async().await;
    let _down = app
        .putio
        .mock("GET", "/transfers/list")
        .with_status(400)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"ERROR","error_type":"Down","error_message":"down"}"#)
        .create_async()
        .await;
    // Force the refetch now instead of waiting out the 10 s cache
    app.state
        .download_manager
        .transfers_snapshot(Duration::ZERO)
        .await
        .unwrap();
    queue_file(&app, 21, 2, QueueStatus::Pending).await;

    let response = app.get("/active").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["putio_stale"], true, "{body}");
    assert_eq!(body["putio_as_of"], as_of);
    let item = &body["items"][0];
    assert_eq!(item["key"], "transfer:2", "{body}");
    assert_eq!(item["files"]["total"], 2, "NAS figures are current: {item}");
}
