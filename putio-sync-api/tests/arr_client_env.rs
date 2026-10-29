//! `PUT /arr/client` with `PUTIO_SYNC_ARR_PASSWORD` set (feature 018, FR-007). Its own test
//! binary, so the env var can't leak into other tests.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, body_text, test_app};
use putio_sync_core::config::Config;
use serde_json::json;

#[tokio::test]
async fn an_env_password_wins_and_is_never_written() {
    std::env::set_var("PUTIO_SYNC_ARR_PASSWORD", "env-password");
    let app = test_app().await;

    let body = body_json(app.get("/arr").await).await;
    assert_eq!(body["client"]["env"], json!({"password": true}));

    // Turning it on needs no form password: env has one. The form's is ignored.
    let response = app
        .req(
            Method::PUT,
            "/arr/client",
            Some(&app.token),
            Some(json!({"enabled": true, "username": "putio-sync", "password": "form-password"})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    assert!(!text.contains("env-password"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).unwrap()["password_changed"],
        false
    );

    let settings = app.state.arr.as_ref().unwrap().service.settings();
    assert!(settings.enabled);
    assert_eq!(settings.password.as_deref(), Some("env-password"));
    let file = Config::load(app._dir.path().join("config.toml")).unwrap();
    assert!(file.arr.enabled);
    assert_eq!(file.arr.password, None);
    std::env::remove_var("PUTIO_SYNC_ARR_PASSWORD");
}
