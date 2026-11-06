//! The transcription pass of the engine (feature 025), with a fake ffmpeg and a mock server.
//! Unix only: the fakes are shell scripts. No real ffmpeg or Whisper server is used.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mockito::Matcher;
use putio_sync_core::subtitles::TranscriptionConfig;
use putio_sync_subtitles::engine::{self, EngineError, Outcome, VideoRequest};
use putio_sync_subtitles::modes::ModePlan;

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// A fake ffmpeg and ffprobe (1500 s of video) in their own folder
fn tools(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    script(&bin, "ffprobe", "echo 1500");
    script(
        &bin,
        "ffmpeg",
        "for last; do :; done\nhead -c 4000 /dev/zero > \"$last\"",
    )
}

fn config(url: &str, ffmpeg: &Path) -> TranscriptionConfig {
    TranscriptionConfig {
        enabled: true,
        url: url.to_string(),
        ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
        ..Default::default()
    }
}

fn video(dir: &Path) -> PathBuf {
    let v = dir.join("Movie.2024.mkv");
    std::fs::write(&v, b"video").unwrap();
    v
}

fn answer(language: &str, text: &str) -> String {
    format!(r#"{{"language":"{language}","segments":[{{"start":1.0,"end":3.0,"text":"{text}"}}]}}"#)
}

fn none(_: f64) {}

#[test]
fn it_runs_only_when_on_and_nothing_wanted_exists() {
    let cfg = TranscriptionConfig {
        enabled: true,
        ..Default::default()
    };
    let langs = vec!["en".to_string()];
    let modes = ModePlan::legacy(&langs);
    assert!(engine::wants_transcription(
        &cfg,
        &modes,
        &Outcome::default(),
        &langs
    ));
    let mut found = Outcome::default();
    found.present.push("en".into());
    assert!(
        !engine::wants_transcription(&cfg, &modes, &found, &langs),
        "a wanted subtitle exists"
    );
    let off = TranscriptionConfig::default();
    assert!(
        !engine::wants_transcription(&off, &modes, &Outcome::default(), &langs),
        "off"
    );
    assert!(
        !engine::wants_transcription(&cfg, &ModePlan::legacy(&[]), &Outcome::default(), &[]),
        "no languages"
    );
}

#[tokio::test]
async fn chunks_join_into_one_file_in_the_detected_language_even_if_unwanted() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = tools(dir.path());
    let v = video(dir.path());
    let hints: Arc<Mutex<Vec<bool>>> = Arc::default();
    let seen = hints.clone();
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("POST", "/v1/audio/transcriptions")
        .match_request(move |req| {
            let body = String::from_utf8_lossy(req.body().unwrap()).into_owned();
            seen.lock()
                .unwrap()
                .push(body.contains("name=\"language\"\r\n\r\nes"));
            true
        })
        .with_status(200)
        .with_body(answer("spanish", "Hola"))
        .expect(3)
        .create_async()
        .await;

    let langs = vec!["en".to_string()];
    let modes = ModePlan::legacy(&langs);
    let req = VideoRequest::new(&v, None, &langs, &modes);
    let mut out = Outcome::default();
    out.missing.push("en".into());
    let ticks = Mutex::new(Vec::new());
    let progress = |p: f64| ticks.lock().unwrap().push(p);
    engine::transcribe_video(&req, &config(&server.url(), &ffmpeg), &progress, &mut out)
        .await
        .unwrap();
    m.assert_async().await;

    // Chunk 1 had no hint; the other two reuse its language
    assert_eq!(*hints.lock().unwrap(), [false, true, true]);
    assert_eq!(*ticks.lock().unwrap(), [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]);

    let saved = &out.saved[0];
    assert_eq!(saved.language, "es");
    assert_eq!(saved.source, "transcribed");
    assert!(!saved.wanted);
    assert_eq!(saved.path, dir.path().join("Movie.2024.es.srt"));
    assert_eq!(out.missing, ["en"], "the wanted language is still missing");
    let text = std::fs::read_to_string(&saved.path).unwrap();
    assert!(
        text.contains("00:00:01,000 --> 00:00:03,000\nHola\n"),
        "{text}"
    );
    assert!(
        text.contains("00:10:01,000 --> 00:10:03,000"),
        "second chunk shifted: {text}"
    );
    assert!(
        text.contains("00:20:01,000 --> 00:20:03,000"),
        "third chunk shifted: {text}"
    );
    assert!(text.contains("\n3\n00:20:01"), "renumbered: {text}");
}

#[tokio::test]
async fn a_wanted_language_clears_it_from_missing() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = tools(dir.path());
    let v = video(dir.path());
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_body(answer("en", "Hi"))
        .create_async()
        .await;
    let langs = vec!["en".to_string()];
    let modes = ModePlan::legacy(&langs);
    let req = VideoRequest::new(&v, None, &langs, &modes);
    let mut out = Outcome::default();
    out.missing.push("en".into());
    engine::transcribe_video(&req, &config(&server.url(), &ffmpeg), &none, &mut out)
        .await
        .unwrap();
    assert!(out.missing.is_empty());
    assert!(out.saved[0].wanted);
    assert!(
        out.message().contains("en: transcribed"),
        "{}",
        out.message()
    );
}

#[tokio::test]
async fn an_existing_file_is_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = tools(dir.path());
    let v = video(dir.path());
    let existing = dir.path().join("Movie.2024.es.srt");
    std::fs::write(&existing, "mine").unwrap();
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_body(answer("es", "Hola"))
        .create_async()
        .await;
    let langs = vec!["en".to_string()];
    let modes = ModePlan::legacy(&langs);
    let req = VideoRequest::new(&v, None, &langs, &modes);
    let mut out = Outcome::default();
    engine::transcribe_video(&req, &config(&server.url(), &ffmpeg), &none, &mut out)
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "mine");
    assert!(out.saved.is_empty());
    assert_eq!(out.warnings.len(), 1);
}

#[tokio::test]
async fn silence_saves_nothing_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = tools(dir.path());
    let v = video(dir.path());
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_body(r#"{"language":"en","text":"","segments":[]}"#)
        .create_async()
        .await;
    let langs = vec!["en".to_string()];
    let modes = ModePlan::legacy(&langs);
    let req = VideoRequest::new(&v, None, &langs, &modes);
    let mut out = Outcome::default();
    out.missing.push("en".into());
    engine::transcribe_video(&req, &config(&server.url(), &ffmpeg), &none, &mut out)
        .await
        .unwrap();
    assert!(out.saved.is_empty() && out.no_speech);
    assert!(
        out.message().contains("no speech found"),
        "{}",
        out.message()
    );
    assert!(!dir.path().join("Movie.2024.en.srt").exists());
}

#[tokio::test]
async fn a_server_error_or_missing_ffmpeg_fails_the_step_so_it_retries() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = tools(dir.path());
    let v = video(dir.path());
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(503)
        .with_body("overloaded")
        .create_async()
        .await;
    let langs = vec!["en".to_string()];
    let modes = ModePlan::legacy(&langs);
    let req = VideoRequest::new(&v, None, &langs, &modes);
    let mut out = Outcome::default();
    let err = engine::transcribe_video(&req, &config(&server.url(), &ffmpeg), &none, &mut out)
        .await
        .unwrap_err();
    assert_eq!(err, EngineError::Failed("HTTP 503: overloaded".into()));

    let err = engine::transcribe_video(
        &req,
        &config(&server.url(), Path::new("/no/such/ffmpeg")),
        &none,
        &mut out,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("ffmpeg not found"), "{err}");
}
