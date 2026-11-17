//! Update checker routes (feature 055): the notice, Check now and Dismiss, against a mock
//! releases endpoint

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, TestApp};
use mockito::Matcher;
use putio_sync_api::router;
use putio_sync_core::config::UpdatesConfig;
use putio_sync_update::channel::Channel;
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

/// The test app with an update checker on `channel` that has found 0.4.0
async fn checked_on(server: &mockito::ServerGuard, channel: Channel) -> TestApp {
    let mut app = test_app().await;
    let updates = UpdateService::for_tests(
        UpdatesConfig::default(),
        app.db.clone(),
        "0.3.0".parse().unwrap(),
        server.url(),
    )
    .with_channel(channel);
    app.state = app.state.clone().with_updates(updates);
    app.router = router(app.state.clone());
    let res = app.post_json("/update/check", json!({})).await;
    assert_eq!(res.status(), StatusCode::OK);
    app
}

#[tokio::test]
async fn snap_gets_its_command() {
    let server = releases_server(RELEASES).await;
    let app = checked_on(&server, Channel::Snap).await;
    let body = get_update(&app).await;
    assert_eq!(body["channel"]["kind"], "snap");
    assert_eq!(body["channel"]["label"], "Snap");
    assert_eq!(body["channel"]["mode"], "instructions");
    assert!(body["channel"].get("image").is_none());
    assert_eq!(
        body["available"]["instruction"]["commands"],
        json!(["sudo snap refresh putio-sync"])
    );
    let status = body_json(app.get("/status").await).await;
    assert_eq!(status["install_channel"], "snap");
}

#[tokio::test]
async fn container_gets_the_pull_for_its_tag() {
    let server = releases_server(RELEASES).await;
    let channel = Channel::Container {
        image: "ghcr.io/devclinton/putio-sync".into(),
        variant: Some("dhi".into()),
    };
    let app = checked_on(&server, channel).await;
    let body = get_update(&app).await;
    assert_eq!(body["channel"]["kind"], "container");
    assert_eq!(body["channel"]["label"], "Docker (dhi image)");
    assert_eq!(body["channel"]["image"], "ghcr.io/devclinton/putio-sync");
    assert_eq!(body["channel"]["variant"], "dhi");
    let instruction = &body["available"]["instruction"];
    assert_eq!(
        instruction["commands"],
        json!(["docker pull ghcr.io/devclinton/putio-sync:dhi"])
    );
    assert_eq!(
        instruction["alternatives"],
        json!(["docker pull ghcr.io/devclinton/putio-sync:dhi-0.4.0"])
    );
    assert_eq!(
        instruction["steps"],
        "Recreate the container after the pull."
    );
    let status = body_json(app.get("/status").await).await;
    assert_eq!(status["install_channel"], "container");
}

// --- Update now (US3) ---

/// A GitHub-archive install of 0.3.0 in `dir` that has found the releases on `server`
async fn installed(server: &mockito::ServerGuard, dir: &std::path::Path, busy: usize) -> TestApp {
    let mut app = test_app().await;
    let exe = dir.join("putio-sync");
    std::fs::write(&exe, b"old").unwrap();
    let updates = UpdateService::for_tests(
        UpdatesConfig::default(),
        app.db.clone(),
        "0.3.0".parse().unwrap(),
        server.url(),
    )
    .with_channel(Channel::GithubArchive)
    .with_install(exe, dir.to_path_buf())
    .with_busy_source(move || Box::pin(async move { busy }));
    app.state = app.state.clone().with_updates(updates);
    app.router = router(app.state.clone());
    let res = app.post_json("/update/check", json!({})).await;
    assert_eq!(res.status(), StatusCode::OK);
    app
}

async fn apply(app: &TestApp, interrupt: bool) -> (StatusCode, Value) {
    let res = app
        .post_json("/update/apply", json!({ "interrupt": interrupt }))
        .await;
    (res.status(), body_json(res).await)
}

#[tokio::test]
async fn managed_channels_cannot_update_now() {
    let server = releases_server(RELEASES).await;
    let app = checked_on(&server, Channel::Snap).await;
    let (status, body) = apply(&app, false).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        body,
        json!({"error": "not_supported", "mode": "instructions"})
    );
}

#[tokio::test]
async fn update_now_needs_a_newer_release() {
    let server = releases_server(r#"[{"tag_name": "v0.3.0", "html_url": "u", "body": ""}]"#).await;
    let dir = tempfile::tempdir().unwrap();
    let app = installed(&server, dir.path(), 0).await;
    let (status, body) = apply(&app, false).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body, json!({"error": "no_update"}));
}

#[tokio::test]
async fn update_now_asks_before_interrupting_transfers() {
    let server = releases_server(RELEASES).await;
    let dir = tempfile::tempdir().unwrap();
    let app = installed(&server, dir.path(), 2).await;
    let body = get_update(&app).await;
    assert_eq!(body["busy"], json!({"active_transfers": 2}));
    assert_eq!(body["attempt"], Value::Null);
    assert_eq!(body["available"]["patch"], false);

    let (status, body) = apply(&app, false).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body, json!({"error": "busy", "active_transfers": 2}));

    let (status, body) = apply(&app, true).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["attempt"]["to"], "0.4.0");
    assert_eq!(body["attempt"]["state"], "downloading");

    // These releases have no assets, so the attempt fails with nothing replaced
    let mut attempt = Value::Null;
    for _ in 0..200 {
        attempt = get_update(&app).await["attempt"].clone();
        if attempt["state"] == "failed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(attempt["state"], "failed", "{attempt}");
    assert!(attempt["error"].as_str().unwrap().contains("SHA256SUMS"));
    assert_eq!(
        std::fs::read(dir.path().join("putio-sync")).unwrap(),
        b"old"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn update_now_names_a_folder_it_cannot_write() {
    use std::os::unix::fs::PermissionsExt;
    let server = releases_server(RELEASES).await;
    let dir = tempfile::tempdir().unwrap();
    let app = installed(&server, dir.path(), 0).await;
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let (status, body) = apply(&app, false).await;
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "not_writable");
    assert_eq!(body["path"], dir.path().to_str().unwrap());
}

#[tokio::test]
async fn the_notice_says_when_a_patch_exists() {
    let Some(target) = putio_sync_update::fetch::Target::current() else {
        return; // no release build for this platform
    };
    let to: putio_sync_update::version::Version = "0.4.0".parse().unwrap();
    let asset = |name: String, size: u64| json!({"name": name, "browser_download_url": format!("https://x/{size}"), "size": size});
    let releases = json!([{
        "tag_name": "v0.4.0", "html_url": "u", "body": "",
        "assets": [
            asset(target.full(to), 9_000_000),
            asset(target.patch("0.3.0".parse().unwrap(), to), 400_000),
        ],
    }]);
    let server = releases_server(&releases.to_string()).await;
    let dir = tempfile::tempdir().unwrap();
    let app = installed(&server, dir.path(), 0).await;
    let body = get_update(&app).await;
    assert_eq!(body["available"]["patch"], true);
    assert_eq!(body["available"]["download_bytes"], 400_000);
}
