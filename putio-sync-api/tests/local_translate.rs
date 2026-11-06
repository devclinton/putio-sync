//! "Translate subtitles" on Local files (feature 026, US3): the preflight says whether a
//! translator and a source subtitle exist, and the POST starts a `manual:<n>` pipeline item
//! with one translate step per language.

mod common;

use axum::http::StatusCode;
use common::{body_json, test_app_with, TestApp};
use putio_sync_core::translation::TranslatorKind;
use serde_json::{json, Value};

const SRT: &str = "1\n00:00:01,000 --> 00:00:02,000\nHello there\n\n";

async fn app(kind: TranslatorKind) -> TestApp {
    let mut app = test_app_with(move |c| {
        c.subtitles.languages = vec!["en".to_string(), "es".to_string()];
        c.translation.kind = kind;
        c.translation.chat.base_url = "http://127.0.0.1:9".to_string();
        c.translation.chat.model = "m".to_string();
    })
    .await;
    app.attach_pipeline().await;
    app
}

fn video(app: &TestApp, rel: &str, with_srt: bool) {
    let p = app._dir.path().join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, b"video").unwrap();
    if with_srt {
        std::fs::write(p.with_extension("en.srt"), SRT).unwrap();
    }
}

async fn preflight(app: &TestApp, path: &str) -> (StatusCode, Value) {
    let r = app
        .get(&format!("/local/files/downloads/translate?path={path}"))
        .await;
    (r.status(), body_json(r).await)
}

async fn start(app: &TestApp, body: Value) -> (StatusCode, Value) {
    let r = app
        .post_json("/local/files/downloads/translate", body)
        .await;
    (r.status(), body_json(r).await)
}

#[tokio::test]
async fn preflight_lists_what_exists_and_what_is_missing() {
    let app = app(TranslatorKind::Chat).await;
    video(&app, "Movies/Movie.mkv", true);
    let (status, body) = preflight(&app, "Movies/Movie.mkv").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["eligible"], true);
    assert_eq!(body["translator"], "chat");
    assert_eq!(body["missing"], json!(["es"]));
    assert_eq!(body["existing"][0]["language"], "en");
    assert_eq!(body["max_targets"], 10);
}

#[tokio::test]
async fn preflight_explains_why_not() {
    let off = app(TranslatorKind::None).await;
    video(&off, "A.mkv", true);
    let (_, body) = preflight(&off, "A.mkv").await;
    assert_eq!(body["eligible"], false);
    assert!(body["reason"].as_str().unwrap().contains("translator"));

    let on = app(TranslatorKind::Chat).await;
    video(&on, "B.mkv", false);
    let (_, body) = preflight(&on, "B.mkv").await;
    assert_eq!(body["eligible"], false);
    assert!(body["reason"].as_str().unwrap().contains("no subtitle"));

    assert_eq!(preflight(&on, "Nope.mkv").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn start_makes_a_manual_item_with_a_step_per_language_and_refuses_a_duplicate() {
    let app = app(TranslatorKind::Chat).await;
    video(&app, "Movie.mkv", true);
    let (status, body) = start(
        &app,
        json!({"path": "Movie.mkv", "targets": ["es", "fr", "es"]}),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    let key = body["key"].as_str().unwrap().to_string();
    assert!(key.starts_with("manual:"));
    let steps = app
        .state
        .pipeline
        .as_ref()
        .unwrap()
        .store()
        .steps(&key)
        .await
        .unwrap();
    let mut branches: Vec<_> = steps.iter().filter_map(|s| s.branch.clone()).collect();
    branches.sort();
    assert_eq!(branches, ["es", "fr"], "duplicates in the request collapse");
    let (status, dup) = start(&app, json!({"path": "Movie.mkv", "targets": ["de"]})).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(dup["key"], key);
}

#[tokio::test]
async fn start_validates_its_input() {
    let app = app(TranslatorKind::Chat).await;
    video(&app, "Movie.mkv", true);
    video(&app, "notes.txt", false);
    for (body, want) in [
        (
            json!({"path": "Movie.mkv", "targets": []}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"path": "Movie.mkv", "targets": ["klingon-xx"]}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"path": "Movie.mkv", "targets": ["es","fr","de","it","pt","ru","ja","ko","zh","ar","nl"]}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"path": "notes.txt", "targets": ["es"]}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"path": "Missing.mkv", "targets": ["es"]}),
            StatusCode::NOT_FOUND,
        ),
    ] {
        let (status, b) = start(&app, body.clone()).await;
        assert_eq!(status, want, "{body}: {b}");
    }
    let off = self::app(TranslatorKind::None).await;
    video(&off, "Movie.mkv", true);
    let (status, _) = start(&off, json!({"path": "Movie.mkv", "targets": ["es"]})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
