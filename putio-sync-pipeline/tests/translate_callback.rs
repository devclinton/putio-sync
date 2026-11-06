//! Feature 026: the callback translator inside the pipeline, against a mock translation
//! service. The "service" answers 202 and the test plays its part by calling
//! `accept_callback` with the token it was given.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::*;
use mockito::{Server, ServerGuard};
use putio_sync_core::subtitles::SubtitlesConfig;
use putio_sync_core::translation::{CallbackConfig, TranslationConfig, TranslatorKind};
use putio_sync_db::models::DownloadStatus;
use putio_sync_pipeline::callback::AcceptError;
use putio_sync_pipeline::model::{ItemState, StepKind, StepState};

const SRT: &str = "1\n00:00:01,000 --> 00:00:02,000\nHello there\n\n2\n00:00:03,000 --> 00:00:04,500\nHow are you?\n\n";
const ES: &str =
    "1\n00:00:01,000 --> 00:00:02,000\nHola\n\n2\n00:00:03,000 --> 00:00:04,500\nComo estas?\n\n";
const BIG: u64 = 101 * 1024 * 1024;

/// What the service was sent, as JSON, one entry per request
type Seen = Arc<Mutex<Vec<serde_json::Value>>>;

async fn service(status: usize, body: &'static str) -> (ServerGuard, Seen) {
    let seen: Seen = Arc::default();
    let mut s = Server::new_async().await;
    let sink = seen.clone();
    s.mock("POST", "/translate")
        .with_status(status)
        .with_body_from_request(move |req| {
            sink.lock()
                .unwrap()
                .push(serde_json::from_slice(req.body().unwrap()).unwrap());
            body.as_bytes().to_vec()
        })
        .expect_at_least(0)
        .create_async()
        .await;
    (s, seen)
}

fn token_of(seen: &Seen, n: usize) -> String {
    let v = seen.lock().unwrap()[n]["callback_url"]
        .as_str()
        .unwrap()
        .to_string();
    v.rsplit('/').next().unwrap().to_string()
}

async fn env_for(url: &str, timeout_secs: u64) -> Env {
    let config = TranslationConfig {
        kind: TranslatorKind::Callback,
        callback: CallbackConfig {
            url: format!("{url}/translate"),
            callback_base_url: "http://putio.test".to_string(),
            timeout_secs,
            ..Default::default()
        },
        ..Default::default()
    };
    env_custom(
        |_| vec![],
        move |c| {
            c.subtitles = SubtitlesConfig {
                languages: vec!["en".to_string(), "es".to_string()],
                ..Default::default()
            };
            c.translation = config;
        },
    )
    .await
}

fn big_file(path: &Path) {
    let f = std::fs::File::create(path).unwrap();
    f.set_len(BIG).unwrap();
}

async fn movie(env: &Env) -> PathBuf {
    let video = env.dir.path().join("Movie.2020.mkv");
    big_file(&video);
    std::fs::write(env.dir.path().join("Movie.2020.en.srt"), SRT).unwrap();
    env.db
        .downloads()
        .insert(
            5,
            "Movie.2020.mkv",
            10,
            &video.to_string_lossy(),
            DownloadStatus::Completed,
        )
        .await
        .unwrap();
    downloaded_item(env, "file:5", &video).await;
    video
}

async fn translate_step(env: &Env) -> putio_sync_pipeline::StepRow {
    env.store
        .steps("file:5")
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.kind == StepKind::Translate)
        .unwrap()
}

#[tokio::test]
async fn a_202_parks_the_step_and_the_callback_finishes_it_once() {
    let (s, seen) = service(202, "").await;
    let env = env_for(&s.url(), 600).await;
    movie(&env).await;
    env.pipeline.run_due().await;

    let step = translate_step(&env).await;
    assert_eq!(step.state, StepState::Running, "parked, not failed");
    let t = env.pipeline.timeline("file:5").await.unwrap();
    assert_ne!(t.state, ItemState::Done);
    let sent = seen.lock().unwrap()[0].clone();
    assert_eq!(sent["video_name"], "Movie.2020.mkv");
    assert_eq!(sent["target_lang"], "es");
    assert!(sent["srt"].as_str().unwrap().contains("Hello there"));
    // another pass must not send the job again
    env.pipeline.run_due().await;
    assert_eq!(seen.lock().unwrap().len(), 1);

    let token = token_of(&seen, 0);
    env.pipeline
        .accept_callback(&token, Some("text/plain"), ES)
        .await
        .unwrap();
    assert!(env.dir.path().join("Movie.2020.es.srt").exists());
    assert_eq!(translate_step(&env).await.state, StepState::Done);
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("file:5").await.unwrap();
    assert_eq!(t.state, ItemState::Done);

    // single use: the same token again gets nothing
    assert_eq!(
        env.pipeline
            .accept_callback(&token, Some("text/plain"), ES)
            .await,
        Err(AcceptError::NotFound)
    );
}

