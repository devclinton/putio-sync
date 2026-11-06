//! Configuration assistant routes (feature 015): group previews never write and agree with
//! the PUT, the assistant route explains every field, and export/import carry no
//! credentials.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, body_text, test_app, test_daemon_app, TestApp, API_TOKEN, PUTIO_TOKEN};
use putio_sync_core::config::Config;
use serde_json::{json, Value};

const GROUPS: &[&str] = &[
    "download",
    "watcher",
    "logging",
    "notifications",
    "cleanup",
    "storage",
    "precheck",
    "tailscale",
    "network",
    "hooks",
    "pipeline",
    "plugins",
    "putio",
    "subtitles",
];

async fn get_ok(app: &TestApp, path: &str) -> Value {
    let response = app.get(path).await;
    assert_eq!(response.status(), StatusCode::OK, "GET {path}");
    body_json(response).await
}

async fn post(app: &TestApp, path: &str, body: Value) -> (StatusCode, Value) {
    let response = app.post_json(path, body).await;
    let status = response.status();
    (status, body_json(response).await)
}

/// The group's PUT body as its GET returns it
async fn group_body(app: &TestApp, group: &str) -> Value {
    let body = get_ok(app, &format!("/settings/{group}")).await;
    if group == "subtitles" {
        let mut body = body;
        for extra in ["known_languages", "live", "applies_now", "ffmpeg"] {
            body.as_object_mut().unwrap().remove(extra);
        }
        body
    } else if group == "precheck" || group == "putio" {
        let mut body = body;
        body.as_object_mut().unwrap().remove("effective");
        body.as_object_mut().unwrap().remove("status");
        body
    } else {
        body
    }
}

fn config_text(app: &TestApp) -> String {
    let path = app.state.config_path.as_ref().expect("config path");
    std::fs::read_to_string(path.as_ref()).expect("config file")
}

fn saved(app: &TestApp) -> Config {
    let path = app.state.config_path.as_ref().expect("config path");
    Config::load(path.as_ref()).expect("config file loads")
}

fn assert_no_secrets(text: &str) {
    assert!(!text.contains(API_TOKEN), "API token in {text}");
    assert!(!text.contains(PUTIO_TOKEN), "Put.io token in {text}");
}

#[tokio::test]
async fn preview_lists_changes_and_writes_nothing() {
    let app = test_app().await;
    let before_file = config_text(&app);
    let mut body = group_body(&app, "download").await;
    let workers = body["workers"].as_u64().unwrap();
    body["workers"] = json!(workers + 3);

    let (status, preview) = post(&app, "/settings/download/preview", body).await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["valid"], true);
    assert_eq!(
        preview["changes"],
        json!([{"key": "workers", "before": workers, "after": workers + 3, "live": true}])
    );
    assert!(!preview["when"].as_str().unwrap().is_empty());
    assert_eq!(preview["risks"], json!([]));

    assert_eq!(config_text(&app), before_file, "the file is unchanged");
    assert_eq!(
        app.state.config.read().await.download.workers as u64,
        workers,
        "the running config is unchanged"
    );
}

#[tokio::test]
async fn preview_without_changes_is_empty_for_every_group() {
    let app = test_daemon_app(true).await;
    for group in GROUPS {
        let body = group_body(&app, group).await;
        let (status, preview) = post(&app, &format!("/settings/{group}/preview"), body).await;
        assert_eq!(status, StatusCode::OK, "{group}: {preview}");
        assert_eq!(preview["valid"], true, "{group}: {preview}");
        assert_eq!(preview["changes"], json!([]), "{group}");
    }
}

