//! Event hook routes (feature 027): the hook list, test events and the event schema

mod common;

use axum::http::{header, Method, StatusCode};
use common::{body_json, body_text, test_app_with, TestApp};
use putio_sync_core::hooks::HookConfig;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn command_hook(name: &str, program: &Path) -> HookConfig {
    HookConfig {
        name: name.to_string(),
        events: vec!["download.finished".to_string()],
        command: Some(vec![program.display().to_string()]),
        timeout_secs: 5,
        ..Default::default()
    }
}

async fn app_with_hooks(hooks: Vec<HookConfig>) -> TestApp {
    let app = test_app_with(move |config| config.hooks = hooks).await;
    app.state.hooks.start();
    app
}

#[tokio::test]
async fn needs_a_token() {
    let app = app_with_hooks(Vec::new()).await;
    for (method, path) in [
        (Method::GET, "/hooks"),
        (Method::POST, "/hooks/test"),
        (Method::GET, "/events/schema"),
    ] {
        let response = app.req(method, path, None, None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn schema_is_served_as_json_schema() {
    let app = app_with_hooks(Vec::new()).await;
    let response = app.get("/events/schema").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "application/schema+json"
    );
    let schema: Value = serde_json::from_str(&body_text(response).await).unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
}

#[tokio::test]
async fn test_event_runs_the_hook_and_shows_in_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("event.json");
    let ok = script(dir.path(), "ok.sh", &format!("cat > {}", out.display()));
    let bad = script(dir.path(), "bad.sh", "echo nope >&2; exit 3");
    let app = app_with_hooks(vec![command_hook("ok", &ok), command_hook("bad", &bad)]).await;

    let response = app.post_json("/hooks/test", json!({ "name": "ok" })).await;
    assert_eq!(response.status(), StatusCode::OK);
    let run = body_json(response).await;
    assert_eq!(run["result"], "success");
    assert_eq!(run["event_type"], "hook.test");
    let event: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(event["type"], "hook.test");
    assert_eq!(event["data"]["hook"], "ok");

    // A failing hook still answers 200, with the failure in the run
    let response = app.post_json("/hooks/test", json!({ "name": "bad" })).await;
    assert_eq!(response.status(), StatusCode::OK);
    let run = body_json(response).await;
    assert_eq!(run["result"], "failure");
    assert_eq!(run["exit_code"], 3);
    assert!(run["stderr"].as_str().unwrap().contains("nope"));

    let list = body_json(app.get("/hooks").await).await;
    let hooks = list["hooks"].as_array().unwrap();
    assert_eq!(hooks.len(), 2);
    let ok_status = hooks.iter().find(|h| h["name"] == "ok").unwrap();
    assert_eq!(ok_status["kind"], "command");
    assert_eq!(ok_status["target"], ok.display().to_string());
    assert_eq!(ok_status["events"], json!(["download.finished"]));
    assert_eq!(ok_status["runs"][0]["result"], "success");
}

#[tokio::test]
async fn unknown_hook_is_404_and_a_second_test_is_409() {
    let dir = tempfile::tempdir().unwrap();
    let slow = script(dir.path(), "slow.sh", "sleep 2");
    let app = app_with_hooks(vec![command_hook("slow", &slow)]).await;

    let response = app
        .post_json("/hooks/test", json!({ "name": "nope" }))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let first = app.post_json("/hooks/test", json!({ "name": "slow" }));
    let second = async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        app.post_json("/hooks/test", json!({ "name": "slow" }))
            .await
    };
    let (first, second) = tokio::join!(first, second);
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn url_hooks_show_a_redacted_target() {
    let app = app_with_hooks(vec![HookConfig {
        name: "ha".to_string(),
        events: vec!["*".to_string()],
        url: Some("https://ha.local:8123/api/webhook/abc?token=s3cret".to_string()),
        headers: [("Authorization".to_string(), "Bearer hdr-secret".to_string())].into(),
        ..Default::default()
    }])
    .await;
    let response = app.get("/hooks").await;
    let text = body_text(response).await;
    assert!(text.contains("https://ha.local:8123/api/…"), "{text}");
    assert!(!text.contains("s3cret") && !text.contains("hdr-secret") && !text.contains("abc"));
}
