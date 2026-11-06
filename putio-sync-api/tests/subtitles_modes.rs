//! Source modes in the subtitles settings (feature 050): the new fields come back and round
//! trip, a Whisper mode without transcription is refused, and the OpenSubtitles key and
//! password are masked and kept on a masked save.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, body_text, test_app, test_app_with};
use serde_json::{json, Value};

async fn put(app: &common::TestApp, body: Value) -> (StatusCode, Value) {
    let r = app
        .req(
            Method::PUT,
            "/settings/subtitles",
            Some(&app.token),
            Some(body),
        )
        .await;
    let status = r.status();
    (status, body_json(r).await)
}

#[tokio::test]
async fn an_untouched_config_reports_the_default_mode_and_threshold() {
    let app = test_app().await;
    let v = body_json(app.get("/settings/subtitles").await).await;
    assert_eq!(v["effective_mode"], "download_then_whisper");
    assert_eq!(v["effective_min_confidence"], 65);
    assert_eq!(v["opensubtitles_on"], false);
    assert!(v["mode"].is_null(), "{v}");
    assert_eq!(v["category_modes"], json!({}));
    assert_eq!(v["min_confidence"], 65);
}

#[tokio::test]
async fn modes_save_and_come_back() {
    let app = test_app().await;
    let (status, v) = put(
        &app,
        json!({
            "languages": ["en"],
            "mode": "download_only",
            "category_modes": {"Sonarr": "download_only"},
            "min_confidence": 80,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["mode"], "download_only");
    assert_eq!(v["category_modes"]["sonarr"], "download_only");
    assert_eq!(v["effective_min_confidence"], 80);
    let v = body_json(app.get("/settings/subtitles").await).await;
    assert_eq!(v["effective_mode"], "download_only");
}

#[tokio::test]
async fn a_whisper_mode_without_transcription_is_refused() {
    let app = test_app().await;
    let (status, v) = put(&app, json!({"languages": ["en"], "mode": "whisper_only"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert!(v.to_string().contains("transcription"), "{v}");
    let (status, _) = put(
        &app,
        json!({"languages": ["en"], "language_modes": {"es": "both"}}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = put(&app, json!({"languages": ["en"], "min_confidence": 101})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_opensubtitles_key_and_password_are_masked_and_kept() {
    const KEY: &str = "OSKEY0123456789";
    const PASS: &str = "OSPASS0123456789";
    let app = test_app_with(|c| {
        c.subtitles.languages = vec!["en".into()];
        c.subtitles.opensubtitles.api_key = KEY.into();
        c.subtitles.opensubtitles.username = "me".into();
        c.subtitles.opensubtitles.password = PASS.into();
    })
    .await;
    let text = body_text(app.get("/settings/subtitles").await).await;
    assert!(!text.contains(KEY) && !text.contains(PASS), "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["opensubtitles"]["api_key"], "********");
    assert_eq!(v["opensubtitles"]["password"], "********");
    assert_eq!(v["opensubtitles_on"], true);
    // Saving what the page was given keeps the stored values
    let (status, back) = put(&app, v).await;
    assert_eq!(status, StatusCode::OK, "{back}");
    assert!(!back.to_string().contains(KEY));
    let export = body_text(app.get("/settings/export").await).await;
    assert!(!export.contains(KEY) && !export.contains(PASS), "{export}");
}
