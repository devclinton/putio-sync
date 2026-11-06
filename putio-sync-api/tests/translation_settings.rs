//! `/settings/translation` (feature 026): validation, masking, applying now and the
//! settings listing.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, TestApp};
use putio_sync_core::config::Config;
use serde_json::{json, Value};

async fn get_ok(app: &TestApp, path: &str) -> Value {
    let response = app.get(path).await;
    assert_eq!(response.status(), StatusCode::OK, "GET {path}");
    body_json(response).await
}

async fn put(app: &TestApp, body: Value) -> (StatusCode, Value) {
    let response = app
        .req(
            Method::PUT,
            "/settings/translation",
            Some(&app.token),
            Some(body),
        )
        .await;
    let status = response.status();
    (status, body_json(response).await)
}

fn saved(app: &TestApp) -> Config {
    let path = app.state.config_path.as_ref().expect("config path").clone();
    Config::load(path.as_ref()).expect("config file loads")
}

fn chat_body(key: &str) -> Value {
    json!({
        "kind": "chat",
        "chat": {"base_url": "http://ollama.lan:11434/v1", "model": "qwen2.5:14b", "api_key": key},
    })
}

#[tokio::test]
async fn defaults_are_off() {
    let app = test_app().await;
    let view = get_ok(&app, "/settings/translation").await;
    assert_eq!(view["kind"], "none");
    assert_eq!(view["max_in_flight"], 2);
    assert_eq!(view["timeout_secs"], 180);
    assert_eq!(view["chunk_cues"], 40);
    assert_eq!(view["callback"]["timeout_secs"], 3600);
    assert_eq!(view["chat"]["api_key"], "");
    assert!(view["resolved_endpoint"].is_null());
    assert!(view["callback_url_form"].is_null());
    assert_eq!(view["target_languages"], json!([]));
    assert!(view["live"].as_array().unwrap().len() >= 5);
    assert!(view["applies_now"].as_str().unwrap().contains("waiting"));
}

#[tokio::test]
async fn a_chat_save_masks_the_key_resolves_the_endpoint_and_applies_now() {
    let app = test_app().await;
    let (status, body) = put(&app, chat_body("chat-secret-key")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("Applies now"),
        "{body}"
    );
    assert!(!body.to_string().contains("chat-secret-key"), "{body}");
    assert_eq!(
        body["resolved_endpoint"],
        "http://ollama.lan:11434/v1/chat/completions"
    );
    // No wanted language: the save goes through with a note
    assert!(body["warnings"][0]
        .as_str()
        .unwrap()
        .contains("nothing will be translated"));

    let view = get_ok(&app, "/settings/translation").await;
    assert_eq!(view["chat"]["api_key"], "********");
    assert_eq!(saved(&app).translation.chat.api_key, "chat-secret-key");
    assert_eq!(saved(&app).translation.chat.model, "qwen2.5:14b");

    // Sending the mask back keeps the stored key
    let mut again = view.clone();
    again["chat"]["model"] = json!("llama3");
    let (status, _) = put(&app, again).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved(&app).translation.chat.api_key, "chat-secret-key");
    assert_eq!(saved(&app).translation.chat.model, "llama3");
}

#[tokio::test]
async fn wanted_languages_are_the_targets_and_clear_the_warning() {
    let app = test_app().await;
    let response = app
        .req(
            Method::PUT,
            "/settings/subtitles",
            Some(&app.token),
            Some(json!({"languages": ["en", "es"]})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let (_, body) = put(&app, chat_body("")).await;
    assert_eq!(body["target_languages"], json!(["en", "es"]));
    assert!(body.get("warnings").is_none(), "{body}");
}

#[tokio::test]
async fn invalid_settings_change_nothing() {
    let app = test_app().await;
    let (status, _) = put(&app, chat_body("k")).await;
    assert_eq!(status, StatusCode::OK);
    for bad in [
        json!({"kind": "chat", "chat": {"base_url": "ftp://x", "model": "m"}}),
        json!({"kind": "chat", "chat": {"base_url": "http://x:1/v1", "model": ""}}),
        json!({"kind": "chat", "chat": {"base_url": "http://x:1/v1", "model": "m"}, "chunk_cues": 4}),
        json!({"kind": "chat", "chat": {"base_url": "http://x:1/v1", "model": "m"}, "max_in_flight": 9}),
        json!({"kind": "callback", "callback": {"url": "http://svc/t"}}),
        json!({"kind": "callback", "callback": {"url": "http://svc/t",
               "callback_base_url": "http://nas:8088", "timeout_secs": 5}}),
        json!({"kind": "callback", "callback": {"url": "http://svc/t",
               "callback_base_url": "http://nas:8088", "headers": {"bad name": "v"}}}),
        json!({"kind": "callback", "callback": {"url": "http://svc/t",
               "callback_base_url": "http://nas:8088", "headers": {"X-Key": "********"}}}),
        json!({"max_in_flight": 2, "instruction": "x".repeat(501)}),
    ] {
        let (status, body) = put(&app, bad.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad} -> {body}");
        assert!(body["error"].is_string(), "{body}");
    }
    assert_eq!(saved(&app).translation.chat.model, "qwen2.5:14b");
}

#[tokio::test]
async fn callback_headers_are_masked_and_kept_by_name() {
    let app = test_app().await;
    let (status, body) = put(
        &app,
        json!({"kind": "callback", "callback": {
            "url": "http://svc.lan/translate", "callback_base_url": "http://nas.lan:8088/",
            "headers": {"Authorization": "Bearer top-secret-header"}}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.to_string().contains("top-secret-header"), "{body}");
    assert_eq!(body["callback"]["headers"]["Authorization"], "********");
    assert_eq!(
        body["callback_url_form"],
        "http://nas.lan:8088/translate/callback/<token>"
    );
    assert_eq!(
        saved(&app).translation.callback.callback_base_url,
        "http://nas.lan:8088"
    );

    let view = get_ok(&app, "/settings/translation").await;
    let (status, _) = put(&app, view).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        saved(&app).translation.callback.headers["Authorization"],
        "Bearer top-secret-header"
    );
}

#[tokio::test]
async fn the_preview_lists_changes_without_saving() {
    let app = test_app().await;
    let response = app
        .post_json("/settings/translation/preview", chat_body("k"))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["valid"], true, "{body}");
    assert!(body["changes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["key"] == "kind"));
    assert_eq!(saved(&app).translation.kind.as_str(), "none");
}

#[tokio::test]
async fn the_group_is_listed_in_the_settings_overview() {
    let app = test_app().await;
    let export = common::body_text(app.get("/settings/export").await).await;
    assert!(export.contains("translation"), "{export}");
    let assistant = get_ok(&app, "/settings/assistant").await;
    assert!(assistant.to_string().contains("translation"), "{assistant}");
}
