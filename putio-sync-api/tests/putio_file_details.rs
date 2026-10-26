//! `GET /putio/files/{id}` returns the name and `parent_id` the Put.io page walks up to
//! build the breadcrumb of a folder opened by URL (feature 004, US4).

mod common;

use axum::http::StatusCode;
use common::{body_json, test_app};
use serde_json::json;

#[tokio::test]
async fn file_details_carry_name_and_parent() {
    let mut app = test_app().await;
    let mock = app
        .putio
        .mock("GET", "/files/110")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({ "status": "OK", "file": {
                "id": 110, "name": "Extras", "size": 0, "file_type": "FOLDER",
                "parent_id": 100, "is_folder": true, "is_shared": false,
                "created_at": "2026-10-20T09:30:00", "content_type": "application/x-directory",
            } })
            .to_string(),
        )
        .create_async()
        .await;

    let response = app.get("/putio/files/110").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["name"], "Extras");
    assert_eq!(body["parent_id"], 100);
    mock.assert_async().await;
}
