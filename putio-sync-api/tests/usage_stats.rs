//! Usage stats routes (feature 058 part 2): the token requirement, the exact payload, opting in
//! and out, bad bodies and the install id reset

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_app_with};
use serde_json::json;

#[tokio::test]
async fn routes_need_a_token() {
    let app = test_app().await;
    for (method, path) in [
        (Method::GET, "/usage-stats"),
        (Method::PUT, "/usage-stats"),
        (Method::POST, "/usage-stats/reset-id"),
    ] {
        let res = app
            .req(method, path, None, Some(json!({"enabled": true})))
            .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn default_is_off_and_shows_the_payload() {
    let app = test_app().await;
    let res = app.get("/usage-stats").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["enabled"], false);
    assert_eq!(body["endpoint_set"], false);
    assert!(body["last_sent"].is_null());
    let payload = &body["payload"];
    assert_eq!(payload["schema"], 1);
    assert!(!payload["install_id"].as_str().unwrap().is_empty());
    let text = payload.to_string();
    for secret in [
        app._dir.path().to_str().unwrap(),
        app.token.as_str(),
        common::PUTIO_TOKEN,
    ] {
        assert!(!text.contains(secret), "payload leaked {secret}");
    }
}

#[tokio::test]
async fn enable_and_disable_are_saved() {
    let app = test_app().await;
    let res = app
        .req(
            Method::PUT,
            "/usage-stats",
            Some(&app.token),
            Some(json!({"enabled": true})),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["enabled"], true);
    assert!(body["message"].as_str().unwrap().contains("Usage stats"));
    let file = std::fs::read_to_string(&**app.state.config_path.as_ref().unwrap()).unwrap();
    assert!(file.contains("[usage_stats]") && file.contains("enabled = true"));
    assert_eq!(
        body_json(app.get("/usage-stats").await).await["enabled"],
        true
    );

    let res = app
        .req(
            Method::PUT,
            "/usage-stats",
            Some(&app.token),
            Some(json!({"enabled": false})),
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        body_json(app.get("/usage-stats").await).await["enabled"],
        false
    );
}

#[tokio::test]
async fn a_bad_body_is_a_400() {
    let app = test_app().await;
    for body in [json!({}), json!({"enabled": "yes"})] {
        let res = app
            .req(Method::PUT, "/usage-stats", Some(&app.token), Some(body))
            .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn reset_id_makes_a_new_one() {
    let app = test_app().await;
    let before = body_json(app.get("/usage-stats").await).await["payload"]["install_id"].clone();
    let again = body_json(app.get("/usage-stats").await).await["payload"]["install_id"].clone();
    assert_eq!(before, again);
    let res = app
        .req(
            Method::POST,
            "/usage-stats/reset-id",
            Some(&app.token),
            None,
        )
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let after = body_json(res).await["payload"]["install_id"].clone();
    assert_ne!(before, after);
}

#[tokio::test]
async fn an_endpoint_in_the_config_shows_as_set_without_leaking_it() {
    let app = test_app_with(|config| {
        config.usage_stats.endpoint = "https://stats.example.com/in".into();
    })
    .await;
    let body = body_json(app.get("/usage-stats").await).await;
    assert_eq!(body["endpoint_set"], true);
    assert!(!body.to_string().contains("stats.example.com"));
}
