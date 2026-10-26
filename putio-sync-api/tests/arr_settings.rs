//! The Settings card's `/arr` routes (feature 011, contracts/api.md)

mod common;

use axum::http::StatusCode;
use common::{body_json, test_app, test_app_with};
use putio_sync_core::config::{ArrConfig, ArrKind, ArrServer};
use serde_json::json;

fn server(kind: ArrKind, url: String, api_key: Option<&str>, category: &str) -> ArrServer {
    ArrServer {
        kind,
        url,
        api_key: api_key.map(str::to_string),
        category: category.to_string(),
    }
}

#[tokio::test]
async fn disabled_shows_defaults() {
    let app = test_app().await;
    let body = body_json(app.get("/arr").await).await;
    assert_eq!(body["enabled"], false);
    assert_eq!(body["password_set"], false);
    assert_eq!(body["username"], "putio-sync");
    assert_eq!(body["url_base"], "/transmission/");
    assert_eq!(body["servers"], json!([]));
    assert_eq!(body["clients_seen"], json!([]));
    assert_eq!(
        body["items"],
        json!({"putio": 0, "local": 0, "done": 0, "failed": 0})
    );
}

#[tokio::test]
async fn enabled_lists_servers_without_keys() {
    let app = test_app_with(|config| {
        config.arr = ArrConfig {
            enabled: true,
            password: Some("pw".into()),
            servers: vec![
                server(
                    ArrKind::Sonarr,
                    "http://nas:8989/".into(),
                    Some("k1"),
                    "sonarr",
                ),
                server(ArrKind::Radarr, "http://nas:7878".into(), None, "radarr"),
            ],
            ..ArrConfig::default()
        };
    })
    .await;
    let body = body_json(app.get("/arr").await).await;
    assert_eq!(body["enabled"], true);
    assert_eq!(body["password_set"], true);
    assert_eq!(
        body["download_dir"],
        app._dir.path().display().to_string().trim_end_matches('/')
    );
    assert_eq!(
        body["servers"],
        json!([
            {"index": 0, "kind": "sonarr", "url": "http://nas:8989", "category": "sonarr", "api_key_set": true},
            {"index": 1, "kind": "radarr", "url": "http://nas:7878", "category": "radarr", "api_key_set": false},
        ])
    );
}

#[tokio::test]
async fn server_test_results() {
    let mut arr = mockito::Server::new_async().await;
    let status = arr
        .mock("GET", "/api/v3/system/status")
        .match_header("x-api-key", "good")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"appName":"Sonarr","version":"4.0.15.2941"}"#)
        .create_async()
        .await;
    arr.mock("GET", "/api/v3/system/status")
        .match_header("x-api-key", "bad")
        .with_status(401)
        .create_async()
        .await;
    let url = arr.url();
    let app = test_app_with(|config| {
        config.arr.servers = vec![
            server(ArrKind::Sonarr, url.clone(), Some("good"), "sonarr"),
            server(ArrKind::Sonarr, url.clone(), Some("bad"), "tv"),
            server(ArrKind::Radarr, url.clone(), Some("good"), "radarr"),
            server(ArrKind::Radarr, url, None, "movies"),
        ];
    })
    .await;

    let test = |i: usize| format!("/arr/servers/{i}/test");
    let ok = body_json(app.post_json(&test(0), json!({})).await).await;
    assert_eq!(
        ok,
        json!({"ok": true, "app": "Sonarr", "version": "4.0.15.2941"})
    );
    status.assert_async().await;

    let bad = body_json(app.post_json(&test(1), json!({})).await).await;
    assert_eq!(bad["ok"], false);
    assert_eq!(bad["reason"], "unauthorized");

    let wrong_app = body_json(app.post_json(&test(2), json!({})).await).await;
    assert_eq!(wrong_app["reason"], "not_arr");
    assert_eq!(wrong_app["detail"], "not Radarr: found Sonarr");

    let no_key = body_json(app.post_json(&test(3), json!({})).await).await;
    assert_eq!(no_key["reason"], "no_api_key");
    assert!(no_key["detail"]
        .as_str()
        .unwrap()
        .contains("PUTIO_SYNC_RADARR_API_KEY"));

    assert_eq!(
        app.post_json(&test(9), json!({})).await.status(),
        StatusCode::NOT_FOUND
    );
}
