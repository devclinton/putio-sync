//! The qBittorrent Web API v2 through the router (feature 067).
//!
//! The first tests cover the sign-in and its security. The `contract_*` tests replay the
//! requests each supported tool makes, in the order it makes them. Those request shapes are
//! **reconstructed from knowledge of each tool's client code, not captured from the tools**:
//! the repository has no captures. A capture that differs is a bug in the test.

mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::response::Response;
use common::{body_json, body_text, test_app, test_app_with, TestApp};
use mockito::Matcher;
use putio_sync_core::config::ArrConfig;
use serde_json::{json, Value};
use std::time::Duration;

const HASH: &str = "c12fe1c06bba254a9dc9f519b335aa7c1367a88a";
const PASSWORD: &str = "arr-password";
const FORM: &str = "application/x-www-form-urlencoded";

/// 2 GB, so the release pre-check (042) passes it
const TORRENT: &[u8] = b"d4:infod6:lengthi2147483648e4:name9:Movie.mkv12:piece lengthi1e6:pieces20:aaaaaaaaaaaaaaaaaaaaee";

async fn arr_app() -> TestApp {
    test_app_with(|config| {
        config.arr = ArrConfig {
            enabled: true,
            password: Some(PASSWORD.to_string()),
            ..ArrConfig::default()
        };
        // No cache requests from tests (Constitution III)
        config.precheck.magnet_lookup = false;
    })
    .await
}

/// One tool's HTTP client: its User-Agent and the cookie it keeps
struct Client<'a> {
    app: &'a TestApp,
    agent: &'static str,
    cookie: Option<String>,
}

impl<'a> Client<'a> {
    fn new(app: &'a TestApp, agent: &'static str) -> Self {
        Self {
            app,
            agent,
            cookie: None,
        }
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        content_type: Option<&str>,
        body: Vec<u8>,
    ) -> Response {
        let mut builder = Request::builder()
            .method(method)
            .uri(format!("/api/v2/{path}"))
            .header(header::USER_AGENT, self.agent);
        if let Some(cookie) = &self.cookie {
            builder = builder.header(header::COOKIE, cookie);
        }
        if let Some(content_type) = content_type {
            builder = builder.header(header::CONTENT_TYPE, content_type);
        }
        self.app.send(builder.body(Body::from(body)).unwrap()).await
    }

    async fn get(&self, path: &str) -> Response {
        self.send(Method::GET, path, None, Vec::new()).await
    }

    async fn post(&self, path: &str, form: &str) -> Response {
        self.send(Method::POST, path, Some(FORM), form.as_bytes().to_vec())
            .await
    }

    async fn get_json(&self, path: &str) -> Value {
        let response = self.get(path).await;
        assert_eq!(response.status(), StatusCode::OK, "GET {path}");
        body_json(response).await
    }

    async fn get_text(&self, path: &str) -> String {
        let response = self.get(path).await;
        assert_eq!(response.status(), StatusCode::OK, "GET {path}");
        body_text(response).await
    }

    /// POST a form and expect 200; the body comes back
    async fn post_ok(&self, path: &str, form: &str) -> String {
        let response = self.post(path, form).await;
        assert_eq!(response.status(), StatusCode::OK, "POST {path} {form}");
        body_text(response).await
    }

    /// Sign in as the tool does and keep `SID=...`, the first part of `Set-Cookie`
    async fn login(&mut self) {
        let form = format!("username=putio-sync&password={PASSWORD}");
        let response = self.post("auth/login", &form).await;
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(body_text(response).await, "Ok.");
        let first = cookie.split(';').next().unwrap().to_string();
        assert!(first.starts_with("SID="), "{first}");
        self.cookie = Some(first);
    }
}

