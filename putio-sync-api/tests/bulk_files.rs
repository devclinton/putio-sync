//! `POST /putio/files/delete` deletes several Put.io items with one call (feature 008,
//! US1). Any file the queue still needs (pending, downloading, paused) makes the whole
//! request 409, naming it, without calling Put.io.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_app_without_db, TestApp};
use mockito::Matcher;
use putio_sync_db::models::QueueStatus;
use serde_json::json;

const PATH: &str = "/putio/files/delete";

/// Mock `POST /files/delete` with `ids` (comma-joined), expected `hits` times
async fn mock_delete(app: &mut TestApp, ids: &str, hits: usize) -> mockito::Mock {
    app.putio
        .mock("POST", "/files/delete")
        .match_body(Matcher::Json(json!({ "file_ids": ids })))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(hits)
        .create_async()
        .await
}

/// Any POST to Put.io; for tests where nothing may be deleted
async fn mock_nothing(app: &mut TestApp) -> mockito::Mock {
    app.putio
        .mock("POST", Matcher::Any)
        .expect(0)
        .create_async()
        .await
}

#[tokio::test]
async fn deletes_all_in_one_call() {
    let mut app = test_app().await;
    // Completed rows no longer need the file on Put.io
    app.seed_queue(&[(2, "two.mkv", QueueStatus::Completed)])
        .await;
    let mock = mock_delete(&mut app, "1,2,3", 1).await;

    let response = app.post_json(PATH, json!({ "file_ids": [1, 2, 3] })).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["message"],
        "Successfully deleted 3 file(s)"
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn duplicate_ids_sent_once() {
    let mut app = test_app().await;
    let mock = mock_delete(&mut app, "5,4", 1).await;

    let response = app
        .post_json(PATH, json!({ "file_ids": [5, 4, 5, 4] }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    mock.assert_async().await;
}

#[tokio::test]
async fn any_queued_file_409_and_nothing_deleted() {
    for status in [
        QueueStatus::Pending,
        QueueStatus::Downloading,
        QueueStatus::Paused,
    ] {
        let mut app = test_app().await;
        app.seed_queue(&[
            (7, "seven.mkv", status.clone()),
            (8, "eight.mkv", QueueStatus::Completed),
        ])
        .await;
        let mock = mock_nothing(&mut app).await;

        let response = app.post_json(PATH, json!({ "file_ids": [6, 7, 8] })).await;
        assert_eq!(response.status(), StatusCode::CONFLICT, "{status:?}");
        assert_eq!(
            body_json(response).await,
            json!({
                "error": "1 of 3 items are queued or downloading",
                "details": "Skip or cancel them first: seven.mkv"
            })
        );
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn names_every_protected_file() {
    let mut app = test_app().await;
    app.seed_queue(&[
        (7, "seven.mkv", QueueStatus::Pending),
        (9, "nine.mkv", QueueStatus::Paused),
    ])
    .await;
    let mock = mock_nothing(&mut app).await;

    let response = app.post_json(PATH, json!({ "file_ids": [7, 8, 9] })).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(response).await["details"],
        "Skip or cancel them first: seven.mkv, nine.mkv"
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn empty_or_bad_body_400() {
    let mut app = test_app().await;
    let mock = mock_nothing(&mut app).await;

    for body in [
        json!({ "file_ids": [] }),
        json!({}),
        json!({ "file_ids": "1,2" }),
    ] {
        let response = app.post_json(PATH, body.clone()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
    }
    mock.assert_async().await;
}

#[tokio::test]
async fn without_db_guard_skipped() {
    let mut app = test_app_without_db().await;
    let mock = mock_delete(&mut app, "7,8", 1).await;

    let response = app.post_json(PATH, json!({ "file_ids": [7, 8] })).await;
    assert_eq!(response.status(), StatusCode::OK);
    mock.assert_async().await;
}

#[tokio::test]
async fn needs_token() {
    let mut app = test_app().await;
    let mock = mock_nothing(&mut app).await;

    let response = app
        .req(Method::POST, PATH, None, Some(json!({ "file_ids": [1] })))
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    mock.assert_async().await;
}
