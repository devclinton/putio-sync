//! `POST /local/queue {file_id}`: the UI's "download" action queues a Put.io file or
//! folder through the folder sync's filters and dedupe (feature 004, US4, R12, FR-014).

mod common;

use axum::http::StatusCode;
use common::{body_json, test_app, test_app_without_db, TestApp};
use serde_json::{json, Value};

const FOLDER: i64 = 50;

fn entry(id: i64, name: &str, file_type: &str, parent_id: i64) -> Value {
    json!({
        "id": id,
        "name": name,
        "size": 100,
        "file_type": file_type,
        "parent_id": parent_id,
        "is_folder": file_type == "FOLDER",
        "is_shared": false,
    })
}

/// A folder `Show` (50) at the root holding `a.mkv` (1), `b.exe` (2) and `c.mkv` (3),
/// with `c.mkv` already queued. `exe` is in the default blocked extensions.
async fn folder_app() -> (TestApp, Vec<mockito::Mock>) {
    let mut app = test_app().await;
    let folder = entry(FOLDER, "Show", "FOLDER", 0);
    let a = entry(1, "a.mkv", "VIDEO", FOLDER);
    let mocks = vec![
        app.mock_file(FOLDER, json!({ "status": "OK", "file": folder }))
            .await,
        app.mock_file(1, json!({ "status": "OK", "file": a })).await,
        app.mock_files_list(
            FOLDER,
            json!({
                "status": "OK",
                "parent": folder,
                "files": [
                    a,
                    entry(2, "b.exe", "FILE", FOLDER),
                    entry(3, "c.mkv", "VIDEO", FOLDER),
                ],
            }),
        )
        .await,
    ];
    app.db()
        .queue()
        .add(3, "c.mkv", 100, "http://unused", "Show", 0)
        .await
        .unwrap();
    (app, mocks)
}

#[tokio::test]
async fn folder_is_queued_with_counts() {
    let (app, _mocks) = folder_app().await;

    let response = app
        .post_json("/local/queue", json!({ "file_id": FOLDER }))
        .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        body_json(response).await,
        json!({ "found": 3, "queued": 1, "skipped": 1, "blocked": 1, "failed": 0 })
    );

    let item = app.db().queue().get_by_file_id(1).await.unwrap().unwrap();
    assert_eq!(item.relative_path, "Show");
    assert!(app.db().queue().get_by_file_id(2).await.unwrap().is_none());
}

#[tokio::test]
async fn single_file_is_queued() {
    let (app, _mocks) = folder_app().await;

    let response = app.post_json("/local/queue", json!({ "file_id": 1 })).await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(
        body_json(response).await,
        json!({ "found": 1, "queued": 1, "skipped": 0, "blocked": 0, "failed": 0 })
    );
}

#[tokio::test]
async fn unknown_file_404() {
    let mut app = test_app().await;
    let _missing = app
        .putio
        .mock("GET", "/files/999")
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"ERROR","error_type":"NotFound"}"#)
        .create_async()
        .await;

    let response = app
        .post_json("/local/queue", json!({ "file_id": 999 }))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(body_json(response).await["error"], "File not found");
}

#[tokio::test]
async fn without_db_503() {
    let app = test_app_without_db().await;
    let response = app.post_json("/local/queue", json!({ "file_id": 1 })).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body_json(response).await["error"], "Database unavailable");
}