#[tokio::test]
async fn an_unknown_token_is_not_found() {
    let (s, _seen) = service(202, "").await;
    let env = env_for(&s.url(), 600).await;
    movie(&env).await;
    assert_eq!(
        env.pipeline
            .accept_callback(&"a".repeat(43), None, ES)
            .await,
        Err(AcceptError::NotFound)
    );
    assert_eq!(
        env.pipeline.accept_callback("short", None, ES).await,
        Err(AcceptError::NotFound)
    );
}

#[tokio::test]
async fn an_immediate_200_finishes_without_parking() {
    let (s, seen) = service(200, ES).await;
    let env = env_for(&s.url(), 600).await;
    movie(&env).await;
    env.pipeline.run_due().await;
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(translate_step(&env).await.state, StepState::Done);
    assert!(env.dir.path().join("Movie.2020.es.srt").exists());
}

#[tokio::test]
async fn a_result_with_the_wrong_timing_is_refused_and_the_step_fails() {
    let (s, seen) = service(202, "").await;
    let env = env_for(&s.url(), 600).await;
    movie(&env).await;
    env.pipeline.run_due().await;
    let token = token_of(&seen, 0);
    let bad = "1\n00:00:09,000 --> 00:00:10,000\nHola\n\n";
    let r = env
        .pipeline
        .accept_callback(&token, Some("text/plain"), bad)
        .await;
    assert!(matches!(r, Err(AcceptError::Unprocessable(_))), "{r:?}");
    assert!(!env.dir.path().join("Movie.2020.es.srt").exists());
    assert_ne!(translate_step(&env).await.state, StepState::Running);
    assert_eq!(
        env.pipeline
            .accept_callback(&token, Some("text/plain"), ES)
            .await,
        Err(AcceptError::NotFound),
        "the token is used up"
    );
}

#[tokio::test]
async fn an_error_body_fails_the_step_but_the_service_gets_ok() {
    let (s, seen) = service(202, "").await;
    let env = env_for(&s.url(), 600).await;
    movie(&env).await;
    env.pipeline.run_due().await;
    let token = token_of(&seen, 0);
    env.pipeline
        .accept_callback(
            &token,
            Some("application/json"),
            r#"{"error":"model overloaded"}"#,
        )
        .await
        .unwrap();
    assert!(!env.dir.path().join("Movie.2020.es.srt").exists());
    assert_ne!(translate_step(&env).await.state, StepState::Running);
}

#[tokio::test]
async fn a_callback_that_never_comes_times_the_step_out_without_retrying() {
    let (s, seen) = service(202, "").await;
    let env = env_for(&s.url(), 1).await;
    movie(&env).await;
    env.pipeline.run_due().await;
    assert_eq!(translate_step(&env).await.state, StepState::Running);
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    env.pipeline.expire_callbacks().await;
    let step = translate_step(&env).await;
    assert_eq!(step.state, StepState::Failed);
    let attempts = env.store.attempts(step.id).await.unwrap();
    assert_eq!(attempts.len(), 1, "no automatic retry");
    assert_eq!(attempts[0].result.unwrap().as_str(), "timeout");
    env.pipeline.run_due().await;
    assert_eq!(seen.lock().unwrap().len(), 1);
    // a late callback is refused
    let token = token_of(&seen, 0);
    assert_eq!(
        env.pipeline.accept_callback(&token, None, ES).await,
        Err(AcceptError::NotFound)
    );
}

#[tokio::test]
async fn startup_expires_jobs_whose_step_is_not_running() {
    let (s, seen) = service(202, "").await;
    let env = env_for(&s.url(), 600).await;
    movie(&env).await;
    env.pipeline.run_due().await;
    assert_eq!(env.store.waiting_jobs().await.unwrap().len(), 1);
    let step = translate_step(&env).await;
    env.store
        .set_step(step.id, StepState::Interrupted, None, None)
        .await
        .unwrap();
    env.pipeline.startup().await;
    assert!(env.store.waiting_jobs().await.unwrap().is_empty());
    let token = token_of(&seen, 0);
    assert_eq!(
        env.pipeline.accept_callback(&token, None, ES).await,
        Err(AcceptError::NotFound)
    );
}