/// A `multipart/form-data` body: text fields, then one .torrent file part
fn multipart(fields: &[(&str, &str)], file: Option<(&str, &[u8])>) -> (String, Vec<u8>) {
    let boundary = "----putio-sync-test-boundary";
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    if let Some((filename, bytes)) = file {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"torrents\"; filename=\"{filename}\"\r\nContent-Type: application/x-bittorrent\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn magnet() -> String {
    format!("magnet:?xt=urn:btih:{}&dn=Show.S01E01", HASH.to_uppercase())
}

fn encoded(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Put.io for one magnet added to `category`: the folder (id 77) is created, the transfer
/// (id 900) is added, and the transfer list shows it half done
async fn mock_magnet(app: &mut TestApp, category: &str) -> Vec<mockito::Mock> {
    let mut mocks = vec![
        app.mock_files_list(0, json!({"files": [], "parent": {"id": 0, "name": "root", "size": 0, "file_type": "FOLDER", "parent_id": 0}, "status": "OK"}))
            .await,
    ];
    mocks.push(
        app.putio
            .mock("POST", "/files/create-folder")
            .match_body(Matcher::PartialJson(json!({"name": category, "parent_id": 0})))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                json!({"file": {"id": 77, "name": category, "size": 0, "file_type": "FOLDER", "parent_id": 0}, "status": "OK"})
                    .to_string(),
            )
            .create_async()
            .await,
    );
    let transfer = json!({"id": 900, "name": "Show.S01E01", "size": 1000, "status": "IN_QUEUE",
        "created_at": "2026-10-26T08:00:00", "save_parent_id": 77, "is_private": false});
    mocks.push(
        app.putio
            .mock("POST", "/transfers/add")
            .match_body(Matcher::PartialJson(json!({"save_parent_id": 77})))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({"transfer": transfer, "status": "OK"}).to_string())
            .expect(1)
            .create_async()
            .await,
    );
    mocks.push(
        app.putio
            .mock("GET", "/transfers/list")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                json!({"transfers": [{"id": 900, "name": "Show.S01E01", "size": 1000, "status": "DOWNLOADING",
                    "percent_done": 50, "estimated_time": 10, "created_at": "2026-10-26T08:00:00",
                    "save_parent_id": 77, "is_private": false}], "status": "OK"})
                .to_string(),
            )
            .create_async()
            .await,
    );
    mocks
}

/// Put.io for a .torrent upload into the root folder (transfer id 5)
async fn mock_upload(app: &mut TestApp) -> mockito::Mock {
    app.putio
        .mock("POST", "/files/upload")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({"transfer": {"id": 5, "name": "Movie.mkv", "status": "IN_QUEUE",
                "created_at": "2026-10-26T08:00:00", "save_parent_id": 0, "is_private": false}, "status": "OK"})
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await
}

async fn mock_cancel(app: &mut TestApp) -> mockito::Mock {
    app.putio
        .mock("POST", "/transfers/cancel")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(1)
        .create_async()
        .await
}

// ---- Sign-in and security ----

