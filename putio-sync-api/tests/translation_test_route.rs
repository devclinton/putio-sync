//! `POST /settings/translation/test` (feature 026), chat mode: two sample lines go to the
//! chat server and the problem is named when it fails. The server is a mockito mock.

mod common;

use axum::http::StatusCode;
use common::{body_json, test_app, test_app_with, TestApp};
use mockito::Matcher;
use serde_json::{json, Value};

const KEY: &str = "chat-secret-key";

async fn test_with(app: &TestApp, chat: Value) -> (StatusCode, Value) {
    let response = app
        .post_json(
            "/settings/translation/test",
            json!({"translation": {"kind": "chat", "chat": chat}, "sample_target": "es"}),
        )
        .await;
    let status = response.status();
    (status, body_json(response).await)
}

fn completion(content: &str) -> String {
    json!({"choices": [{"message": {"content": content}, "finish_reason": "stop"}]}).to_string()
}

#[tokio::test]
async fn a_working_server_returns_the_sample() {
    let app = test_app().await;
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", format!("Bearer {KEY}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(completion("1| Buenos días.\n2| ¿Adónde vas?<br>Es tarde."))
        .expect(1)
        .create_async()
        .await;
    let (status, body) = test_with(
        &app,
        json!({"base_url": server.url(), "model": "m", "api_key": KEY}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true, "{body}");
    assert_eq!(body["kind"], "chat");
    assert!(body["problem"].is_null());
    assert_eq!(body["sample"][0]["translation"], "Buenos días.");
    assert_eq!(body["sample"][1]["translation"], "¿Adónde vas?\nEs tarde.");
    assert!(body["endpoint"]
        .as_str()
        .unwrap()
        .ends_with("/v1/chat/completions"));
    assert!(!body.to_string().contains(KEY));
    m.assert_async().await;
}

#[tokio::test]
async fn the_stored_key_is_used_for_a_masked_one() {
    let app = test_app().await;
    let mut server = mockito::Server::new_async().await;
    let response = app
        .req(
            axum::http::Method::PUT,
            "/settings/translation",
            Some(&app.token),
            Some(json!({"kind": "chat", "chat":
                {"base_url": server.url(), "model": "m", "api_key": KEY}})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", format!("Bearer {KEY}").as_str())
        .with_status(200)
        .with_body(completion("1| a\n2| b"))
        .expect(1)
        .create_async()
        .await;
    let response = app.post_json("/settings/translation/test", json!({})).await;
    let body = body_json(response).await;
    assert_eq!(body["ok"], true, "{body}");
}

#[tokio::test]
async fn problems_are_named() {
    let app = test_app().await;
    let cases: [(usize, &str, &str); 4] = [
        (401, "{\"error\":{\"message\":\"bad key\"}}", "key"),
        (
            404,
            "{\"error\":{\"message\":\"model 'm' not found\"}}",
            "model",
        ),
        (404, "not here", "url"),
        (500, "{\"error\":{\"message\":\"boom\"}}", "server"),
    ];
    for (status, reply, want) in cases {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/v1/chat/completions")
            .with_status(status)
            .with_body(reply)
            .create_async()
            .await;
        server
            .mock("GET", "/v1/models")
            .with_status(200)
            .with_body(json!({"data": [{"id": "llama3"}, {"id": "qwen"}]}).to_string())
            .create_async()
            .await;
        let (code, body) = test_with(
            &app,
            json!({"base_url": server.url(), "model": "m", "api_key": KEY}),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["ok"], false, "{body}");
        assert_eq!(body["problem"], want, "{body}");
        if want == "model" {
            assert_eq!(body["models"], json!(["llama3", "qwen"]));
        } else {
            assert!(body["models"].is_null());
        }
    }
}

#[tokio::test]
async fn a_reply_that_is_not_numbered_is_a_format_problem_with_the_raw_lines() {
    let app = test_app().await;
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_body(completion("1| Buenos días."))
        .create_async()
        .await;
    let (_, body) = test_with(&app, json!({"base_url": server.url(), "model": "m"})).await;
    assert_eq!(body["ok"], false);
    assert_eq!(body["problem"], "format", "{body}");
    assert_eq!(body["sample"][0]["translation"], "Buenos días.");
}

#[tokio::test]
async fn an_unreachable_server_and_invalid_settings() {
    let app = test_app().await;
    let (status, body) = test_with(
        &app,
        json!({"base_url": "http://127.0.0.1:9", "model": "m"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["problem"], "url", "{body}");

    let (status, body) = test_with(&app, json!({"base_url": "nope", "model": "m"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let response = app.post_json("/settings/translation/test", json!({})).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ---- callback mode ----

async fn test_callback(app: &TestApp, service: &str, base: &str) -> (StatusCode, Value) {
    let response = app
        .post_json(
            "/settings/translation/test",
            json!({"translation": {"kind": "callback", "callback": {
                "url": format!("{service}/translate"),
                "callback_base_url": base,
                "headers": {"X-Api-Key": KEY},
            }}, "sample_target": "es"}),
        )
        .await;
    let status = response.status();
    (status, body_json(response).await)
}

async fn healthy_base() -> mockito::ServerGuard {
    let mut s = mockito::Server::new_async().await;
    s.mock("GET", "/health")
        .with_status(200)
        .create_async()
        .await;
    s
}

#[tokio::test]
async fn a_service_that_answers_202_will_call_back_and_no_job_is_recorded() {
    let mut app = test_app().await;
    let pipeline = app.attach_pipeline().await;
    let mut svc = mockito::Server::new_async().await;
    let m = svc
        .mock("POST", "/translate")
        .match_header("x-api-key", KEY)
        .with_status(202)
        .expect(1)
        .create_async()
        .await;
    let base = healthy_base().await;
    let (status, body) = test_callback(&app, &svc.url(), &base.url()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true, "{body}");
    assert_eq!(body["result"], "will_call_back");
    assert_eq!(body["service"]["status"], 202);
    assert_eq!(body["base_url_reachable_from_here"]["status"], 200);
    assert!(pipeline.store().waiting_jobs().await.unwrap().is_empty());
    assert!(!body.to_string().contains(KEY));
    m.assert_async().await;
}

#[tokio::test]
async fn a_service_that_answers_200_with_a_subtitle_is_immediate() {
    let app = test_app().await;
    let mut svc = mockito::Server::new_async().await;
    svc.mock("POST", "/translate")
        .with_status(200)
        .with_body("1\n00:00:01,000 --> 00:00:03,000\nBuenos dias.\n\n2\n00:00:04,000 --> 00:00:06,000\nA donde vas?\n\n")
        .create_async()
        .await;
    let base = healthy_base().await;
    let (_, body) = test_callback(&app, &svc.url(), &base.url()).await;
    assert_eq!(body["ok"], true, "{body}");
    assert_eq!(body["result"], "immediate");
}

#[tokio::test]
async fn failures_are_named() {
    let app = test_app().await;
    let base = healthy_base().await;
    for (status, problem) in [(401, "headers"), (404, "url"), (500, "server")] {
        let mut svc = mockito::Server::new_async().await;
        svc.mock("POST", "/translate")
            .with_status(status)
            .create_async()
            .await;
        let (code, body) = test_callback(&app, &svc.url(), &base.url()).await;
        assert_eq!(code, StatusCode::OK, "{body}");
        assert_eq!(body["ok"], false);
        assert_eq!(body["problem"], problem, "{status}: {body}");
        assert_eq!(body["service"]["status"], status);
    }
}

#[tokio::test]
async fn an_unreachable_callback_address_is_reported() {
    let app = test_app().await;
    let mut svc = mockito::Server::new_async().await;
    svc.mock("POST", "/translate")
        .with_status(202)
        .create_async()
        .await;
    let (_, body) = test_callback(&app, &svc.url(), "http://127.0.0.1:9").await;
    assert_eq!(body["ok"], false, "{body}");
    assert_eq!(body["problem"], "callback_base_url");
    assert_eq!(body["base_url_reachable_from_here"]["checked"], true);
    assert!(body["base_url_reachable_from_here"]["status"].is_null());
}

#[tokio::test]
async fn a_callback_without_a_base_url_is_400() {
    let app = test_app().await;
    let response = app
        .post_json(
            "/settings/translation/test",
            json!({"translation": {"kind": "callback", "callback": {"url": "http://svc.lan/t"}}}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_saved_chat_key_is_not_sent_to_another_server() {
    let mut saved = mockito::Server::new_async().await;
    let mut other = mockito::Server::new_async().await;
    let never = other
        .mock("POST", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let saved_url = saved.url();
    let app = test_app_with(|config| {
        config.translation.kind = putio_sync_core::translation::TranslatorKind::Chat;
        config.translation.chat.base_url = saved_url.clone();
        config.translation.chat.model = "m".to_string();
        config.translation.chat.api_key = KEY.to_string();
    })
    .await;
    let (status, body) = test_with(
        &app,
        json!({"base_url": other.url(), "model": "m", "api_key": "********"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string().contains("Type the API key again"),
        "{body}"
    );
    never.assert_async().await;
    // The saved server with the mask works
    let hit = saved
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", format!("Bearer {KEY}").as_str())
        .with_status(200)
        .with_body(completion("1| a\n2| b"))
        .expect(1)
        .create_async()
        .await;
    let (status, body) = test_with(
        &app,
        json!({"base_url": saved_url, "model": "m", "api_key": "********"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true, "{body}");
    hit.assert_async().await;
}

#[tokio::test]
async fn a_saved_callback_header_is_not_sent_to_another_service() {
    let mut other = mockito::Server::new_async().await;
    let never = other
        .mock("POST", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let app = test_app_with(|config| {
        config.translation.kind = putio_sync_core::translation::TranslatorKind::Callback;
        config.translation.callback.url = "http://svc.lan/translate".to_string();
        config.translation.callback.callback_base_url = "http://127.0.0.1:9".to_string();
        config
            .translation
            .callback
            .headers
            .insert("Authorization".into(), "Bearer saved-header".into());
    })
    .await;
    let response = app
        .post_json(
            "/settings/translation/test",
            json!({"translation": {"kind": "callback", "callback": {
                "url": other.url(), "callback_base_url": "http://127.0.0.1:9",
                "headers": {"Authorization": "********"}}}}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    never.assert_async().await;
}

#[tokio::test]
async fn the_remote_error_body_is_not_echoed() {
    let app = test_app().await;
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(500)
        .with_body("{\"error\":{\"message\":\"internal-secret-detail\"}}")
        .create_async()
        .await;
    let (_, body) = test_with(&app, json!({"base_url": server.url(), "model": "m"})).await;
    assert_eq!(body["error"], "The server answered HTTP 500", "{body}");
    assert!(
        !body.to_string().contains("internal-secret-detail"),
        "{body}"
    );

    let mut svc = mockito::Server::new_async().await;
    svc.mock("POST", Matcher::Any)
        .with_status(500)
        .with_body("internal-secret-detail")
        .create_async()
        .await;
    let (_, body) = test_callback(&app, &svc.url(), "http://127.0.0.1:9").await;
    assert_eq!(body["error"], "The server answered HTTP 500", "{body}");
    assert!(
        !body.to_string().contains("internal-secret-detail"),
        "{body}"
    );
}
