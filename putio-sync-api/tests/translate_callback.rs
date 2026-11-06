//! `POST /translate/callback/{token}` (feature 026): public, one-time token, uniform refusals,
//! limiter, size cap, and the token never in a log line.

mod common;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use common::{body_json, test_app, TestApp};
use putio_sync_pipeline::model::{ItemState, StepKind, StepState};
use putio_sync_pipeline::plan::PlannedStep;
use putio_sync_pipeline::store::{CallbackJob, NewItem};
use putio_sync_pipeline::Stage;
use putio_sync_translation::callback::{hash_token, new_token, sha256_hex};
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

const SRT: &str = "1\n00:00:01,000 --> 00:00:02,000\nHello there\n\n2\n00:00:03,000 --> 00:00:04,500\nHow are you?\n\n";
const ES: &str =
    "1\n00:00:01,000 --> 00:00:02,000\nHola\n\n2\n00:00:03,000 --> 00:00:04,500\nComo estas?\n\n";

fn captured_logs() -> Arc<Mutex<Vec<u8>>> {
    static LOGS: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();
    LOGS.get_or_init(|| {
        let logs = Arc::new(Mutex::new(Vec::new()));
        let sink = logs.clone();
        let _ = tracing_subscriber::fmt()
            .with_env_filter("trace,mockito=off")
            .with_ansi(false)
            .with_writer(move || LogWriter(sink.clone()))
            .try_init();
        logs
    })
    .clone()
}

struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A parked translation: returns its token and the folder the result lands in
async fn parked(app: &mut TestApp, key: &str) -> (String, tempfile::TempDir) {
    let pipeline = app.attach_pipeline().await;
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("Movie.mkv");
    std::fs::write(&video, b"video").unwrap();
    let source = dir.path().join("Movie.en.srt");
    std::fs::write(&source, SRT).unwrap();
    let store = pipeline.store();
    store
        .insert_item(
            &NewItem {
                key: key.to_string(),
                name: "Movie".to_string(),
                category: None,
                source: "manual".to_string(),
                dir: video.to_string_lossy().into_owned(),
                transfer_id: None,
                stage: Stage::Translating,
                state: ItemState::Running,
            },
            &[PlannedStep {
                stage: Stage::Translating,
                branch: Some("es".to_string()),
                position: 0,
                kind: StepKind::Translate,
                name: "Movie.mkv".to_string(),
                required: false,
                changes_files: true,
                state: StepState::Waiting,
                skip_reason: None,
            }],
        )
        .await
        .unwrap();
    let step = store.steps(key).await.unwrap().remove(0);
    let (attempt_id, _) = store.start_attempt(step.id).await.unwrap();
    let token = new_token();
    store
        .insert_job(&CallbackJob {
            token_hash: hash_token(&token),
            job_id: "job-1".to_string(),
            step_id: step.id,
            attempt_id,
            source_lang: "en".to_string(),
            target_lang: "es".to_string(),
            source_path: source.to_string_lossy().into_owned(),
            source_sha256: sha256_hex(SRT.as_bytes()),
            target_path: video.to_string_lossy().into_owned(),
            state: "waiting".to_string(),
            created_at: 0,
            expires_at: i64::MAX / 2,
            finished_at: None,
        })
        .await
        .unwrap();
    (token, dir)
}

async fn post(
    app: &TestApp,
    token: &str,
    ctype: &str,
    body: Vec<u8>,
) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("/translate/callback/{token}"))
        .header("Content-Type", ctype)
        .body(Body::from(body))
        .unwrap();
    let response = app.send(request).await;
    let status = response.status();
    (status, body_json(response).await)
}

#[tokio::test]
async fn the_callback_works_without_the_api_token_and_only_once() {
    let mut app = test_app().await;
    let (token, dir) = parked(&mut app, "manual:1").await;
    let (status, body) = post(&app, &token, "text/plain", ES.as_bytes().to_vec()).await;
    assert_eq!(
        (status, body),
        (StatusCode::OK, serde_json::json!({"ok": true}))
    );
    assert!(dir.path().join("Movie.es.srt").exists());
    let (status, _) = post(&app, &token, "text/plain", ES.as_bytes().to_vec()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_json_body_with_the_subtitle_is_taken() {
    let mut app = test_app().await;
    let (token, dir) = parked(&mut app, "manual:1").await;
    let body = serde_json::json!({"srt": ES}).to_string().into_bytes();
    let (status, _) = post(&app, &token, "application/json", body).await;
    assert_eq!(status, StatusCode::OK);
    assert!(dir.path().join("Movie.es.srt").exists());
}

#[tokio::test]
async fn a_subtitle_with_the_wrong_timing_is_422() {
    let mut app = test_app().await;
    let (token, dir) = parked(&mut app, "manual:1").await;
    let bad = "1\n00:00:09,000 --> 00:00:10,000\nHola\n\n";
    let (status, body) = post(&app, &token, "text/plain", bad.as_bytes().to_vec()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["error"].is_string());
    assert!(!dir.path().join("Movie.es.srt").exists());
}

#[tokio::test]
async fn unknown_malformed_and_used_tokens_get_the_same_404() {
    let mut app = test_app().await;
    let (token, _dir) = parked(&mut app, "manual:1").await;
    let unknown = new_token();
    let (s1, b1) = post(&app, &unknown, "text/plain", ES.as_bytes().to_vec()).await;
    let (s2, b2) = post(&app, "short", "text/plain", ES.as_bytes().to_vec()).await;
    post(&app, &token, "text/plain", ES.as_bytes().to_vec()).await;
    let (s3, b3) = post(&app, &token, "text/plain", ES.as_bytes().to_vec()).await;
    assert_eq!(
        (s1, s2, s3),
        (
            StatusCode::NOT_FOUND,
            StatusCode::NOT_FOUND,
            StatusCode::NOT_FOUND
        )
    );
    assert_eq!(b1, b2);
    assert_eq!(b2, b3);
    assert_eq!(b1["error"], "unknown or expired callback");
}

#[tokio::test]
async fn repeated_bad_tokens_are_rate_limited() {
    let mut app = test_app().await;
    let _ = parked(&mut app, "manual:1").await;
    let mut last = StatusCode::OK;
    for _ in 0..8 {
        let request = Request::builder()
            .method(Method::POST)
            .uri(format!("/translate/callback/{}", new_token()))
            .body(Body::from("x"))
            .unwrap();
        let response = app.send(request).await;
        last = response.status();
        if last == StatusCode::TOO_MANY_REQUESTS {
            assert!(response.headers().contains_key("retry-after"));
            break;
        }
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn a_body_over_4_mib_is_413() {
    let mut app = test_app().await;
    let (token, _dir) = parked(&mut app, "manual:1").await;
    let (status, _) = post(&app, &token, "text/plain", vec![b'a'; 4 * 1024 * 1024 + 1]).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn the_token_never_appears_in_a_log_line() {
    let logs = captured_logs();
    let mut app = test_app().await;
    let (token, _dir) = parked(&mut app, "manual:1").await;
    post(&app, &token, "text/plain", ES.as_bytes().to_vec()).await;
    post(&app, &token, "text/plain", ES.as_bytes().to_vec()).await;
    let logs = String::from_utf8_lossy(&logs.lock().unwrap()).to_string();
    assert!(
        logs.contains("/translate/callback/"),
        "the request was logged"
    );
    assert!(!logs.contains(&token), "token in the logs");
}
