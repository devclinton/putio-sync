//! No response body or log line carries a token, and every protected route refuses a
//! request without one (feature 004, SC-003, SC-004, FR-008).

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, body_text, test_app, test_app_without_token, API_TOKEN, PUTIO_TOKEN};
use serde_json::{json, Value};
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

/// Every log line the tests in this file produce, from any thread
fn captured_logs() -> Arc<Mutex<Vec<u8>>> {
    static LOGS: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();
    LOGS.get_or_init(|| {
        let logs = Arc::new(Mutex::new(Vec::new()));
        let sink = logs.clone();
        // Everything at TRACE except mockito, which echoes the requests it receives
        // (the mock's own log, not ours)
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

/// Protected routes from contracts/api.md and the existing routers, with a body for the
/// ones that read JSON
fn protected_routes() -> Vec<(Method, &'static str, Option<Value>)> {
    let get = |path| (Method::GET, path, None);
    let post = |path| (Method::POST, path, None);
    vec![
        get("/putio/files"),
        get("/putio/files/1"),
        (Method::DELETE, "/putio/files/1", None),
        get("/putio/files/1/download"),
        get("/putio/transfers"),
        (
            Method::POST,
            "/putio/transfers",
            Some(json!({ "url": "magnet:?xt=urn:btih:test" })),
        ),
        get("/putio/transfers/1"),
        (Method::DELETE, "/putio/transfers/1", None),
        post("/putio/transfers/1/start"),
        post("/putio/transfers/1/stop"),
        get("/local/files"),
        get("/local/files/missing.txt"),
        get("/local/queue"),
        (Method::PUT, "/local/queue", Some(json!({}))),
        get("/local/queue/stats"),
        post("/local/queue/clear"),
        post("/local/queue/pause"),
        post("/local/queue/resume"),
        post("/local/queue/1/move_up"),
        post("/local/queue/1/move_down"),
        post("/local/queue/1/move_first"),
        post("/local/queue/1/move_last"),
        post("/local/queue/1/skip"),
        post("/local/sync"),
        (
            Method::POST,
            "/local/downloads",
            Some(json!({ "file_id": 1 })),
        ),
        get("/local/downloads"),
        get("/local/downloads/1"),
        (Method::DELETE, "/local/downloads/1", None),
        get("/local/watcher/status"),
        post("/local/watcher/scan"),
        get("/local/workers"),
        get("/local/workers/1"),
        get("/settings/download"),
        get("/settings/watcher"),
        get("/settings/logging"),
        get("/settings/notifications"),
        (Method::PUT, "/settings/notifications", Some(json!({}))),
        get("/stats/downloads"),
        get("/stats/transfers"),
        get("/stats/disk"),
        get("/history"),
        get("/history/stats"),
        get("/status"),
        // Unknown paths fall through to the protected fallback
        get("/robots.txt"),
    ]
}

/// Routes that answer without a token
fn public_routes() -> Vec<(Method, &'static str, Option<Value>)> {
    vec![
        (Method::GET, "/health", None),
        (Method::GET, "/", None),
        (Method::GET, "/assets/app.js", None),
        (Method::GET, "/assets/api.js", None),
        (Method::GET, "/assets/pages/login.js", None),
        (Method::GET, "/auth/status", None),
    ]
}

/// Mock an empty root listing and an empty transfer list
async fn mock_listings(app: &mut common::TestApp) -> (mockito::Mock, mockito::Mock) {
    let root = json!({
        "id": 0, "name": "root", "size": 0, "file_type": "FOLDER", "parent_id": -1,
        "is_folder": true, "is_shared": false, "created_at": "2026-10-20T09:30:00",
        "content_type": "application/x-directory",
    });
    let files = app
        .mock_files_list(
            0,
            json!({ "status": "OK", "files": [], "parent": root, "total": 0 }),
        )
        .await;
    let transfers = app
        .putio
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "status": "OK", "transfers": [] }).to_string())
        .create_async()
        .await;
    (files, transfers)
}

fn assert_no_secret(what: &str, text: &str) {
    for secret in [API_TOKEN, PUTIO_TOKEN] {
        assert!(!text.contains(secret), "{what} contains {secret}:\n{text}");
    }
}

#[tokio::test]
async fn no_token_in_responses_or_logs() {
    let logs = captured_logs();
    let mut app = test_app().await;

    // Answer the listings so the proxy handlers run to the end; anything else gets a
    // 404 (not retried, unlike mockito's default 501), which exercises the error paths
    let (_files, _transfers) = mock_listings(&mut app).await;
    let mut _rest = Vec::new();
    for method in ["GET", "POST"] {
        let mock = app
            .putio
            .mock(method, mockito::Matcher::Any)
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body(json!({ "status": "ERROR", "error_message": "Not found" }).to_string())
            .create_async()
            .await;
        _rest.push(mock);
    }

    let routes: Vec<_> = protected_routes()
        .into_iter()
        .chain(public_routes())
        .collect();
    for (method, path, body) in &routes {
        for token in [Some(API_TOKEN), None] {
            let response = app.req(method.clone(), path, token, body.clone()).await;
            let status = response.status();
            assert_no_secret(
                &format!("{method} {path} ({status})"),
                &body_text(response).await,
            );
        }
    }

    for token in ["wrong-token", API_TOKEN] {
        let response = app
            .req(
                Method::POST,
                "/auth/login",
                None,
                Some(json!({ "token": token })),
            )
            .await;
        assert_no_secret("POST /auth/login", &body_text(response).await);
    }

    // Background work started above (downloads, scans) may still be logging
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let logs = String::from_utf8_lossy(&logs.lock().unwrap()).to_string();
    assert!(
        logs.contains("Authentication failed"),
        "the subscriber captured nothing:\n{logs}"
    );
    assert_no_secret("captured logs", &logs);
}

#[tokio::test]
async fn putio_routes_use_the_shared_client() {
    let mut app = test_app().await;
    let (files, transfers) = mock_listings(&mut app).await;

    assert_eq!(app.get("/putio/files").await.status(), StatusCode::OK);
    assert_eq!(app.get("/putio/transfers").await.status(), StatusCode::OK);
    files.assert_async().await;
    transfers.assert_async().await;

    let url = body_json(app.get("/putio/files/7/download").await).await["download_url"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(url.starts_with(&app.putio.url()), "{url}");
}

#[tokio::test]
async fn every_protected_route_refused_without_token() {
    let app = test_app().await;
    for (method, path, body) in protected_routes() {
        let response = app.req(method.clone(), path, None, body).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
        assert_eq!(
            body_json(response).await["error"],
            "Unauthorized",
            "{method} {path}"
        );
    }
    // No token is not a failed authentication, so none of that tripped the limiter
    assert_eq!(app.get("/status").await.status(), StatusCode::OK);

    let app = test_app_without_token().await;
    for (method, path, body) in protected_routes() {
        let response = app.req(method.clone(), path, None, body).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
        assert_eq!(
            body_json(response).await["error"],
            "API token not configured",
            "{method} {path}"
        );
    }
}