#[tokio::test]
async fn refused_preview_has_the_put_error() {
    let app = test_app().await;
    let mut body = group_body(&app, "watcher").await;
    body["enabled"] = json!(true);
    body["paths"] = json!(["/no/such/watch/folder/015"]);

    let (status, preview) = post(&app, "/settings/watcher/preview", body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["valid"], false);
    assert_eq!(preview["changes"], json!([]));

    let response = app
        .req(
            Method::PUT,
            "/settings/watcher",
            Some(&app.token),
            Some(body),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(preview["error"], body_json(response).await["error"]);
}

#[tokio::test]
async fn preview_shows_restart_fields() {
    let app = test_app().await;
    let mut body = group_body(&app, "download").await;
    body["target_directory"] = json!(app._dir.path().join("elsewhere"));

    let (_, preview) = post(&app, "/settings/download/preview", body).await;
    let change = &preview["changes"][0];
    assert_eq!(change["key"], "target_directory");
    assert_eq!(change["live"], false);
}

#[tokio::test]
async fn preview_shows_risks_of_the_candidate() {
    let app = test_app().await;
    let mut body = group_body(&app, "download").await;
    body["delete_after_download"] = json!(true);
    body["target_directory"] = json!(std::env::temp_dir().join("putio-sync-015-preview"));

    let (_, preview) = post(&app, "/settings/download/preview", body).await;
    let ids: Vec<&str> = preview["risks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"delete_temp_folder"), "{preview}");
    assert_eq!(preview["risks"][0]["level"], "danger");
}

#[tokio::test]
async fn malformed_preview_is_400_and_needs_the_token() {
    let app = test_app().await;
    let (status, response) = post(&app, "/settings/logging/preview", json!({"level": 3})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(response["error"].is_string());

    let body = group_body(&app, "logging").await;
    for (method, path, body) in [
        (
            Method::POST,
            "/settings/logging/preview",
            Some(body.clone()),
        ),
        (Method::GET, "/settings/assistant", None),
        (Method::GET, "/settings/export", None),
        (
            Method::POST,
            "/settings/import",
            Some(json!({"toml": "", "apply": false})),
        ),
    ] {
        let response = app.req(method, path, None, body).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn assistant_explains_every_field() {
    let app = test_daemon_app(true).await;
    let assistant = get_ok(&app, "/settings/assistant").await;
    for group in GROUPS {
        let body = group_body(&app, group).await;
        let help = &assistant["groups"][group];
        for field in body.as_object().unwrap().keys() {
            let affects = help["affects"][field].as_str().unwrap_or_default();
            assert!(!affects.is_empty(), "{group}.{field} has no affects text");
        }
        for live in help["live"].as_array().unwrap() {
            assert!(
                body.get(live.as_str().unwrap()).is_some(),
                "{group}: {live}"
            );
        }
        assert!(help["defaults"].is_object(), "{group} defaults");
    }
    assert!(!assistant["groups"]["watcher"]["live"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(assistant["risks"], json!([]));
    assert_no_secrets(&assistant.to_string());
}

#[tokio::test]
async fn assistant_live_fields_follow_the_engine() {
    let server = test_app().await;
    let assistant = get_ok(&server, "/settings/assistant").await;
    assert_eq!(assistant["groups"]["watcher"]["live"], json!([]));
    assert_eq!(
        assistant["groups"]["cleanup"]["live"],
        json!(["min_age_hours", "nested", "keep_patterns"])
    );
}

#[tokio::test]
async fn assistant_defaults_are_the_default_config() {
    let app = test_app().await;
    let mut logging = group_body(&app, "logging").await;
    logging["level"] = json!("trace");
    let response = app
        .req(
            Method::PUT,
            "/settings/logging",
            Some(&app.token),
            Some(logging),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let assistant = get_ok(&app, "/settings/assistant").await;
    let defaults = &assistant["groups"]["logging"]["defaults"];
    assert_eq!(defaults["level"], "info");
    assert!(assistant["groups"]["precheck"]["defaults"]
        .get("effective")
        .is_none());
    let download = &assistant["groups"]["download"]["defaults"];
    assert_eq!(
        download["target_directory"],
        json!(app._dir.path()),
        "the download folder is kept"
    );
    assert_eq!(download["workers"], 2);

    // The defaults are a valid body for the preview
    let (status, preview) = post(&app, "/settings/logging/preview", defaults.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["valid"], true);
    assert_eq!(preview["changes"][0]["key"], "level");
}

#[tokio::test]
async fn export_has_no_secrets() {
    let app = test_app().await;
    let response = app.get("/settings/export").await;
    assert_eq!(response.status(), StatusCode::OK);
    let disposition = response.headers()["content-disposition"].to_str().unwrap();
    assert!(disposition.contains("putio-sync-settings.toml"));
    assert!(response.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/plain"));
    let text = body_text(response).await;
    assert_no_secrets(&text);
    assert!(
        !text
            .lines()
            .any(|l| l.trim_start().starts_with("auth_token")),
        "{text}"
    );
    let tables: Vec<&str> = text.lines().filter(|l| l.starts_with('[')).collect();
    assert!(
        !tables
            .iter()
            .any(|t| t.starts_with("[auth") || t.starts_with("[arr")),
        "{tables:?}"
    );
    assert!(text.contains("[download]"));
}

#[tokio::test]
async fn import_previews_then_applies() {
    let app = test_app().await;
    let exported = body_text(app.get("/settings/export").await).await;
    let workers = app.state.config.read().await.download.workers;
    let mut download = group_body(&app, "download").await;
    download["workers"] = json!(workers + 4);
    let response = app
        .req(
            Method::PUT,
            "/settings/download",
            Some(&app.token),
            Some(download),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    // A comment in the file survives the import
    let path = app.state.config_path.as_ref().unwrap().to_path_buf();
    let with_comment = format!("# kept by 015\n{}", config_text(&app));
    std::fs::write(&path, with_comment).unwrap();
    let before_file = config_text(&app);

    let (status, preview) = post(
        &app,
        "/settings/import",
        json!({"toml": exported, "apply": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(preview["valid"], true);
    assert_eq!(
        preview["changes"],
        json!([{"key": "download.workers", "before": workers + 4, "after": workers, "live": true}])
    );
    assert_eq!(preview["unknown"], json!([]));
    assert!(preview.get("message").is_none());
    assert_eq!(config_text(&app), before_file, "a preview writes nothing");

    let (status, applied) = post(
        &app,
        "/settings/import",
        json!({"toml": exported, "apply": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["message"], "Imported 1 change.");
    assert_eq!(app.state.config.read().await.download.workers, workers);
    let file = config_text(&app);
    assert!(file.starts_with("# kept by 015"), "{file}");
    let on_disk = saved(&app);
    assert_eq!(on_disk.download.workers, workers);
    assert!(on_disk.auth.is_some(), "the file keeps its credentials");
}

#[tokio::test]
async fn import_ignores_credentials_and_reports_unknown_keys() {
    let app = test_app().await;
    let text = "[auth]\noauth_token = \"other\"\n\n[api]\nauth_token = \"other\"\n\n\
                [logging]\nlevel = \"debug\"\nbogus_key = 1\n";
    let (status, preview) = post(
        &app,
        "/settings/import",
        json!({"toml": text, "apply": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let ignored: Vec<&str> = preview["ignored"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(ignored.contains(&"auth") && ignored.contains(&"api.auth_token"));
    assert_eq!(preview["unknown"][0]["key"], "logging.bogus_key");
    assert_eq!(preview["changes"][0]["key"], "logging.level");
    assert_no_secrets(&preview.to_string());

    let config = app.state.config.read().await;
    assert_eq!(config.logging.level, "debug");
    assert_eq!(config.api.auth_token.as_deref(), Some(API_TOKEN));
}

#[tokio::test]
async fn refused_import_writes_nothing() {
    let app = test_app().await;
    let before_file = config_text(&app);
    let text = "[watch]\nenabled = true\npaths = [\"/no/such/watch/folder/015\"]\n";

    let (status, preview) = post(
        &app,
        "/settings/import",
        json!({"toml": text, "apply": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["valid"], false);
    assert!(preview["error"].is_string());

    let (status, response) = post(
        &app,
        "/settings/import",
        json!({"toml": text, "apply": true}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(response["error"], preview["error"]);
    assert_eq!(config_text(&app), before_file);
    assert!(!app.state.config.read().await.watch.enabled);
}

#[tokio::test]
async fn invalid_or_oversized_import_is_400() {
    let app = test_app().await;
    let (status, response) = post(
        &app,
        "/settings/import",
        json!({"toml": "[download\nworkers = \"secret-value\"", "apply": false}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let error = response["error"].as_str().unwrap();
    assert!(error.contains("line 1"), "{error}");
    assert!(!error.contains("secret-value"), "{error}");

    let big = format!("# {}\n", "x".repeat(1024 * 1024 + 1));
    let (status, response) = post(
        &app,
        "/settings/import",
        json!({"toml": big, "apply": false}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(response["error"].as_str().unwrap().contains("1 MB"));
}

#[tokio::test]
async fn import_with_a_bad_pattern_is_refused() {
    let app = test_app().await;
    let before_file = config_text(&app);
    let text = "[download]\nwhitelist_patterns = \"*.{mkv\"\n";
    let (status, response) = post(
        &app,
        "/settings/import",
        json!({"toml": text, "apply": true}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    let error = response["error"].as_str().unwrap();
    assert!(
        error.contains("whitelist_patterns (Only download): unbalanced { }"),
        "{error}"
    );
    assert_eq!(config_text(&app), before_file);
}

#[tokio::test]
async fn plugin_secrets_are_masked_in_export_and_kept_on_import() {
    let app = test_app().await;
    let body = json!({ "plugins_dir": null, "plugins": [{
        "name": "ntfy", "enabled": true,
        "config": { "topic": "nas" },
        "secrets": { "token": "plugin-export-secret" }
    }]});
    let response = app
        .req(
            Method::PUT,
            "/settings/plugins",
            Some(&app.token),
            Some(body),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let exported = body_text(app.get("/settings/export").await).await;
    assert!(!exported.contains("plugin-export-secret"), "{exported}");
    assert!(exported.contains("[[plugins]]"), "{exported}");
    assert!(exported.contains("\"********\""), "{exported}");

    let edited = exported.replace("topic = \"nas\"", "topic = \"other\"");
    let (status, applied) = post(
        &app,
        "/settings/import",
        json!({"toml": edited, "apply": true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{applied}");
    assert_eq!(applied["message"], "Imported 1 change.");
    assert!(!applied.to_string().contains("plugin-export-secret"));
    let plugins = saved(&app).plugins;
    assert_eq!(plugins[0].config["topic"], "other");
    assert_eq!(plugins[0].secrets["token"], "plugin-export-secret");
    let ntfy = app
        .state
        .plugins
        .status()
        .into_iter()
        .find(|p| p.name == "ntfy")
        .unwrap();
    assert_eq!(ntfy.state, putio_sync_plugins::PluginState::Enabled);
}
