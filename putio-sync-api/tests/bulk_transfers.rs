//! Bulk transfer routes (feature 008, US2 and US3): remove sends one Put.io cancel with
//! every id, retry one call per id with a result for each, and clear lists the transfers,
//! cleans those in the chosen status group with one call and cancels any the clean left.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, TestApp};
use mockito::Matcher;
use serde_json::{json, Value};

/// A Put.io transfer object
fn transfer(id: i64, status: &str) -> Value {
    json!({
        "id": id, "name": format!("t{id}"), "size": 1024, "status": status,
        "down_speed": 0, "up_speed": 0, "downloaded": 0, "uploaded": 0,
        "percent_done": 0, "estimated_time": 0, "peers": 0, "availability": 0,
        "current_ratio": 0.0, "seconds_seeding": 0, "error_message": null,
        "created_at": "2026-10-25T12:00:00Z", "finished_at": null,
        "save_parent_id": 0, "file_id": null, "is_private": false
    })
}

/// Two completed, one seeding, one failed, one downloading, one queued
async fn mock_list(app: &mut TestApp) -> mockito::Mock {
    let transfers = vec![
        transfer(1, "COMPLETED"),
        transfer(2, "SEEDING"),
        transfer(3, "ERROR"),
        transfer(4, "DOWNLOADING"),
        transfer(5, "COMPLETED"),
        transfer(6, "IN_QUEUE"),
    ];
    app.putio
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "status": "OK", "transfers": transfers }).to_string())
        .create_async()
        .await
}

