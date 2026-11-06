//! `POST /settings/subtitles/test` (feature 025): a short tone goes to the transcription
//! server and ffmpeg is looked for. A finished test is always 200; a bad URL is 400.
//! The server is a mockito mock; ffmpeg is a fake script.

mod common;

use axum::http::StatusCode;
use common::{body_json, test_app, TestApp};
use mockito::Matcher;
use serde_json::{json, Value};

const KEY: &str = "whisper-secret-key";

async fn test_with(app: &TestApp, transcription: Value) -> (StatusCode, Value) {
    let response = app
        .post_json(
            "/settings/subtitles/test",
            json!({ "transcription": transcription }),
        )
        .await;
    let status = response.status();
    (status, body_json(response).await)
}

#[tokio::test]
async fn a_working_server_is_ok_and_ffmpeg_missing_is_reported() {
    let app = test_app().await;
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("POST", "/v1/audio/transcriptions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({"language": "en", "text": "hello there",
                          "segments": [{"start": 0.0, "end": 1.0, "text": "hello there"}]})
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let (status, body) = test_with(
        &app,
        json!({"enabled": true, "url": server.url(), "ffmpeg_path": "/nonexistent/ffmpeg"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true, "{body}");
    assert_eq!(body["server"]["status"], 200);
    assert_eq!(body["server"]["language"], "en");
    assert_eq!(body["server"]["text"], "hello there");
    assert_eq!(body["ffmpeg"]["found"], false);
    m.assert_async().await;
}

#[tokio::test]
async fn an_empty_transcript_is_still_ok() {
    let app = test_app().await;
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({"language": "en", "text": "", "segments": []}).to_string())
        .create_async()
        .await;
    let (status, body) = test_with(&app, json!({"enabled": true, "url": server.url()})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true, "{body}");
    assert_eq!(body["server"]["text"], "");
}

#[tokio::test]
async fn a_server_error_is_reported_with_status_and_scrubbed_message() {
    let app = test_app().await;
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(401)
        .with_body(format!("bad key {KEY}"))
        .create_async()
        .await;
    let (status, body) = test_with(
        &app,
        json!({"enabled": true, "url": server.url(), "api_key": KEY}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], false);
    assert_eq!(body["server"]["status"], 401);
    let error = body["server"]["error"].as_str().unwrap();
    assert!(error.starts_with("HTTP 401"), "{error}");
    assert!(!body.to_string().contains(KEY), "{body}");
}

#[tokio::test]
async fn an_unreachable_server_is_a_failed_test_not_an_http_error() {
    let app = test_app().await;
    let (status, body) =
        test_with(&app, json!({"enabled": true, "url": "http://127.0.0.1:1"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], false);
    assert!(body["server"]["status"].is_null());
    assert!(!body["server"]["error"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn a_missing_or_invalid_url_is_a_400() {
    let app = test_app().await;
    let (status, body) = test_with(&app, json!({"enabled": true, "url": ""})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, _) = test_with(&app, json!({"enabled": true, "url": "ftp://x"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_masked_key_means_the_stored_key_and_never_comes_back() {
    let app = test_app().await;
    {
        let mut config = app.state.config.write().await;
        config.subtitles.transcription.api_key = KEY.to_string();
    }
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("POST", Matcher::Any)
        .match_header("authorization", format!("Bearer {KEY}").as_str())
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({"language": "en", "text": "x", "segments": []}).to_string())
        .expect(1)
        .create_async()
        .await;
    let (status, body) = test_with(
        &app,
        json!({"enabled": true, "url": server.url(), "api_key": "********"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], true);
    assert!(!body.to_string().contains(KEY), "{body}");
    m.assert_async().await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_found_ffmpeg_reports_its_path_and_version() {
    use std::os::unix::fs::PermissionsExt;
    let app = test_app().await;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("ffmpeg");
    std::fs::write(
        &script,
        "#!/bin/sh\necho 'ffmpeg version 9.9.9 Copyright'\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({"language": "en", "text": "x", "segments": []}).to_string())
        .create_async()
        .await;
    let (status, body) = test_with(
        &app,
        json!({"enabled": true, "url": server.url(),
               "ffmpeg_path": script.to_string_lossy()}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ffmpeg"]["found"], true);
    assert_eq!(body["ffmpeg"]["version"], "9.9.9");
    assert_eq!(body["ffmpeg"]["path"], script.to_string_lossy().as_ref());
}
