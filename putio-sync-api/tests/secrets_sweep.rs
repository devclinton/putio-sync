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
        post("/putio/transfers/1/retry"),
        get("/local/files"),
        get("/local/files/downloads"),
        get("/local/files/downloads/summary?path=x"),
        (Method::DELETE, "/local/files/downloads?path=x", None),
        (
            Method::POST,
            "/local/files/downloads/rename",
            Some(json!({ "path": "x", "name": "y" })),
        ),
        (
            Method::POST,
            "/local/files/watch-0/requeue",
            Some(json!({ "path": "x.torrent.rejected" })),
        ),
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
        get("/settings/precheck"),
        (Method::PUT, "/settings/precheck", Some(json!({}))),
        get("/stats/downloads"),
        get("/stats/transfers"),
        get("/stats/disk"),
        get("/stats/summary"),
        get("/history"),
        get("/history/stats"),
        get("/status"),
        get("/doctor"),
        post("/doctor/run"),
        post("/doctor/fixes/dead_queue_rows.remove/preview"),
        (
            Method::POST,
            "/doctor/fixes/dead_queue_rows.remove/apply",
            Some(json!({ "items": ["1"], "confirm": true })),
        ),
        get("/arr"),
        get("/arr/status"),
        post("/arr/applications/0/test"),
        post("/arr/applications/test"),
        post("/arr/applications"),
        (Method::PUT, "/arr/applications/0", Some(json!({}))),
        (Method::DELETE, "/arr/applications/0", None),
        (Method::PUT, "/arr/client", Some(json!({}))),
        post("/arr/discover"),
        post("/arr/setup/preview"),
        post("/arr/setup"),
        get("/stats/releases"),
        get("/releases/reports"),
        get("/releases/flags"),
        (
            Method::POST,
            "/releases/flags",
            Some(json!({ "target": { "file_id": 1 }, "kind": "fake" })),
        ),
        (Method::DELETE, "/releases/flags/1", None),
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

/// A private tracker's passkey (in the tracker address, magnet link and source) never
/// reaches a response or the recorded download sources (feature 006, FR-011, SC-005)
#[tokio::test]
async fn no_tracker_passkey_in_responses_or_sources() {
    const PASSKEY: &str = "a1b2c3d4e5f6passkey";
    let mut app = test_app().await;
    let root = json!({
        "id": 0, "name": "root", "size": 0, "file_type": "FOLDER", "parent_id": -1,
        "is_folder": true, "is_shared": false,
    });
    let file = json!({
        "id": 31, "name": "movie.mkv", "size": 100, "file_type": "VIDEO", "parent_id": 0,
        "is_folder": false, "is_shared": false,
    });
    let _files = app
        .mock_files_list(
            0,
            json!({ "status": "OK", "files": [file], "parent": root }),
        )
        .await;
    let _transfers = app
        .putio
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({ "status": "OK", "transfers": [{
                "id": 5, "name": "movie", "status": "COMPLETED",
                "created_at": "2026-10-20T09:30:00", "save_parent_id": 0, "file_id": 31,
                "type": "TORRENT", "is_private": true,
                "tracker": format!("https://tracker.example.org/{PASSKEY}/announce"),
                "magneturi": format!("magnet:?xt=urn:btih:abc&tr=https%3A%2F%2Ftracker.example.org%2F{PASSKEY}%2Fannounce"),
                "source": format!("magnet:?xt=urn:btih:abc&tr=https%3A%2F%2Ftracker.example.org%2F{PASSKEY}%2Fannounce"),
            }]})
            .to_string(),
        )
        .create_async()
        .await;

    let response = app.get("/putio/transfers").await;
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    assert!(!text.contains(PASSKEY), "{text}");
    assert!(
        text.contains(r#""tracker_host":"tracker.example.org""#),
        "{text}"
    );

    app.state.download_manager.enqueue_tree(0).await.unwrap();
    let source = app.db().sources().get(31).await.unwrap().unwrap();
    assert_eq!(source.tracker_host.as_deref(), Some("tracker.example.org"));
    assert!(!format!("{source:?}").contains(PASSKEY));

    app.db()
        .history()
        .insert(
            31,
            "movie.mkv",
            100,
            putio_sync_db::models::HistoryAction::Downloaded,
            Some(1),
            None,
        )
        .await
        .unwrap();
    let text = body_text(app.get("/stats/summary?range=24h").await).await;
    assert!(text.contains("tracker.example.org"), "{text}");
    assert!(!text.contains(PASSKEY), "{text}");

    // A flag keeps the host only, in the response and both exports (042 US6)
    let flagged = app
        .post_json(
            "/releases/flags",
            json!({ "target": { "file_id": 31 }, "kind": "fake" }),
        )
        .await;
    assert_eq!(flagged.status(), StatusCode::CREATED);
    let mut texts = vec![body_text(flagged).await];
    for path in [
        "/releases/reports",
        "/releases/reports?format=csv",
        "/releases/flags?file_id=31",
    ] {
        texts.push(body_text(app.get(path).await).await);
    }
    assert!(texts[1].contains("tracker.example.org"), "{}", texts[1]);
    for text in texts {
        assert!(!text.contains(PASSKEY), "{text}");
        assert!(!text.contains("magnet:"), "{text}");
    }
}