/// Mock a POST to `path` whose body has `field` = `ids`, expected `hits` times
async fn mock_ids(
    app: &mut TestApp,
    path: &str,
    ids: &str,
    answer: Value,
    hits: usize,
) -> mockito::Mock {
    app.putio
        .mock("POST", path)
        .match_body(Matcher::Json(json!({ "transfer_ids": ids })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(answer.to_string())
        .expect(hits)
        .create_async()
        .await
}

/// Any POST to Put.io; for tests where nothing may change
async fn mock_no_post(app: &mut TestApp) -> mockito::Mock {
    app.putio
        .mock("POST", Matcher::Any)
        .expect(0)
        .create_async()
        .await
}

// ---- Remove -----------------------------------------------------------------

#[tokio::test]
async fn remove_cancels_all_in_one_call() {
    let mut app = test_app().await;
    let mock = mock_ids(
        &mut app,
        "/transfers/cancel",
        "1,4,5",
        json!({"status":"OK"}),
        1,
    )
    .await;

    let response = app
        .post_json(
            "/putio/transfers/remove",
            json!({ "transfer_ids": [1, 4, 1, 5] }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["message"],
        "Removed 3 transfer(s)"
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn bulk_routes_refuse_empty_lists() {
    let mut app = test_app().await;
    let mock = mock_no_post(&mut app).await;

    for path in ["/putio/transfers/remove", "/putio/transfers/retry"] {
        let response = app.post_json(path, json!({ "transfer_ids": [] })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        let response = app.post_json(path, json!({ "ids": [1] })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
    }
    mock.assert_async().await;
}

#[tokio::test]
async fn bulk_routes_need_token() {
    let mut app = test_app().await;
    let mock = mock_no_post(&mut app).await;

    for (path, body) in [
        ("/putio/transfers/remove", json!({ "transfer_ids": [1] })),
        ("/putio/transfers/retry", json!({ "transfer_ids": [1] })),
        (
            "/putio/transfers/clear",
            json!({ "status": "finished", "dry_run": false }),
        ),
    ] {
        let response = app.req(Method::POST, path, None, Some(body)).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
    mock.assert_async().await;
}

// ---- Retry ------------------------------------------------------------------

#[tokio::test]
async fn retry_reports_each_id() {
    let mut app = test_app().await;
    let ok = app
        .putio
        .mock("POST", "/transfers/retry")
        .match_body(Matcher::Json(json!({ "id": 3 })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK","transfer":{"id":3}}"#)
        .expect(1)
        .create_async()
        .await;
    let refused = app
        .putio
        .mock("POST", "/transfers/retry")
        .match_body(Matcher::Json(json!({ "id": 4 })))
        .with_status(400)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"ERROR","error_type":"BadRequest","error_message":"Transfer is not failed"}"#)
        .expect(1)
        .create_async()
        .await;

    let response = app
        .post_json("/putio/transfers/retry", json!({ "transfer_ids": [4, 3] }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["retried"], json!([3]), "{body}");
    assert_eq!(body["failed"].as_array().map(Vec::len), Some(1), "{body}");
    assert_eq!(body["failed"][0]["id"], 4, "{body}");
    assert!(
        !body["failed"][0]["error"]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "{body}"
    );
    ok.assert_async().await;
    refused.assert_async().await;
}

// ---- Clear ------------------------------------------------------------------

async fn clear(app: &TestApp, status: &str, dry_run: bool) -> axum::response::Response {
    app.post_json(
        "/putio/transfers/clear",
        json!({ "status": status, "dry_run": dry_run }),
    )
    .await
}

fn ids(body: &Value) -> Vec<i64> {
    body["transfers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_i64().unwrap())
        .collect()
}

#[tokio::test]
async fn dry_run_lists_the_group_and_changes_nothing() {
    for (status, expected) in [
        ("completed", vec![1, 2, 5]),
        ("failed", vec![3]),
        ("finished", vec![1, 2, 3, 5]),
    ] {
        let mut app = test_app().await;
        let list = mock_list(&mut app).await;
        let mock = mock_no_post(&mut app).await;

        let response = clear(&app, status, true).await;
        assert_eq!(response.status(), StatusCode::OK, "{status}");
        let body = body_json(response).await;
        assert_eq!(ids(&body), expected, "{status}");
        assert_eq!(body["count"], expected.len(), "{status}");
        assert_eq!(body["dry_run"], true);
        assert_eq!(body["status"], status);
        list.assert_async().await;
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn clear_cleans_the_group_in_one_call() {
    let mut app = test_app().await;
    let _list = mock_list(&mut app).await;
    let clean = mock_ids(
        &mut app,
        "/transfers/clean",
        "1,2,5",
        json!({ "status": "OK", "deleted_ids": [1, 2, 5] }),
        1,
    )
    .await;
    let cancel = app
        .putio
        .mock("POST", "/transfers/cancel")
        .expect(0)
        .create_async()
        .await;

    let response = clear(&app, "completed", false).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(ids(&body), vec![1, 2, 5]);
    assert_eq!(body["count"], 3);
    clean.assert_async().await;
    cancel.assert_async().await;
}

/// Ids the clean left in place go in one cancel (research R3)
#[tokio::test]
async fn clear_cancels_what_clean_left() {
    let mut app = test_app().await;
    let _list = mock_list(&mut app).await;
    let clean = mock_ids(
        &mut app,
        "/transfers/clean",
        "1,2,3,5",
        json!({ "status": "OK", "deleted_ids": [1, 5] }),
        1,
    )
    .await;
    let cancel = mock_ids(
        &mut app,
        "/transfers/cancel",
        "2,3",
        json!({"status":"OK"}),
        1,
    )
    .await;

    let response = clear(&app, "finished", false).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["count"], 4);
    clean.assert_async().await;
    cancel.assert_async().await;
}

#[tokio::test]
async fn clear_with_nothing_matching_sends_nothing() {
    let mut app = test_app().await;
    let _list = app
        .putio
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "status": "OK", "transfers": [transfer(4, "DOWNLOADING")] }).to_string())
        .create_async()
        .await;
    let mock = mock_no_post(&mut app).await;

    let response = clear(&app, "finished", false).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["count"], 0);
    assert_eq!(body["transfers"], json!([]));
    mock.assert_async().await;
}

#[tokio::test]
async fn clear_refuses_bad_requests() {
    let mut app = test_app().await;
    let list = app
        .putio
        .mock("GET", "/transfers/list")
        .expect(0)
        .create_async()
        .await;
    let any = mock_no_post(&mut app).await;

    for body in [
        json!({ "status": "completed" }),
        json!({ "status": "downloading", "dry_run": true }),
        json!({ "dry_run": false }),
    ] {
        let response = app.post_json("/putio/transfers/clear", body.clone()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
    }
    list.assert_async().await;
    any.assert_async().await;
}
