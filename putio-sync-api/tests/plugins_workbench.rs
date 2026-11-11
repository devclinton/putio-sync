//! Plugin workbench routes (feature 055): the overview, the editor setting, new, read,
//! check, save, copy and runs

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app_with, TestApp};
use serde_json::{json, Value};
use std::path::Path;

async fn app(dir: &Path, editor: bool) -> TestApp {
    let dir = dir.to_path_buf();
    test_app_with(move |config| {
        config.plugins_dir = Some(dir);
        config.plugin_editor = editor;
    })
    .await
}

async fn put_json(app: &TestApp, path: &str, body: Value) -> axum::response::Response {
    app.req(Method::PUT, path, Some(&app.token), Some(body))
        .await
}

#[tokio::test]
async fn needs_a_token() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path(), true).await;
    for (method, path) in [
        (Method::GET, "/plugins/workbench"),
        (Method::PUT, "/plugins/workbench/editor"),
        (Method::POST, "/plugins/workbench/new"),
        (Method::GET, "/plugins/workbench/source/ntfy"),
        (Method::POST, "/plugins/workbench/check"),
        (Method::PUT, "/plugins/workbench/source/ntfy"),
        (Method::POST, "/plugins/workbench/copy/ntfy"),
        (Method::POST, "/plugins/workbench/run"),
        (Method::GET, "/plugins/workbench/runs"),
        (Method::POST, "/plugins/workbench/runs/1"),
    ] {
        let response = app.req(method, path, None, None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn the_editor_is_off_by_default_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path(), false).await;
    let overview = body_json(app.get("/plugins/workbench").await).await;
    assert_eq!(overview["editor"], false);
    assert_eq!(overview["starters"].as_array().unwrap().len(), 3);
    let events = overview["events"].as_array().unwrap();
    assert!(events.iter().any(|e| e["type"] == "download.finished"));
    assert!(events.iter().all(|e| e["type"] != "plugin.test"));
    assert_eq!(overview["file"]["extension"], "mkv");

    let new = json!({"starter": "webhook-notifier", "name": "mine"});
    let response = app.post_json("/plugins/workbench/new", new.clone()).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = app
        .post_json("/plugins/workbench/copy/ntfy", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let unsaved = json!({
        "name": "ntfy", "call": {"kind": "tick"}, "manifest": "", "script": ""
    });
    let response = app.post_json("/plugins/workbench/run", unsaved).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());

    // Reading and checking need no editor
    let ntfy = body_json(app.get("/plugins/workbench/source/ntfy").await).await;
    assert_eq!(ntfy["source"], "bundled");
    assert_eq!(ntfy["editable"], false);
    let checked = app
        .post_json(
            "/plugins/workbench/check",
            json!({"manifest": "name = \"x\"\nversion = \"1\"\nevents = [\"*\"]\n", "script": "return {"}),
        )
        .await;
    let checked = body_json(checked).await;
    assert_eq!(checked["problems"][0]["file"], "main.lua");
    assert_eq!(checked["problems"][0]["line"], 1);

    // Turning it on is written to the config
    let response = put_json(&app, "/plugins/workbench/editor", json!({"enabled": true})).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(app.state.config.read().await.plugin_editor);
    let response = app.post_json("/plugins/workbench/new", new).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(dir.path().join("mine/main.lua").is_file());
}

#[tokio::test]
async fn new_edit_save_and_run() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path(), true).await;
    app.state.plugins.start();
    let new = json!({"starter": "store-counter", "name": "counter"});
    assert_eq!(
        app.post_json("/plugins/workbench/new", new.clone())
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.post_json("/plugins/workbench/new", new).await.status(),
        StatusCode::CONFLICT
    );
    let bad = json!({"starter": "store-counter", "name": "no spaces"});
    assert_eq!(
        app.post_json("/plugins/workbench/new", bad).await.status(),
        StatusCode::BAD_REQUEST
    );

    let source = body_json(app.get("/plugins/workbench/source/counter").await).await;
    assert_eq!(source["editable"], true);
    let files = source["files"].as_array().unwrap();
    let versions: serde_json::Map<String, Value> = files
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap().to_string(),
                f["version"].clone(),
            )
        })
        .collect();
    let script = files[1]["text"]
        .as_str()
        .unwrap()
        .replace("finished", "done");
    let save = json!({
        "manifest": files[0]["text"], "script": script, "versions": versions
    });
    let response = put_json(&app, "/plugins/workbench/source/counter", save.clone()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let saved = body_json(response).await;
    assert_eq!(saved["problems"], json!([]));
    assert!(dir.path().join("counter/main.lua.bak").is_file());
    let response = put_json(&app, "/plugins/workbench/source/counter", save).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);

    let run = json!({
        "name": "counter",
        "call": {"kind": "event", "event_type": "download.finished"}
    });
    let response = app.post_json("/plugins/workbench/run", run).await;
    assert_eq!(response.status(), StatusCode::OK);
    let run = body_json(response).await;
    assert_eq!(run["run"]["result"], "success", "{run}");
    assert!(run["run"]["logs"][0]["message"]
        .as_str()
        .unwrap()
        .ends_with("done"));
    assert_eq!(run["store_before"], json!({}));
    assert!(run["store_after"]["per_day"].is_object());

    let runs = body_json(app.get("/plugins/workbench/runs").await).await;
    let id = runs["runs"][0]["id"].as_u64().unwrap();
    let again = app
        .post_json(&format!("/plugins/workbench/runs/{id}"), json!({}))
        .await;
    assert_eq!(again.status(), StatusCode::OK);
    let missing = app
        .post_json("/plugins/workbench/runs/9999", json!({}))
        .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let unknown = app
        .post_json(
            "/plugins/workbench/run",
            json!({"name": "nope", "call": {"kind": "tick"}}),
        )
        .await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    // Bundled plugins are read-only until copied
    let response = put_json(
        &app,
        "/plugins/workbench/source/ntfy",
        json!({"manifest": "", "script": ""}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = app
        .post_json("/plugins/workbench/copy/ntfy", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let ntfy = body_json(app.get("/plugins/workbench/source/ntfy").await).await;
    assert_eq!(ntfy["source"], "folder");
    let response = app
        .post_json("/plugins/workbench/copy/ntfy", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
