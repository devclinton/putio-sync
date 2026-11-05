//! The `putio` settings group: the Put.io app ID that turns on the device link.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app};
use putio_sync_core::config::Config;
use serde_json::json;

#[tokio::test]
async fn the_app_id_is_checked_saved_and_shown_with_its_source() {
    if std::env::var("PUTIO_SYNC_APP_ID").is_ok()
        || putio_sync_core::setup::BUILTIN_APP_ID.is_some()
    {
        return;
    }
    let app = test_app().await;
    let body = body_json(app.get("/settings/putio").await).await;
    assert_eq!(body["app_id"], json!(null));
    assert_eq!(body["status"]["in_use"], json!(null));
    assert_eq!(body["status"]["source"], json!(null));
    assert_eq!(body["status"]["env_override"], json!(false));
    assert_eq!(
        body["status"]["register_url"],
        json!("https://app.put.io/oauth")
    );

    let put = |id: &str| {
        app.req(
            Method::PUT,
            "/settings/putio",
            Some(&app.token),
            Some(json!({ "app_id": id })),
        )
    };
    assert_eq!(put("not-a-number").await.status(), StatusCode::BAD_REQUEST);
    assert_eq!(put(" 4242 ").await.status(), StatusCode::OK);
    let body = body_json(app.get("/settings/putio").await).await;
    assert_eq!(body["app_id"], json!("4242"));
    assert_eq!(body["status"]["in_use"], json!("4242"));
    assert_eq!(body["status"]["source"], json!("config"));
    let file = Config::load(app._dir.path().join("config.toml")).unwrap();
    assert_eq!(file.putio.app_id.as_deref(), Some("4242"));

    // Empty clears it
    assert_eq!(put("").await.status(), StatusCode::OK);
    let file = Config::load(app._dir.path().join("config.toml")).unwrap();
    assert_eq!(file.putio.app_id, None);
}
