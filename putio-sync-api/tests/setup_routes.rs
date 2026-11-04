//! Setup mode (feature 013): the gate, the Put.io link, folders, choices and finish,
//! against a mocked Put.io.

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use putio_sync_api::{setup_router, SetupState};
use putio_sync_core::config::Config;
use putio_sync_core::setup::SetupReason;
use serde_json::{json, Value};
use std::path::PathBuf;
use tower::ServiceExt;

const ACCOUNT: &str = r#"{"status":"OK","info":{"user_id":1,"username":"alice","mail":"alice@example.com","account_active":true,"disk":{"size":1000000,"used":5000,"avail":995000},"avatar_url":null}}"#;

struct Setup {
    state: SetupState,
    app: Router,
    dir: tempfile::TempDir,
}

impl Setup {
    fn path(&self) -> PathBuf {
        self.dir.path().join("config.toml")
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        auth: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(auth) = auth {
            req = req.header(header::AUTHORIZATION, format!("Bearer {auth}"));
        }
        let req = match body {
            Some(body) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }

    async fn gated(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let code = self.state.code().to_string();
        self.call(method, path, Some(&code), body).await
    }
}

fn setup_with(base: Option<Config>, putio: Option<&str>) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let mut state = SetupState::new(
        dir.path().join("config.toml"),
        base,
        vec![SetupReason::NoConfig],
    );
    if let Some(url) = putio {
        state = state.with_putio_base_url(url);
    }
    Setup {
        app: setup_router(state.clone()),
        state,
        dir,
    }
}

fn setup() -> Setup {
    setup_with(None, None)
}

#[tokio::test]
async fn status_and_health_are_public() {
    let s = setup();
    let (status, body) = s.call(Method::GET, "/setup/status", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["setup_required"], json!(true));
    assert_eq!(body["reasons"][0]["reason"], json!("no_config"));
    assert_eq!(body["api_token_configured"], json!(false));

    let (_, health) = s.call(Method::GET, "/health", None, None).await;
    assert_eq!(health["status"], json!("setup"));
    let (_, auth) = s.call(Method::GET, "/auth/status", None, None).await;
    assert_eq!(
        auth,
        json!({ "token_configured": false, "setup_required": true })
    );
    let (status, _) = s.call(Method::GET, "/", None, None).await;
    assert_eq!(status, StatusCode::OK);
    // The wizard page and the shell it is loaded by are public too
    for asset in [
        "/assets/app.js",
        "/assets/pages/setup.js",
        "/assets/app.css",
    ] {
        let (status, _) = s.call(Method::GET, asset, None, None).await;
        assert_eq!(status, StatusCode::OK, "{asset}");
    }
}

