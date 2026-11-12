//! Update checker routes (feature 055): the notice, Check now and Dismiss, against a mock
//! releases endpoint

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, TestApp};
use mockito::Matcher;
use putio_sync_api::router;
use putio_sync_core::config::UpdatesConfig;
use putio_sync_update::UpdateService;
use serde_json::{json, Value};

const RELEASES: &str = r####"[
    {"tag_name": "v0.4.0", "published_at": "2026-11-20T10:00:00Z",
     "html_url": "https://github.com/devclinton/putio-sync/releases/tag/v0.4.0",
     "body": "### Added\n\n- An update checker.", "assets": []},
    {"tag_name": "v0.3.1", "published_at": "2026-11-12T10:00:00Z",
     "html_url": "https://github.com/devclinton/putio-sync/releases/tag/v0.3.1",
     "body": "", "assets": []},
    {"tag_name": "v0.3.0", "published_at": "2026-11-01T10:00:00Z",
     "html_url": "u", "body": "", "assets": []}
]"####;

/// The test app with an update checker running 0.3.0 against `releases`
async fn app_with(releases: &mockito::ServerGuard) -> TestApp {
    let mut app = test_app().await;
    let updates = UpdateService::for_tests(
        UpdatesConfig::default(),
        app.db.clone(),
        "0.3.0".parse().unwrap(),
        releases.url(),
    );
    app.state = app.state.clone().with_updates(updates);
    app.router = router(app.state.clone());
    app
}

async fn releases_server(body: &str) -> mockito::ServerGuard {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/releases")
        .match_query(Matcher::Any)
        .with_body(body)
        .create_async()
        .await;
    server
}

async fn get_update(app: &TestApp) -> Value {
    let res = app.get("/update").await;
    assert_eq!(res.status(), StatusCode::OK);
    body_json(res).await
}

#[tokio::test]
async fn nothing_known_before_the_first_check() {
    let server = releases_server(RELEASES).await;
    let app = app_with(&server).await;
    let body = get_update(&app).await;
    assert_eq!(body["current"], "0.3.0");
    assert_eq!(body["channel"]["kind"], "unknown");
    assert_eq!(body["channel"]["mode"], "instructions");
    assert_eq!(body["check"]["enabled"], true);
    assert_eq!(body["check"]["last_attempt_at"], Value::Null);
    assert_eq!(body["available"], Value::Null);
    assert_eq!(body["rolled_back"], Value::Null);
}

#[tokio::test]
async fn check_finds_the_newer_releases_with_notes() {
    let server = releases_server(RELEASES).await;
    let app = app_with(&server).await;
    let res = app.post_json("/update/check", json!({})).await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    let available = &body["available"];
    assert_eq!(available["version"], "0.4.0");
    assert_eq!(available["date"], "2026-11-20");
    assert_eq!(
        available["url"],
        "https://github.com/devclinton/putio-sync/releases/tag/v0.4.0"
    );
    assert_eq!(available["dismissed"], false);
    let releases = available["releases"].as_array().unwrap();
    assert_eq!(releases.len(), 2);
    assert_eq!(releases[0]["name"], "0.4.0");
    assert_eq!(releases[0]["kinds"][0]["name"], "Added");
    assert_eq!(
        releases[0]["kinds"][0]["entries"][0]["text"],
        "An update checker."
    );
    assert_eq!(releases[1]["name"], "0.3.1");
    assert_eq!(releases[1]["kinds"], json!([]));
    assert!(body["check"]["last_success_at"].is_string());

    // GET shows the same, and a second check within a minute is refused
    assert_eq!(get_update(&app).await["available"]["version"], "0.4.0");
    let res = app.post_json("/update/check", json!({})).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(body_json(res).await, json!({ "error": "checked_recently" }));
}

#[tokio::test]
async fn the_running_version_shows_nothing() {
    let server = releases_server(
        r#"[{"tag_name": "v0.3.0", "published_at": "2026-11-01T10:00:00Z",
            "html_url": "u", "body": "", "assets": []}]"#,
    )
    .await;
    let app = app_with(&server).await;
    let body = body_json(app.post_json("/update/check", json!({})).await).await;
    assert_eq!(body["available"], Value::Null);
    assert_eq!(body["check"]["error"], Value::Null);
}

#[tokio::test]
async fn a_failed_check_reports_its_error() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/releases")
        .match_query(Matcher::Any)
        .with_status(403)
        .with_header("x-ratelimit-remaining", "0")
        .create_async()
        .await;
    let app = app_with(&server).await;
    let body = body_json(app.post_json("/update/check", json!({})).await).await;
    assert_eq!(body["check"]["error"], "GitHub rate limit reached");
    assert_eq!(body["available"], Value::Null);
}

#[tokio::test]
async fn dismiss_hides_the_notice_and_is_stored() {
    let server = releases_server(RELEASES).await;
    let app = app_with(&server).await;
    app.post_json("/update/check", json!({})).await;

    let res = app
        .post_json("/update/dismiss", json!({ "version": "0.4.0" }))
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(get_update(&app).await["available"]["dismissed"], true);
    let stored = app.db().updates().get().await.unwrap();
    assert_eq!(stored.dismissed_version.as_deref(), Some("0.4.0"));

    let res = app
        .post_json("/update/dismiss", json!({ "version": "nope" }))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let res = app.post_json("/update/dismiss", json!({})).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn dismiss_rolled_back_clears_the_message() {
    let server = releases_server(RELEASES).await;
    let app = app_with(&server).await;
    app.db()
        .updates()
        .record_rollback("0.3.1", "did not start")
        .await
        .unwrap();
    app.state.updates.as_ref().unwrap().load().await;
    assert_eq!(
        get_update(&app).await["rolled_back"],
        json!({ "version": "0.3.1", "reason": "did not start" })
    );

    let res = app
        .post_json("/update/dismiss", json!({ "rolled_back": true }))
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(get_update(&app).await["rolled_back"], Value::Null);
    let stored = app.db().updates().get().await.unwrap();
    assert_eq!(stored.rollback_reason, None);
}

#[tokio::test]
async fn every_route_needs_auth() {
    let server = releases_server(RELEASES).await;
    let app = app_with(&server).await;
    for (method, path) in [
        (Method::GET, "/update"),
        (Method::POST, "/update/check"),
        (Method::POST, "/update/dismiss"),
    ] {
        let res = app.req(method.clone(), path, None, Some(json!({}))).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{method} {path}");
    }
}

#[tokio::test]
async fn without_a_checker_the_routes_say_so() {
    let app = test_app().await;
    let res = app.get("/update").await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
}
