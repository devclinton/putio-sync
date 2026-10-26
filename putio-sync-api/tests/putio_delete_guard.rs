//! `DELETE /putio/files/{id}` refuses files the queue still needs (feature 004, US4,
//! R11, FR-020): pending, downloading and paused rows answer 409 without calling Put.io.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_app_without_db, TestApp};
use putio_sync_db::models::QueueStatus;
use serde_json::json;

async fn delete(app: &TestApp, id: i64) -> axum::response::Response {
    app.req(
        Method::DELETE,
        &format!("/putio/files/{id}"),
        Some(&app.token),
        None,
    )
    .await
}

/// Mock `POST /files/delete`, expected `hits` times
async fn mock_delete(app: &mut TestApp, hits: usize) -> mockito::Mock {
    app.putio
        .mock("POST", "/files/delete")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(hits)
        .create_async()
        .await
}

#[tokio::test]
async fn queued_file_409_without_calling_putio() {
    for status in [
        QueueStatus::Pending,
        QueueStatus::Downloading,
        QueueStatus::Paused,
    ] {
        let mut app = test_app().await;
        app.seed_queue(&[(7, "seven.mkv", status.clone())]).await;
        let mock = mock_delete(&mut app, 0).await;

        let response = delete(&app, 7).await;
        assert_eq!(response.status(), StatusCode::CONFLICT, "{status:?}");
        assert_eq!(
            body_json(response).await,
            json!({ "error": "File is queued or downloading", "details": "Skip or cancel it first" })
        );
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn completed_file_deleted() {
    let mut app = test_app().await;
    app.seed_queue(&[(8, "eight.mkv", QueueStatus::Completed)])
        .await;
    let mock = mock_delete(&mut app, 1).await;

    let response = delete(&app, 8).await;
    assert_eq!(response.status(), StatusCode::OK);
    mock.assert_async().await;
}

#[tokio::test]
async fn unqueued_file_deleted() {
    let mut app = test_app().await;
    let mock = mock_delete(&mut app, 1).await;

    assert_eq!(delete(&app, 9).await.status(), StatusCode::OK);
    mock.assert_async().await;
}

#[tokio::test]
async fn without_db_guard_skipped() {
    let mut app = test_app_without_db().await;
    let mock = mock_delete(&mut app, 1).await;

    assert_eq!(delete(&app, 7).await.status(), StatusCode::OK);
    mock.assert_async().await;
}
