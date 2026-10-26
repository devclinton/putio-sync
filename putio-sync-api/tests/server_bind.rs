//! Regression tests: the full app router builds, the server binds, and
//! queue reorder routes reach their handlers.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use putio_sync_api::create_app;
use putio_sync_core::config::{AuthConfig, Config};
use putio_sync_db::Database;
use std::path::Path;
use tower::ServiceExt;

const API_TOKEN: &str = "test-api-token";

fn test_config() -> Config {
    let mut config = Config {
        auth: Some(AuthConfig::with_token("dummy-putio-token".to_string())),
        ..Default::default()
    };
    config.api.auth_token = Some(API_TOKEN.to_string());
    config
}

async fn seeded_app(db_path: &Path) -> axum::Router {
    let db = Database::new(db_path).await.unwrap();
    // Listed highest priority first: 101, 102, 103
    for (file_id, priority) in [(101, 2), (102, 1), (103, 0)] {
        db.queue()
            .add(
                file_id,
                &format!("file-{file_id}.mkv"),
                1024,
                "http://unused",
                "",
                priority,
            )
            .await
            .unwrap();
    }
    drop(db);
    create_app(test_config(), Some(db_path.to_path_buf()), None, None)
        .await
        .unwrap()
}

async fn post(app: &axum::Router, uri: &str) -> StatusCode {
    app.clone()
        .oneshot(
            Request::post(uri)
                .header("Authorization", format!("Bearer {API_TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn server_binds_and_serves_health_without_token() {
    let app = create_app(test_config(), None, None, None).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let http = reqwest::Client::new();
    let health = http
        .get(format!("http://{addr}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status(), 200);
    let body: serde_json::Value = health.json().await.unwrap();
    assert_eq!(body["status"], "ok");
    assert!(body["version"].is_string());

    let protected = http
        .get(format!("http://{addr}/stats"))
        .send()
        .await
        .unwrap();
    assert_eq!(protected.status(), 401);
}

#[tokio::test]
async fn queue_actions_route_by_file_id() {
    let dir = tempfile::tempdir().unwrap();
    let app = seeded_app(&dir.path().join("test.db")).await;

    assert_eq!(post(&app, "/local/queue/102/move_up").await, StatusCode::OK);
    assert_eq!(
        post(&app, "/local/queue/102/move_down").await,
        StatusCode::OK
    );
    assert_eq!(
        post(&app, "/local/queue/103/move_first").await,
        StatusCode::OK
    );
    assert_eq!(
        post(&app, "/local/queue/103/move_last").await,
        StatusCode::OK
    );
    assert_eq!(post(&app, "/local/queue/101/skip").await, StatusCode::OK);
}

#[tokio::test]
async fn queue_actions_reject_bad_and_unknown_ids() {
    let dir = tempfile::tempdir().unwrap();
    let app = seeded_app(&dir.path().join("test.db")).await;

    assert_eq!(
        post(&app, "/local/queue/abc/move_up").await,
        StatusCode::BAD_REQUEST
    );
    for action in ["move_up", "move_down", "move_first", "move_last", "skip"] {
        assert_eq!(
            post(&app, &format!("/local/queue/999/{action}")).await,
            StatusCode::NOT_FOUND,
            "{action} on unknown id"
        );
    }
}