#[tokio::test]
async fn other_paths_answer_503() {
    let s = setup();
    let (status, body) = s.call(Method::GET, "/status", None, None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["setup_required"], json!(true));
    let code = s.state.code().to_string();
    let (status, _) = s.call(Method::GET, "/local/queue", Some(&code), None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    // What's new (feature 017) waits for setup too
    let (status, _) = s.call(Method::GET, "/whats-new", Some(&code), None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    // So does Put.io storage (feature 033)
    for path in ["/putio/storage", "/putio/storage/runs"] {
        let (status, _) = s.call(Method::GET, path, Some(&code), None).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{path}");
    }
    let (status, _) = s
        .call(Method::POST, "/putio/storage/plan", Some(&code), None)
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn the_code_or_the_api_token_unlocks() {
    let s = setup();
    let (status, _) = s.call(Method::POST, "/setup/unlock", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = s
        .call(Method::POST, "/setup/unlock", Some("WRONG-CODE-0000"), None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = s.gated(Method::POST, "/setup/unlock", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let mut base = Config::default();
    base.api.auth_token = Some("existing-api-token".into());
    let s = setup_with(Some(base), None);
    let (status, _) = s
        .call(
            Method::POST,
            "/setup/unlock",
            Some("existing-api-token"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, auth) = s.call(Method::GET, "/auth/status", None, None).await;
    assert_eq!(auth["token_configured"], json!(true));
}

#[tokio::test]
async fn wrong_codes_are_rate_limited() {
    let s = setup();
    for _ in 0..5 {
        let (status, _) = s
            .call(Method::POST, "/setup/unlock", Some("NOPE"), None)
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    // Even the right code waits out the window
    let (status, _) = s.gated(Method::POST, "/setup/unlock", None).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn device_link_without_an_app_id_is_409() {
    std::env::remove_var("PUTIO_SYNC_APP_ID");
    if putio_sync_core::setup::BUILTIN_APP_ID.is_some() {
        return;
    }
    let s = setup();
    let (status, _) = s.gated(Method::POST, "/setup/putio/link", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn device_link_flow() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/oauth2/oob/code")
        .match_query(mockito::Matcher::UrlEncoded("app_id".into(), "777".into()))
        .with_body(r#"{"status":"OK","code":"LINK42"}"#)
        .create_async()
        .await;
    let pending = server
        .mock("GET", "/oauth2/oob/code/LINK42")
        .with_body(r#"{"status":"OK","oauth_token":null}"#)
        .create_async()
        .await;
    let mut base = Config::default();
    base.putio.app_id = Some("777".into());
    let s = setup_with(Some(base), Some(&server.url()));

    let (status, body) = s.gated(Method::POST, "/setup/putio/link", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["code"], json!("LINK42"));
    assert_eq!(body["link_url"], json!("https://put.io/link"));

    let (_, body) = s.gated(Method::GET, "/setup/putio/link", None).await;
    assert_eq!(body["state"], json!("pending"));
    pending.remove_async().await;

    server
        .mock("GET", "/oauth2/oob/code/LINK42")
        .with_body(r#"{"status":"OK","oauth_token":"linked-secret-token"}"#)
        .create_async()
        .await;
    server
        .mock("GET", "/account/info")
        .match_header("authorization", "Bearer linked-secret-token")
        .with_body(ACCOUNT)
        .create_async()
        .await;
    let (_, body) = s.gated(Method::GET, "/setup/putio/link", None).await;
    assert_eq!(body["state"], json!("linked"));
    assert_eq!(body["account"]["username"], json!("alice"));
    assert!(!body.to_string().contains("linked-secret-token"));
    assert!(!body.to_string().contains("alice@example.com"));

    let (_, session) = s.gated(Method::GET, "/setup/session", None).await;
    assert_eq!(session["account"]["username"], json!("alice"));
    assert!(!session.to_string().contains("linked-secret-token"));
}

#[tokio::test]
async fn pasted_tokens_are_checked() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/account/info")
        .match_header("authorization", "Bearer bad-token")
        .with_status(401)
        .with_body(r#"{"error_type":"invalid_grant","status":"ERROR"}"#)
        .create_async()
        .await;
    server
        .mock("GET", "/account/info")
        .match_header("authorization", "Bearer good-token")
        .with_body(ACCOUNT)
        .create_async()
        .await;
    let s = setup_with(None, Some(&server.url()));
    let (status, _) = s
        .gated(
            Method::POST,
            "/setup/putio/token",
            Some(json!({ "token": "bad-token" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = s
        .gated(
            Method::POST,
            "/setup/putio/token",
            Some(json!({ "token": "good-token" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["account"]["used"], json!(5000));
    assert!(!body.to_string().contains("good-token"));
}

#[tokio::test]
async fn folder_checks() {
    let s = setup();
    let ok = s.dir.path().join("downloads");
    std::fs::create_dir(&ok).unwrap();
    let (status, body) = s
        .gated(
            Method::POST,
            "/setup/folder/check",
            Some(json!({ "path": ok })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["writable"], json!(true));

    let missing = s.dir.path().join("new/inner");
    let (_, body) = s
        .gated(
            Method::POST,
            "/setup/folder/check",
            Some(json!({ "path": missing })),
        )
        .await;
    assert_eq!(body["exists"], json!(false));
    assert_eq!(body["can_create"], json!(true));

    let (status, _) = s
        .gated(
            Method::PUT,
            "/setup/folder",
            Some(json!({ "path": "relative/path" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = s
        .gated(Method::PUT, "/setup/folder", Some(json!({ "path": ok })))
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, session) = s.gated(Method::GET, "/setup/session", None).await;
    assert_eq!(session["download_dir"], json!(ok));
}

#[cfg(unix)]
#[tokio::test]
async fn read_only_folders_are_refused() {
    use std::os::unix::fs::PermissionsExt;
    let s = setup();
    let ro = s.dir.path().join("ro");
    std::fs::create_dir(&ro).unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Root can write anywhere; nothing to test then
    if std::fs::write(ro.join("probe"), b"x").is_ok() {
        return;
    }
    let (status, body) = s
        .gated(Method::PUT, "/setup/folder", Some(json!({ "path": ro })))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["writable"], json!(false));
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[tokio::test]
async fn download_choices_are_validated() {
    let s = setup();
    let (status, _) = s
        .gated(
            Method::PUT,
            "/setup/download",
            Some(json!({ "preset": "nonsense" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = s
        .gated(
            Method::PUT,
            "/setup/download",
            Some(json!({
                "preset": "video",
                "folder_rules": [
                    { "path": "/Music", "preset": "audio" },
                    { "path": "/Junk", "skip": true },
                ],
            })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, session) = s.gated(Method::GET, "/setup/session", None).await;
    assert_eq!(session["preset"], json!("video"));
    assert_eq!(session["folder_rules"].as_array().unwrap().len(), 2);
    assert!(session["presets"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == "audio"));
}

#[tokio::test]
async fn finish_needs_an_account_and_a_folder() {
    let s = setup();
    let (status, body) = s.gated(Method::POST, "/setup/finish", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["missing"], json!(["account", "download_dir"]));
    assert!(!s.path().exists());
    assert!(!s.state.is_finished());
}

#[tokio::test]
async fn finish_saves_once_and_creates_the_api_token() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/account/info")
        .with_body(ACCOUNT)
        .create_async()
        .await;
    let s = setup_with(None, Some(&server.url()));
    let downloads = s.dir.path().join("downloads");
    let watch = s.dir.path().join("watch");
    std::fs::create_dir(&watch).unwrap();
    s.gated(
        Method::POST,
        "/setup/putio/token",
        Some(json!({ "token": "putio-tok" })),
    )
    .await;
    let (status, _) = s
        .gated(
            Method::PUT,
            "/setup/folder",
            Some(json!({ "path": downloads, "create": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    s.gated(
        Method::PUT,
        "/setup/download",
        Some(
            json!({ "preset": "video", "folder_rules": [{ "path": "/Music", "preset": "audio" }] }),
        ),
    )
    .await;
    let (status, _) = s
        .gated(
            Method::PUT,
            "/setup/extras",
            Some(json!({ "watch_dir": watch, "metrics": true })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let waiter = {
        let state = s.state.clone();
        tokio::spawn(async move { state.finished().await })
    };
    let (status, body) = s.gated(Method::POST, "/setup/finish", None).await;
    assert_eq!(status, StatusCode::OK);
    let api_token = body["api_token"].as_str().unwrap().to_string();
    assert_eq!(api_token.len(), 64);
    tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
        .await
        .unwrap()
        .unwrap();

    let saved: Config = toml::from_str(&std::fs::read_to_string(s.path()).unwrap()).unwrap();
    assert_eq!(saved.download.target_directory, downloads);
    assert_eq!(saved.download.preset.as_deref(), Some("video"));
    assert_eq!(saved.download.rules[0].preset.as_deref(), Some("audio"));
    assert!(saved.watch.enabled);
    assert_eq!(saved.watch.paths, vec![watch]);
    assert!(saved.metrics.enabled);
    assert_eq!(saved.api.auth_token.as_deref(), Some(api_token.as_str()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(s.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let (status, _) = s.gated(Method::POST, "/setup/finish", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, st) = s.call(Method::GET, "/setup/status", None, None).await;
    assert_eq!(st["setup_required"], json!(false));
}

#[tokio::test]
async fn dry_run_lists_what_would_download() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/account/info")
        .with_body(ACCOUNT)
        .create_async()
        .await;
    server
        .mock("GET", "/files/list?parent_id=0")
        .with_body(r#"{"status":"OK","parent":{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false},"files":[{"id":5,"name":"film.mkv","size":4000,"file_type":"VIDEO","parent_id":0,"is_folder":false,"is_shared":false}]}"#)
        .create_async()
        .await;
    let s = setup_with(None, Some(&server.url()));
    let (status, _) = s.gated(Method::POST, "/setup/dry-run", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    s.gated(
        Method::POST,
        "/setup/putio/token",
        Some(json!({ "token": "t" })),
    )
    .await;
    s.gated(
        Method::PUT,
        "/setup/folder",
        Some(json!({ "path": s.dir.path() })),
    )
    .await;
    let (status, report) = s.gated(Method::POST, "/setup/dry-run", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["to_download"]["count"], json!(1));
    assert_eq!(report["samples"][0]["path"], json!("film.mkv"));
}

#[tokio::test]
async fn arr_routes_are_reachable() {
    let s = setup();
    // A bad request reaches the 018 code and is refused there, before any network call
    let (status, _) = s
        .gated(
            Method::POST,
            "/setup/arr/preview",
            Some(json!({ "target": { "url": "" }, "api_key": "k" })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = s
        .call(Method::POST, "/setup/arr/discover", None, None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
