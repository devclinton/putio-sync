//! "Fetch subtitles" on Local files (feature 025, US3): the preflight says whether Put.io can
//! still help, and the POST starts a `manual:<n>` pipeline item. Put.io is a mockito mock.

mod common;

use axum::http::StatusCode;
use common::{body_json, test_app_with, TestApp};
use putio_sync_db::models::DownloadStatus;
use serde_json::{json, Value};
use std::path::PathBuf;

async fn app_with_languages() -> TestApp {
    test_app_with(|c| c.subtitles.languages = vec!["en".to_string()]).await
}

fn root(app: &TestApp) -> PathBuf {
    app._dir.path().to_path_buf()
}

fn video(app: &TestApp, rel: &str) -> PathBuf {
    let p = root(app).join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, b"video").unwrap();
    p
}

async fn preflight(app: &TestApp, path: &str) -> (StatusCode, Value) {
    let r = app
        .get(&format!("/local/files/downloads/subtitles?path={path}"))
        .await;
    (r.status(), body_json(r).await)
}

async fn start(app: &TestApp, path: &str) -> (StatusCode, Value) {
    let r = app
        .post_json("/local/files/downloads/subtitles", json!({ "path": path }))
        .await;
    (r.status(), body_json(r).await)
}

async fn downloaded(app: &TestApp, id: i64, path: &std::path::Path) {
    app.db()
        .downloads()
        .insert(
            id,
            "Movie.mkv",
            5,
            &path.to_string_lossy(),
            DownloadStatus::Completed,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn preflight_says_available_when_put_io_still_has_the_file() {
    let mut app = app_with_languages().await;
    let v = video(&app, "Movies/Movie.mkv");
    downloaded(&app, 9, &v).await;
    app.mock_file(
        9,
        json!({"status":"OK","file":{"id":9,"name":"Movie.mkv","size":5,"file_type":"VIDEO",
               "parent_id":0,"is_folder":false,"is_shared":false}}),
    )
    .await;
    std::fs::write(root(&app).join("Movies/Movie.en.srt"), "x").unwrap();
    let (status, body) = preflight(&app, "Movies/Movie.mkv").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["putio"], "available");
    assert_eq!(body["eligible"], true);
    assert_eq!(body["languages"], json!(["en"]));
    assert_eq!(body["transcription"]["enabled"], false);
    assert_eq!(body["existing"][0]["name"], "Movie.en.srt");
    assert_eq!(body["existing"][0]["language"], "en");
}

#[tokio::test]
async fn preflight_says_gone_when_put_io_answers_404_and_unknown_without_a_download_record() {
    let mut app = app_with_languages().await;
    let v = video(&app, "Gone.mkv");
    downloaded(&app, 4, &v).await;
    let _m = app
        .putio
        .mock("GET", "/files/4")
        .with_status(404)
        .with_body(r#"{"status":"ERROR","error_type":"NOT_FOUND"}"#)
        .create_async()
        .await;
    let (_, body) = preflight(&app, "Gone.mkv").await;
    assert_eq!(body["putio"], "gone", "{body}");
    assert_eq!(body["eligible"], false);
    assert!(body["reason"].as_str().unwrap().contains("Put.io"));

    video(&app, "Other.mkv");
    let (_, body) = preflight(&app, "Other.mkv").await;
    assert_eq!(body["putio"], "unknown");
    assert_eq!(body["eligible"], false);
}

#[tokio::test]
async fn transcription_makes_a_file_put_io_no_longer_has_eligible() {
    let mut app = test_app_with(|c| {
        c.subtitles.languages = vec!["en".to_string()];
        c.subtitles.transcription.enabled = true;
        c.subtitles.transcription.url = "http://nas:8000".to_string();
        c.subtitles.transcription.ffmpeg_path = "/bin/sh".to_string();
    })
    .await;
    video(&app, "Other.mkv");
    let (_, body) = preflight(&app, "Other.mkv").await;
    assert_eq!(body["putio"], "unknown");
    assert_eq!(
        body["transcription"],
        json!({"enabled": true, "ffmpeg": true})
    );
    assert_eq!(body["eligible"], true, "{body}");
    let _ = &mut app;
}

#[tokio::test]
async fn not_a_video_no_languages_and_missing_files_are_refused() {
    let app = app_with_languages().await;
    video(&app, "notes.txt");
    let (status, body) = preflight(&app, "notes.txt").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["eligible"], false);
    assert!(body["reason"].as_str().unwrap().contains("video"));
    assert_eq!(preflight(&app, "nope.mkv").await.0, StatusCode::NOT_FOUND);

    let off = test_app_with(|_| {}).await;
    video(&off, "Movie.mkv");
    let (_, body) = preflight(&off, "Movie.mkv").await;
    assert_eq!(body["eligible"], false);
    assert!(body["reason"].as_str().unwrap().contains("language"));
}

#[tokio::test]
async fn paths_outside_the_root_are_a_400() {
    let app = app_with_languages().await;
    assert_eq!(
        preflight(&app, "../etc/passwd").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        start(&app, "../etc/passwd").await.0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn start_returns_202_with_a_key_and_a_second_start_is_409_with_the_same_key() {
    let mut app = app_with_languages().await;
    let pipeline = app.attach_pipeline().await;
    video(&app, "Movie.mkv");
    let (status, body) = start(&app, "Movie.mkv").await;
    assert_eq!(status, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["key"], "manual:1");
    let timeline = pipeline.timeline("manual:1").await.expect("an item");
    assert_eq!(timeline.state.as_str(), "running");

    let (status, again) = start(&app, "Movie.mkv").await;
    assert_eq!(status, StatusCode::CONFLICT, "{again}");
    assert_eq!(again["key"], "manual:1");
}

#[tokio::test]
async fn start_refuses_non_videos_missing_files_and_no_languages() {
    let mut app = app_with_languages().await;
    app.attach_pipeline().await;
    video(&app, "notes.txt");
    assert_eq!(start(&app, "notes.txt").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(start(&app, "nope.mkv").await.0, StatusCode::NOT_FOUND);

    let mut off = test_app_with(|_| {}).await;
    off.attach_pipeline().await;
    video(&off, "Movie.mkv");
    assert_eq!(start(&off, "Movie.mkv").await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn start_without_a_pipeline_is_a_503() {
    let app = app_with_languages().await;
    video(&app, "Movie.mkv");
    assert_eq!(
        start(&app, "Movie.mkv").await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn whisper_only_needs_no_put_io_and_download_only_needs_no_transcription() {
    use putio_sync_core::subtitles::SubtitleMode;
    let app = test_app_with(|c| {
        c.subtitles.languages = vec!["en".to_string()];
        c.subtitles.mode = Some(SubtitleMode::WhisperOnly);
        c.subtitles.transcription.enabled = true;
        c.subtitles.transcription.url = "http://nas:8000".to_string();
        c.subtitles.transcription.ffmpeg_path = "/bin/sh".to_string();
    })
    .await;
    video(&app, "Other.mkv");
    let (_, body) = preflight(&app, "Other.mkv").await;
    assert_eq!(body["mode"], "whisper_only");
    assert_eq!(body["eligible"], true, "{body}");

    // Download only: transcription being on does not make an unknown file eligible
    let app = test_app_with(|c| {
        c.subtitles.languages = vec!["en".to_string()];
        c.subtitles.mode = Some(SubtitleMode::DownloadOnly);
        c.subtitles.transcription.enabled = true;
        c.subtitles.transcription.url = "http://nas:8000".to_string();
        c.subtitles.transcription.ffmpeg_path = "/bin/sh".to_string();
    })
    .await;
    video(&app, "Other.mkv");
    let (_, body) = preflight(&app, "Other.mkv").await;
    assert_eq!(body["eligible"], false);
    assert!(!body["reason"].as_str().unwrap().contains("transcription"));
}
