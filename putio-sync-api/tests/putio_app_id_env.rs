//! `PUTIO_SYNC_APP_ID` wins over `[putio] app_id`, and the settings group can't change it.
//! Its own test binary, so the env var can't leak into other tests.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app};
use serde_json::json;

#[tokio::test]
async fn an_env_app_id_wins_and_the_setting_is_read_only() {
    std::env::set_var("PUTIO_SYNC_APP_ID", "999");
    let app = test_app().await;
    let body = body_json(app.get("/settings/putio").await).await;
    assert_eq!(body["status"]["in_use"], json!("999"));
    assert_eq!(body["status"]["source"], json!("environment"));
    assert_eq!(body["status"]["env_override"], json!(true));

    let response = app
        .req(
            Method::PUT,
            "/settings/putio",
            Some(&app.token),
            Some(json!({ "app_id": "4242" })),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let error = body_json(response).await;
    assert!(error.to_string().contains("PUTIO_SYNC_APP_ID"), "{error}");
    // Saving it unchanged is fine
    let response = app
        .req(
            Method::PUT,
            "/settings/putio",
            Some(&app.token),
            Some(json!({ "app_id": null })),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    std::env::remove_var("PUTIO_SYNC_APP_ID");
}