/// The download client's password and the *arr API keys never reach a response or a
/// log line (feature 011, Constitution VI)
#[tokio::test]
async fn no_arr_secret_in_responses_or_logs() {
    const ARR_PASSWORD: &str = "arr-secret-password";
    const ARR_KEY: &str = "0123456789abcdefarrkey";
    const ARR_NEW_PASSWORD: &str = "arr-new-secret-password";
    let logs = captured_logs();
    let mut arr = mockito::Server::new_async().await;
    arr.mock("GET", "/api/v3/system/status")
        .with_status(401)
        .create_async()
        .await;
    let arr_url = arr.url();
    let app = common::test_app_with(|config| {
        config.arr = putio_sync_core::config::ArrConfig {
            enabled: true,
            password: Some(ARR_PASSWORD.to_string()),
            servers: vec![
                putio_sync_core::config::ArrServer {
                    kind: putio_sync_core::config::ArrKind::Sonarr,
                    url: arr_url.clone(),
                    api_key: Some(ARR_KEY.to_string()),
                    name: None,
                    category: Some("sonarr".to_string()),
                },
                putio_sync_core::config::ArrServer {
                    kind: putio_sync_core::config::ArrKind::Radarr,
                    url: "http://127.0.0.1:9".to_string(),
                    api_key: Some(ARR_KEY.to_string()),
                    name: None,
                    category: Some("radarr".to_string()),
                },
            ],
            ..Default::default()
        };
    })
    .await;

    let mut texts = Vec::new();
    for path in ["/arr/applications/0/test", "/arr/applications/1/test"] {
        texts.push(body_text(app.post_json(path, json!({})).await).await);
    }
    let port: u16 = arr_url.rsplit(':').next().unwrap().parse().unwrap();
    let form = json!({"kind": "sonarr", "host": "127.0.0.1", "port": port, "api_key": "",
                      "index": 0});
    texts.push(body_text(app.post_json("/arr/applications/test", form.clone()).await).await);
    let response = app
        .req(
            Method::PUT,
            "/arr/applications/0",
            Some(&app.token),
            Some(form),
        )
        .await;
    texts.push(body_text(response).await);
    texts.push(body_text(app.get("/arr").await).await);
    texts.push(body_text(app.get("/arr/status").await).await);
    texts.push(body_text(app.get("/settings/download").await).await);

    // Setup: a preview, then each outcome. The key is sent in the body and the password to
    // the *arr, never back.
    let mut good = mockito::Server::new_async().await;
    good.mock("GET", "/api/v3/system/status")
        .with_body(r#"{"appName":"Sonarr","version":"4.0.15.2941"}"#)
        .create_async()
        .await;
    good.mock("GET", "/api/v3/downloadclient")
        .with_body("[]")
        .create_async()
        .await;
    good.mock("GET", "/api/v3/downloadclient/schema")
        .with_body(
            json!([{"implementation": "Transmission", "configContract": "TransmissionSettings",
                    "fields": [{"name": "host"}, {"name": "password"}, {"name": "tvCategory"}]}])
            .to_string(),
        )
        .create_async()
        .await;
    good.mock("POST", "/api/v3/downloadclient/test")
        .create_async()
        .await;
    good.mock("POST", "/api/v3/downloadclient")
        .with_status(201)
        .with_body(r#"{"id": 5, "name": "putio-sync"}"#)
        .create_async()
        .await;
    good.mock("GET", "/api/v3/remotepathmapping")
        .with_body("[]")
        .create_async()
        .await;
    good.mock("GET", "/api/v3/filesystem")
        .match_query(mockito::Matcher::Any)
        .with_body(r#"{"directories": [], "files": []}"#)
        .create_async()
        .await;
    for (url, path) in [
        (good.url(), "/arr/setup/preview"),
        (good.url(), "/arr/setup"),
        (arr_url.clone(), "/arr/setup"),
        ("http://127.0.0.1:9".to_string(), "/arr/setup"),
    ] {
        let req = json!({"target": {"url": url}, "api_key": ARR_KEY, "remember": true});
        texts.push(body_text(app.post_json(path, req).await).await);
    }
    texts.push(
        body_text(
            app.post_json("/arr/setup", json!({"target": {"application": 0}}))
                .await,
        )
        .await,
    );
    // A new password is accepted and never shown
    let response = app
        .req(
            Method::PUT,
            "/arr/client",
            Some(&app.token),
            Some(json!({"enabled": true, "username": "putio-sync", "password": ARR_NEW_PASSWORD})),
        )
        .await;
    texts.push(body_text(response).await);
    texts.push(body_text(app.get("/arr").await).await);
    for auth in ["Basic Zm9vOmJhcg==", "Basic !!!"] {
        let request = axum::http::Request::builder()
            .method(Method::POST)
            .uri("/transmission/rpc")
            .header("Authorization", auth)
            .body(axum::body::Body::from("{}"))
            .unwrap();
        texts.push(body_text(app.send(request).await).await);
    }
    // The config file on disk keeps them, as the operator wrote it
    let on_disk = std::fs::read_to_string(app._dir.path().join("config.toml")).unwrap();
    assert!(on_disk.contains(ARR_KEY));

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let logs = String::from_utf8_lossy(&logs.lock().unwrap()).to_string();
    for text in texts.iter().chain([&logs]) {
        assert!(!text.contains(ARR_PASSWORD), "{text}");
        assert!(!text.contains(ARR_KEY), "{text}");
        assert!(!text.contains(ARR_NEW_PASSWORD), "{text}");
    }
    // The setups ran: a created client, and a refused key
    let all = texts.join("\n");
    assert!(all.contains(r#""outcome":"created""#), "{all}");
    assert!(all.contains(r#""reason":"unauthorized""#), "{all}");
}