#[tokio::test]
async fn off_unless_arr_is_enabled() {
    let app = test_app().await;
    let client = Client::new(&app, "test");
    assert_eq!(
        client.get("app/version").await.status(),
        StatusCode::NOT_FOUND
    );
    let form = format!("username=putio-sync&password={PASSWORD}");
    assert_eq!(
        client.post("auth/login", &form).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn enabled_without_password_is_503() {
    let app = test_app_with(|config| config.arr.enabled = true).await;
    let client = Client::new(&app, "test");
    let response = client
        .post("auth/login", "username=putio-sync&password=")
        .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn everything_but_the_sign_in_needs_a_session() {
    let app = arr_app().await;
    let mut client = Client::new(&app, "test");
    for path in ["app/version", "app/webapiVersion", "torrents/info", "nope"] {
        let response = client.get(path).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        assert_eq!(body_text(response).await, "Forbidden");
    }
    assert_eq!(
        client.post("torrents/delete", "hashes=all").await.status(),
        StatusCode::FORBIDDEN
    );
    client.cookie = Some("SID=0123456789abcdef".to_string());
    assert_eq!(
        client.get("app/version").await.status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn sign_in_answers_like_qbittorrent() {
    let app = arr_app().await;
    let client = Client::new(&app, "test");

    assert_eq!(
        client.get("auth/login").await.status(),
        StatusCode::METHOD_NOT_ALLOWED
    );

    // A wrong password or user name: 200 "Fails." and no cookie
    for form in [
        "username=putio-sync&password=guess".to_string(),
        format!("username=admin&password={PASSWORD}"),
        "username=putio-sync".to_string(),
    ] {
        let response = client.post("auth/login", &form).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key(header::SET_COOKIE));
        assert_eq!(body_text(response).await, "Fails.");
    }

    let form = format!("username=putio-sync&password={PASSWORD}");
    let response = client.post("auth/login", &form).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .to_string();
    assert_eq!(body_text(response).await, "Ok.");
    let mut parts = cookie.split("; ");
    let sid = parts.next().unwrap().strip_prefix("SID=").unwrap();
    assert_eq!(sid.len(), 64);
    assert!(sid.bytes().all(|b| b.is_ascii_hexdigit()));
    let attributes: Vec<&str> = parts.collect();
    assert_eq!(
        attributes,
        [
            "HttpOnly",
            "SameSite=Strict",
            "Path=/api/v2",
            "Max-Age=86400"
        ]
    );

    // Each sign-in gets its own id
    let again = client.post("auth/login", &form).await;
    assert_ne!(
        again.headers()[header::SET_COOKIE].to_str().unwrap(),
        cookie
    );
}

#[tokio::test]
async fn wrong_passwords_are_throttled_with_the_transmission_limiter() {
    let app = arr_app().await;
    let client = Client::new(&app, "test");
    for _ in 0..5 {
        let response = client
            .post("auth/login", "username=putio-sync&password=guess")
            .await;
        assert_eq!(body_text(response).await, "Fails.");
    }
    // The right password is refused too, as qBittorrent's ban does
    let form = format!("username=putio-sync&password={PASSWORD}");
    let response = client.post("auth/login", &form).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(response.headers().contains_key(header::RETRY_AFTER));
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    assert!(body_text(response).await.contains("banned"));

    // The UI's own limiter is untouched
    let response = app.get("/status").await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_session_ends_on_logout_expiry_and_password_change() {
    let app = arr_app().await;
    let arr = app.state.arr.clone().unwrap();

    let mut client = Client::new(&app, "test");
    client.login().await;
    assert_eq!(client.get_text("app/version").await, "v4.6.7");
    let response = client.post("auth/logout", "").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers()[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .starts_with("SID=; HttpOnly; SameSite=Strict; Path=/api/v2; Max-Age=0"));
    assert_eq!(
        client.get("app/version").await.status(),
        StatusCode::FORBIDDEN
    );

    client.login().await;
    arr.qbit.age_all(Duration::from_secs(3601));
    assert_eq!(
        client.get("app/version").await.status(),
        StatusCode::FORBIDDEN
    );

    client.login().await;
    let mut settings = (*arr.service.settings()).clone();
    settings.password = Some("new-password".to_string());
    arr.service.apply(settings);
    assert_eq!(
        client.get("app/version").await.status(),
        StatusCode::FORBIDDEN
    );
    assert!(arr.qbit.is_empty());
}

#[tokio::test]
async fn a_browser_on_another_site_is_refused() {
    let app = arr_app().await;
    let mut client = Client::new(&app, "Mozilla/5.0");
    client.login().await;
    let request = |origin: &'static str| {
        Request::builder()
            .method(Method::POST)
            .uri("/api/v2/torrents/delete")
            .header(header::HOST, "nas:15432")
            .header(header::ORIGIN, origin)
            .header(header::COOKIE, client.cookie.clone().unwrap())
            .header(header::CONTENT_TYPE, FORM)
            .body(Body::from("hashes=abc&deleteFiles=true"))
            .unwrap()
    };
    let response = app.send(request("https://evil.example")).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = app.send(request("http://nas:15432")).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_change_needs_a_post() {
    let app = arr_app().await;
    let mut client = Client::new(&app, "test");
    client.login().await;
    for path in [
        "torrents/delete?hashes=abc",
        "torrents/add?urls=magnet:?xt=urn:btih:abc",
        "torrents/pause?hashes=all",
        "torrents/createCategory?category=tv",
        "auth/logout",
    ] {
        assert_eq!(
            client.get(path).await.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "{path}"
        );
    }
    assert_eq!(client.get_text("app/version").await, "v4.6.7");
}

#[tokio::test]
async fn fixed_answers_and_unknown_routes() {
    let app = arr_app().await;
    let mut client = Client::new(&app, "test");
    client.login().await;
    let target = app._dir.path().display().to_string();
    let target = target.trim_end_matches('/');

    assert_eq!(client.get_text("app/webapiVersion").await, "2.9.3");
    assert_eq!(client.get_text("app/defaultSavePath").await, target);
    assert_eq!(client.get_text("transfer/speedLimitsMode").await, "0");
    assert_eq!(
        client.get_json("app/buildInfo").await["platform"],
        "putio-sync"
    );
    assert_eq!(client.get_json("torrents/tags").await, json!([]));
    assert_eq!(client.get_json("torrents/info").await, json!([]));

    let preferences = client.get_json("app/preferences").await;
    assert_eq!(preferences["save_path"], target);
    assert_eq!(preferences["dht"], true);
    assert_eq!(preferences["max_ratio_act"], 0);

    // Accepted and ignored
    for path in [
        "torrents/setShareLimits",
        "torrents/topPrio",
        "torrents/setForceStart",
        "torrents/reannounce",
        "torrents/recheck",
        "torrents/addTags",
    ] {
        client.post_ok(path, "hashes=abc").await;
    }
    client.post_ok("torrents/pause", "hashes=abc").await;
    client.post_ok("torrents/start", "hashes=all").await;

    // An unknown hash, an unknown route, and a delete that names nothing
    for path in [
        "torrents/properties?hash=abc",
        "torrents/files?hash=abc",
        "torrents/trackers",
        "app/shutdown",
    ] {
        let status = client.get(path).await.status();
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED,
            "{path}: {status}"
        );
    }
    assert_eq!(
        client.post("app/shutdown", "").await.status(),
        StatusCode::NOT_FOUND
    );
    for form in ["hashes=all&deleteFiles=true", "hashes=", ""] {
        assert_eq!(
            client.post("torrents/delete", form).await.status(),
            StatusCode::BAD_REQUEST,
            "{form}"
        );
    }
}

#[tokio::test]
async fn categories_are_folders() {
    let app = arr_app().await;
    let mut client = Client::new(&app, "test");
    client.login().await;
    let target = app._dir.path().display().to_string();
    let target = target.trim_end_matches('/');

    assert_eq!(client.get_json("torrents/categories").await, json!({}));
    client
        .post_ok("torrents/createCategory", "category=movies%2F4k")
        .await;
    client
        .post_ok(
            "torrents/createCategory",
            "category=tv&savePath=%2Felsewhere",
        )
        .await;
    let categories = client.get_json("torrents/categories").await;
    assert_eq!(
        categories["tv"],
        json!({"name": "tv", "savePath": format!("{target}/tv")})
    );
    assert_eq!(
        categories["movies/4k"]["savePath"],
        format!("{target}/movies/4k")
    );

    assert_eq!(
        client
            .post("torrents/createCategory", "category=")
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        client
            .post("torrents/createCategory", "category=..%2Fup")
            .await
            .status(),
        StatusCode::CONFLICT
    );
    client
        .post_ok("torrents/removeCategories", "categories=tv")
        .await;
    assert!(client
        .get_json("torrents/categories")
        .await
        .get("tv")
        .is_none());
}

#[tokio::test]
async fn a_bad_add_is_not_a_success() {
    let mut app = arr_app().await;
    let none = app
        .putio
        .mock("POST", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let mut client = Client::new(&app, "test");
    client.login().await;

    assert_eq!(
        client.post("torrents/add", "category=tv").await.status(),
        StatusCode::BAD_REQUEST
    );
    let response = client.post("torrents/add", "urls=magnet%3A%3Fdn%3Dx").await;
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(body_text(response).await, "Fails.");

    let (content_type, body) = multipart(&[], Some(("bad.torrent", b"not a torrent")));
    let response = client
        .send(Method::POST, "torrents/add", Some(&content_type), body)
        .await;
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    // A folder outside the download folder, by `savepath` or by category
    let form = format!("urls={}&savepath=%2Felsewhere", encoded(&magnet()));
    let response = client.post("torrents/add", &form).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(body_text(response)
        .await
        .starts_with("download-dir must be inside"));
    let form = format!("urls={}&category=..%2Fup", encoded(&magnet()));
    assert_eq!(
        client.post("torrents/add", &form).await.status(),
        StatusCode::CONFLICT
    );
    none.assert_async().await;
}

// ---- Contract: Radarr and Sonarr (their qBittorrent download client, API v2 proxy) ----

/// Reconstructed from knowledge of Radarr/Sonarr's `QBittorrentProxyV2`, not captured:
/// the test connection, a magnet grab, the queue poll, and the removal after import.
#[tokio::test]
async fn contract_radarr_and_sonarr() {
    let mut app = arr_app().await;
    let _mocks = mock_magnet(&mut app, "tv-sonarr").await;
    let target = app._dir.path().display().to_string();
    let target = target.trim_end_matches('/').to_string();
    let cancel = mock_cancel(&mut app).await;
    let mut sonarr = Client::new(&app, "Sonarr/4.0.9.2244 (ubuntu 22.04)");

    // The first request carries no cookie; the 403 makes the app sign in
    assert_eq!(
        sonarr.get("app/webapiVersion").await.status(),
        StatusCode::FORBIDDEN
    );
    sonarr.login().await;

    // Test: the versions, the settings it validates, and its category
    let api: Vec<u32> = sonarr
        .get_text("app/webapiVersion")
        .await
        .split('.')
        .map(|n| n.parse().unwrap())
        .collect();
    assert!(api >= vec![2, 6, 1], "content_path needs 2.6.1");
    assert!(sonarr.get_text("app/version").await.starts_with('v'));
    let preferences = sonarr.get_json("app/preferences").await;
    assert_eq!(preferences["save_path"], target);
    assert_eq!(preferences["dht"], true, "magnets need DHT");
    assert_eq!(preferences["queueing_enabled"], true);
    // "Remove at the share limit" (1 or 3) is what the apps refuse
    assert_eq!(preferences["max_ratio_act"], 0);
    assert!(preferences["max_ratio_enabled"].is_boolean());
    assert!(preferences["max_ratio"].is_number());
    assert!(preferences["max_seeding_time_enabled"].is_boolean());
    assert!(preferences["max_seeding_time"].is_number());
    assert!(sonarr
        .get_json("torrents/categories")
        .await
        .get("tv-sonarr")
        .is_none());
    sonarr
        .post_ok("torrents/createCategory", "category=tv-sonarr")
        .await;
    assert_eq!(
        sonarr.get_json("torrents/categories").await["tv-sonarr"]["savePath"],
        format!("{target}/tv-sonarr")
    );

    // Grab a magnet
    let form = format!(
        "urls={}&category=tv-sonarr&paused=false&stopped=false",
        encoded(&magnet())
    );
    assert_eq!(sonarr.post_ok("torrents/add", &form).await, "Ok.");
    // The same release again is not an error, and Put.io is not called twice
    assert_eq!(sonarr.post_ok("torrents/add", &form).await, "Ok.");

    // After the add: wait for it, then the settings the app sets on it
    let upper = HASH.to_uppercase();
    let properties = sonarr
        .get_json(&format!("torrents/properties?hash={upper}"))
        .await;
    assert_eq!(properties["save_path"], format!("{target}/tv-sonarr"));
    assert!(properties["seeding_time"].is_number());
    let form =
        format!("hashes={upper}&ratioLimit=-2&seedingTimeLimit=-2&inactiveSeedingTimeLimit=-2");
    sonarr.post_ok("torrents/setShareLimits", &form).await;
    sonarr
        .post_ok("torrents/topPrio", &format!("hashes={upper}"))
        .await;
    sonarr
        .post_ok(
            "torrents/setForceStart",
            &format!("hashes={upper}&value=true"),
        )
        .await;

    // The queue poll
    let torrents = sonarr.get_json("torrents/info?category=tv-sonarr").await;
    let torrents = torrents.as_array().unwrap();
    assert_eq!(torrents.len(), 1);
    let t = &torrents[0];
    assert_eq!(t["hash"], HASH);
    assert_eq!(t["name"], "Show.S01E01");
    assert_eq!(t["category"], "tv-sonarr");
    assert_eq!(t["size"], 1000);
    assert_eq!(t["progress"], 0.25);
    assert_eq!(t["eta"], 10);
    assert_eq!(t["state"], "downloading");
    assert_eq!(t["save_path"], format!("{target}/tv-sonarr"));
    assert_eq!(t["content_path"], format!("{target}/tv-sonarr/Show.S01E01"));
    for field in [
        "ratio",
        "ratio_limit",
        "seeding_time",
        "seeding_time_limit",
        "inactive_seeding_time_limit",
        "last_activity",
    ] {
        assert!(t[field].is_number(), "{field}");
    }
    assert_eq!(
        sonarr.get_json("torrents/info?category=radarr").await,
        json!([])
    );
    let files = sonarr
        .get_json(&format!("torrents/files?hash={upper}"))
        .await;
    assert_eq!(files[0]["name"], "Show.S01E01");

    // A post-import category would mean another folder
    let form = format!("hashes={upper}&category=tv-imported");
    assert_eq!(
        sonarr.post("torrents/setCategory", &form).await.status(),
        StatusCode::CONFLICT
    );
    sonarr
        .post_ok(
            "torrents/setCategory",
            &format!("hashes={upper}&category=tv-sonarr"),
        )
        .await;

    // Remove: the transfer is cancelled
    sonarr
        .post_ok(
            "torrents/delete",
            &format!("hashes={upper}&deleteFiles=false"),
        )
        .await;
    cancel.assert_async().await;
    assert_eq!(
        sonarr.get_json("torrents/info?category=tv-sonarr").await,
        json!([])
    );
    assert_eq!(
        sonarr
            .get(&format!("torrents/properties?hash={upper}"))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

/// Reconstructed from knowledge, not captured: Radarr and Sonarr send a .torrent as
/// `multipart/form-data` with the file in a part named `torrents`
#[tokio::test]
async fn contract_radarr_and_sonarr_torrent_file() {
    let mut app = arr_app().await;
    let upload = mock_upload(&mut app).await;
    let mut radarr = Client::new(&app, "Radarr/5.14.0.9383 (alpine 3.20.3)");
    radarr.login().await;

    let (content_type, body) = multipart(
        &[("paused", "false"), ("stopped", "false")],
        Some(("Movie.torrent", TORRENT)),
    );
    let response = radarr
        .send(Method::POST, "torrents/add", Some(&content_type), body)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_text(response).await, "Ok.");
    upload.assert_async().await;

    let torrents = radarr.get_json("torrents/info").await;
    assert_eq!(torrents[0]["name"], "Movie.mkv");
    assert_eq!(torrents[0]["category"], "");
    assert_eq!(torrents[0]["state"], "queuedDL");
    assert_eq!(torrents[0]["hash"].as_str().unwrap().len(), 40);
}

// ---- Contract: autobrr (the go-qbittorrent client) ----

/// Reconstructed from knowledge of autobrr's qBittorrent action, not captured: the test,
/// the "max active downloads" check, a multipart add, and the reannounce loop
#[tokio::test]
async fn contract_autobrr() {
    let mut app = arr_app().await;
    let upload = mock_upload(&mut app).await;
    let target = app._dir.path().display().to_string();
    let mut autobrr = Client::new(&app, "autobrr/1.50.0");
    autobrr.login().await;
    assert!(autobrr.get_text("app/version").await.starts_with("v4."));
    assert!(!autobrr.get_text("app/webapiVersion").await.is_empty());

    // The rule check: how many are downloading now
    assert_eq!(
        autobrr.get_json("torrents/info?filter=downloading").await,
        json!([])
    );

    // The add: always multipart, with every option as a text field
    let (content_type, body) = multipart(
        &[
            ("paused", "false"),
            ("stopped", "false"),
            ("autoTMM", "false"),
            ("savepath", &target),
            ("category", ""),
            ("tags", "autobrr"),
            ("contentLayout", "Original"),
            ("skip_checking", "false"),
            ("ratioLimit", "2"),
            ("seedingTimeLimit", "1440"),
        ],
        Some(("release.torrent", TORRENT)),
    );
    let response = autobrr
        .send(Method::POST, "torrents/add", Some(&content_type), body)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    upload.assert_async().await;

    let downloading = autobrr.get_json("torrents/info?filter=downloading").await;
    assert_eq!(downloading.as_array().unwrap().len(), 1);
    let hash = downloading[0]["hash"].as_str().unwrap().to_string();

    // The reannounce loop ends at the first tracker with status 2 (working)
    let trackers = autobrr
        .get_json(&format!("torrents/trackers?hash={hash}"))
        .await;
    assert_eq!(trackers[0]["status"], 2);
    assert_eq!(trackers[0]["msg"], "");
    assert!(trackers[0]["url"].as_str().unwrap().starts_with("https://"));
    autobrr
        .post_ok("torrents/reannounce", &format!("hashes={hash}"))
        .await;
    assert_eq!(
        autobrr
            .get_json(&format!("torrents/properties?hash={hash}"))
            .await["hash"],
        hash
    );
}

// ---- Contract: cross-seed ----

/// Reconstructed from knowledge of cross-seed's qBittorrent client, not captured: it
/// reads the cookie from `Set-Cookie`, sends every request as a POST with a form body, and
/// injects with a multipart add that names the folder of the data it matched
#[tokio::test]
async fn contract_cross_seed() {
    let mut app = arr_app().await;
    let _mocks = mock_magnet(&mut app, "tv").await;
    let target = app._dir.path().display().to_string();
    let target = target.trim_end_matches('/').to_string();
    let mut cross_seed = Client::new(&app, "node");
    cross_seed.login().await;

    let seed = format!("urls={}&category=tv", encoded(&magnet()));
    cross_seed.post_ok("torrents/add", &seed).await;

    // Reads are POSTs with the parameters in the body
    assert!(cross_seed.post_ok("app/version", "").await.starts_with('v'));
    let preferences: Value =
        serde_json::from_str(&cross_seed.post_ok("app/preferences", "").await).unwrap();
    assert_eq!(preferences["torrent_content_layout"], "Original");
    assert_eq!(preferences["auto_tmm_enabled"], false);
    let info: Value = serde_json::from_str(
        &cross_seed
            .post_ok("torrents/info", &format!("hashes={HASH}"))
            .await,
    )
    .unwrap();
    assert_eq!(info[0]["hash"], HASH);
    assert_eq!(info[0]["save_path"], format!("{target}/tv"));
    assert_eq!(info[0]["category"], "tv");
    assert_eq!(info[0]["tags"], "");
    assert_eq!(info[0]["auto_tmm"], false);
    assert!(info[0]["progress"].as_f64().unwrap() < 1.0);
    let none: Value = serde_json::from_str(
        &cross_seed
            .post_ok(
                "torrents/info",
                "hashes=ffffffffffffffffffffffffffffffffffffffff",
            )
            .await,
    )
    .unwrap();
    assert_eq!(none, json!([]));
    let files: Value = serde_json::from_str(
        &cross_seed
            .post_ok("torrents/files", &format!("hash={HASH}"))
            .await,
    )
    .unwrap();
    assert!(files[0]["name"].is_string() && files[0]["size"].is_number());

    // An injection beside data that is not under the download folder is refused
    let fields = [
        ("category", "tv.cross-seed"),
        ("savepath", "/data/torrents/tv"),
        ("autoTMM", "false"),
        ("tags", "cross-seed"),
        ("contentLayout", "Original"),
        ("skip_checking", "true"),
        ("paused", "true"),
    ];
    let (content_type, body) = multipart(&fields, Some(("Movie.cross-seed.torrent", TORRENT)));
    let response = cross_seed
        .send(Method::POST, "torrents/add", Some(&content_type), body)
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);

    // What it does after an injection
    cross_seed
        .post_ok("torrents/recheck", &format!("hashes={HASH}"))
        .await;
    cross_seed
        .post_ok("torrents/resume", &format!("hashes={HASH}"))
        .await;
    cross_seed
        .post_ok(
            "torrents/addTags",
            &format!("hashes={HASH}&tags=cross-seed"),
        )
        .await;
}

// ---- Contract: phone apps (nzb360 and other qBittorrent remotes) ----

/// Reconstructed from the Web API's documented use, not captured: these apps are closed
/// source, so this is the least certain contract. A remote signs in, polls `sync/maindata`
/// and `transfer/info`, lists with a filter and a sort, and pauses, resumes and deletes.
#[tokio::test]
async fn contract_phone_apps() {
    let mut app = arr_app().await;
    let _mocks = mock_magnet(&mut app, "tv").await;
    let cancel = mock_cancel(&mut app).await;
    let mut phone = Client::new(&app, "okhttp/4.12.0");
    phone.login().await;
    phone
        .post_ok(
            "torrents/add",
            &format!("urls={}&category=tv", encoded(&magnet())),
        )
        .await;

    let data = phone.get_json("sync/maindata?rid=0").await;
    assert_eq!(data["rid"], 1);
    assert_eq!(data["full_update"], true);
    assert_eq!(data["torrents"][HASH]["name"], "Show.S01E01");
    assert_eq!(data["torrents"][HASH]["state"], "downloading");
    assert!(data["categories"]["tv"].is_object());
    assert_eq!(data["server_state"]["connection_status"], "connected");
    assert!(data["server_state"]["dl_info_speed"].is_number());
    assert_eq!(phone.get_json("sync/maindata?rid=1").await["rid"], 2);

    let transfer = phone.get_json("transfer/info").await;
    for field in [
        "dl_info_speed",
        "dl_info_data",
        "up_info_speed",
        "up_info_data",
    ] {
        assert!(transfer[field].is_number(), "{field}");
    }

    let list = phone
        .get_json("torrents/info?filter=all&sort=added_on&reverse=true&limit=20&offset=0")
        .await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert!(list[0]["added_on"].as_i64().unwrap() > 1_600_000_000);
    assert_eq!(
        phone.get_json("torrents/info?filter=completed").await,
        json!([])
    );
    assert_eq!(
        phone.get_json("torrents/info?filter=seeding").await,
        json!([])
    );

    // Both the 4.x and the 5.x names
    for path in [
        "torrents/pause",
        "torrents/resume",
        "torrents/stop",
        "torrents/start",
    ] {
        phone.post_ok(path, &format!("hashes={HASH}")).await;
    }

    phone
        .post_ok(
            "torrents/delete",
            &format!("hashes={HASH}&deleteFiles=true"),
        )
        .await;
    cancel.assert_async().await;
    assert_eq!(phone.get_json("sync/maindata").await["torrents"], json!({}));
}
