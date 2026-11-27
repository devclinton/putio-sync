//! The usage profile (feature 056): the setup link page, detection, the review and finish
//! in setup mode, against a mocked Put.io.

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

fn folder(id: i64, name: &str) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false}}"#
    )
}

fn listing() -> String {
    format!(
        r#"{{"status":"OK","parent":{{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false}},"files":[{},{},{}]}}"#,
        folder(1, "Movies"),
        folder(2, "TV Shows"),
        folder(3, "Stuff")
    )
}

struct Setup {
    state: SetupState,
    app: Router,
    dir: tempfile::TempDir,
    _server: mockito::ServerGuard,
}

impl Setup {
    async fn new() -> Self {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/account/info")
            .with_body(ACCOUNT)
            .create_async()
            .await;
        server
            .mock("GET", "/files/list?parent_id=0")
            .with_body(listing())
            .expect_at_least(0)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let state = SetupState::new(
            dir.path().join("config.toml"),
            None,
            vec![SetupReason::NoConfig],
        )
        .with_putio_base_url(&server.url());
        Setup {
            app: setup_router(state.clone()),
            state,
            dir,
            _server: server,
        }
    }

    fn path(&self) -> PathBuf {
        self.dir.path().join("config.toml")
    }

    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        let code = self.state.code();
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {code}"));
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
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// Put.io connected and the download folder chosen
    async fn ready(&self) {
        let (status, _) = self
            .call(
                Method::POST,
                "/setup/putio/token",
                Some(json!({ "token": "putio-tok" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = self
            .call(
                Method::PUT,
                "/setup/folder",
                Some(json!({ "path": self.dir.path().join("downloads"), "create": true })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
    }
}

fn rec<'a>(review: &'a Value, id: &str) -> &'a Value {
    review["recommendations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("no {id} in {review}"))
}

#[tokio::test]
async fn the_setup_link_page_is_public_and_the_link_holds_the_code() {
    let s = Setup::new().await;
    let res = s
        .app
        .clone()
        .oneshot(Request::get("/setup").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(s.state.setup_link(), None);
    s.state.set_link_base("http://nas.lan:15432/");
    assert_eq!(
        s.state.setup_link().unwrap(),
        format!("http://nas.lan:15432/setup#code={}", s.state.code())
    );
}

#[tokio::test]
async fn detect_finds_kinds_from_folder_names() {
    let s = Setup::new().await;
    let (status, _) = s.call(Method::GET, "/setup/profile/detect", None).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "Put.io isn't connected yet"
    );
    s.ready().await;
    let (status, body) = s.call(Method::GET, "/setup/profile/detect", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["found"]["movies"], json!(["Movies"]));
    assert_eq!(body["found"]["tv"], json!(["TV Shows"]));
    assert_eq!(body["folders"], json!(["Movies", "TV Shows", "Stuff"]));
}

#[tokio::test]
async fn the_review_lists_recommendations_and_refuses_no_kinds() {
    let s = Setup::new().await;
    s.ready().await;
    let (status, _) = s
        .call(
            Method::POST,
            "/setup/profile",
            Some(json!({ "answers": { "kinds": [] } })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, review) = s
        .call(
            Method::POST,
            "/setup/profile",
            Some(json!({
                "answers": { "kinds": ["movies", "tv"], "arr": ["radarr"], "by_hand": true, "language": "de" },
                "overrides": {}
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{review}");
    assert_eq!(rec(&review, "rule:Movies")["reason"], json!("found"));
    assert_eq!(rec(&review, "rule:radarr")["reason"], json!("arr_category"));
    assert_eq!(rec(&review, "rule:TV Shows")["value"], json!("video"));
    assert_eq!(rec(&review, "delete")["value"], json!(true));
    assert_eq!(rec(&review, "subtitles")["value"], json!(["de", "en"]));
    assert!(review["notes"].as_array().unwrap().contains(&json!("dlna")));
    // Delete after download is on: the 015 risk shows
    assert!(!review["risks"].as_array().unwrap().is_empty(), "{review}");
}

#[tokio::test]
async fn finish_saves_the_plan_and_the_profile() {
    let s = Setup::new().await;
    s.ready().await;
    let watch = s.dir.path().join("watches");
    let (status, review) = s
        .call(
            Method::POST,
            "/setup/profile",
            Some(json!({
                "answers": { "kinds": ["movies", "music"], "by_hand": true, "watch": watch, "language": "en" },
                "overrides": { "rule:Music": { "value": "everything" } }
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{review}");
    let (status, body) = s.call(Method::POST, "/setup/finish", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let saved: Config = toml::from_str(&std::fs::read_to_string(s.path()).unwrap()).unwrap();
    let preset = |path: &str| {
        saved
            .download
            .rules
            .iter()
            .find(|r| r.path == path)
            .and_then(|r| r.preset.clone())
    };
    assert_eq!(preset("Movies").as_deref(), Some("video"));
    assert_eq!(preset("Music").as_deref(), Some("everything"));
    assert!(saved.watch.enabled);
    assert_eq!(saved.watch.paths, vec![watch.clone()]);
    assert!(watch.is_dir());
    let key = watch.display().to_string();
    assert_eq!(saved.precheck.watch[&key].min_size_mb, Some(5));
    assert!(!saved.download.delete_after_download);
    assert_eq!(saved.subtitles.languages, vec!["en"]);
    let profile = saved.profile.unwrap();
    assert_eq!(profile.answers().kinds.len(), 2);
    assert_eq!(profile.folders["movies"], vec!["Movies"]);
    assert_eq!(
        profile.applied["download.rules[Music].preset"],
        json!("everything")
    );
}

#[tokio::test]
async fn the_manual_path_saves_no_profile() {
    let s = Setup::new().await;
    s.ready().await;
    s.call(
        Method::POST,
        "/setup/profile",
        Some(json!({ "answers": { "kinds": ["movies"] } })),
    )
    .await;
    // "I'll set it up myself"
    let (status, _) = s
        .call(
            Method::POST,
            "/setup/profile",
            Some(json!({ "answers": null })),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = s.call(Method::POST, "/setup/finish", None).await;
    assert_eq!(status, StatusCode::OK);
    let saved: Config = toml::from_str(&std::fs::read_to_string(s.path()).unwrap()).unwrap();
    assert!(saved.profile.is_none());
    assert!(saved.download.rules.is_empty());
}
