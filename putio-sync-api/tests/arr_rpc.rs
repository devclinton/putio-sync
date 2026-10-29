//! The Transmission RPC Radarr and Sonarr talk to, through the router (feature 011,
//! contracts/transmission-rpc.md)

mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::response::Response;
use base64::Engine;
use common::{body_json, body_text, test_app, test_app_with, TestApp};
use mockito::Matcher;
use putio_sync_core::config::ArrConfig;
use serde_json::{json, Value};

const HASH: &str = "c12fe1c06bba254a9dc9f519b335aa7c1367a88a";
const PASSWORD: &str = "arr-password";

fn basic(user: &str, pass: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
    format!("Basic {encoded}")
}

async fn arr_app() -> TestApp {
    test_app_with(|config| {
        config.arr = ArrConfig {
            enabled: true,
            password: Some(PASSWORD.to_string()),
            ..ArrConfig::default()
        };
    })
    .await
}

async fn send(
    app: &TestApp,
    method: Method,
    auth: Option<&str>,
    session: Option<&str>,
    body: &str,
) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri("/transmission/rpc")
        .header(header::USER_AGENT, "Sonarr/4.0.9.2244 (ubuntu 22.04)");
    if let Some(auth) = auth {
        builder = builder.header(header::AUTHORIZATION, auth);
    }
    if let Some(session) = session {
        builder = builder.header("X-Transmission-Session-Id", session);
    }
    app.send(builder.body(Body::from(body.to_string())).unwrap())
        .await
}

/// The session id from the 409 handshake
async fn session_id(app: &TestApp) -> String {
    let auth = basic("putio-sync", PASSWORD);
    let response = send(app, Method::POST, Some(&auth), None, "{}").await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    response.headers()["X-Transmission-Session-Id"]
        .to_str()
        .unwrap()
        .to_string()
}

async fn call(app: &TestApp, session: &str, request: Value) -> Value {
    let auth = basic("putio-sync", PASSWORD);
    let response = send(
        app,
        Method::POST,
        Some(&auth),
        Some(session),
        &request.to_string(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await
}

#[tokio::test]
async fn disabled_is_404() {
    let app = test_app().await;
    let response = send(&app, Method::POST, None, None, "{}").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// Feature 018: turning the client on, or changing its password, needs no restart
#[tokio::test]
async fn settings_apply_without_restart() {
    let app = test_app().await;
    let arr = app
        .state
        .arr
        .clone()
        .expect("built whenever a database exists");
    let mut settings = (*arr.service.settings()).clone();
    settings.enabled = true;
    settings.password = Some(PASSWORD.to_string());
    arr.service.apply(settings.clone());
    session_id(&app).await;

    settings.password = Some("new-password".to_string());
    arr.service.apply(settings.clone());
    let old = basic("putio-sync", PASSWORD);
    let response = send(&app, Method::POST, Some(&old), None, "{}").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    settings.enabled = false;
    arr.service.apply(settings);
    let response = send(&app, Method::POST, None, None, "{}").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn enabled_without_password_is_503() {
    let app = test_app_with(|config| config.arr.enabled = true).await;
    let response = send(&app, Method::POST, None, None, "{}").await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body_text(response).await, "download client has no password");
}

#[tokio::test]
async fn only_post() {
    let app = arr_app().await;
    let session = session_id(&app).await;
    let auth = basic("putio-sync", PASSWORD);
    let response = send(&app, Method::GET, Some(&auth), Some(&session), "").await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}

/// Radarr 6 opens the handshake with a GET, as real Transmission allows
#[tokio::test]
async fn a_get_handshake_gets_the_session_id() {
    let app = arr_app().await;
    let response = send(&app, Method::GET, None, None, "").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let auth = basic("putio-sync", PASSWORD);
    let response = send(&app, Method::GET, Some(&auth), None, "").await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(response.headers().contains_key("X-Transmission-Session-Id"));
}

#[tokio::test]
async fn credentials_are_checked_and_rate_limited() {
    let app = arr_app().await;

    let response = send(&app, Method::POST, None, None, "{}").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers()[header::WWW_AUTHENTICATE],
        r#"Basic realm="putio-sync""#
    );

    let wrong = basic("putio-sync", "guess");
    for _ in 0..5 {
        let response = send(&app, Method::POST, Some(&wrong), None, "{}").await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let right = basic("putio-sync", PASSWORD);
    let response = send(&app, Method::POST, Some(&right), None, "{}").await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn wrong_username_is_refused() {
    let app = arr_app().await;
    let response = send(
        &app,
        Method::POST,
        Some(&basic("admin", PASSWORD)),
        None,
        "{}",
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn handshake_then_session_get() {
    let app = arr_app().await;
    let session = session_id(&app).await;
    assert!(!session.is_empty());

    let auth = basic("putio-sync", PASSWORD);
    let stale = send(&app, Method::POST, Some(&auth), Some("stale"), "{}").await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    assert_eq!(
        stale.headers()["X-Transmission-Session-Id"],
        session.as_str()
    );

    let reply = call(&app, &session, json!({"method": "session-get", "tag": 7})).await;
    assert_eq!(reply["result"], "success");
    assert_eq!(reply["tag"], 7);
    let args = &reply["arguments"];
    assert_eq!(args["version"], "3.00 (putio-sync)");
    assert_eq!(args["rpc-version"], 17);
    assert_eq!(
        args["download-dir"],
        app._dir.path().display().to_string().trim_end_matches('/')
    );
    assert_eq!(args["seedRatioLimited"], false);

    let reply = call(&app, &session, json!({"method": "torrent-verify"})).await;
    assert_eq!(reply["result"], "method not supported");
    let reply = call(&app, &session, json!({"method": "queue-move-top"})).await;
    assert_eq!(reply["result"], "success");

    let response = send(&app, Method::POST, Some(&auth), Some(&session), "not json").await;
    assert_eq!(body_json(response).await["result"], "invalid request");
}

#[tokio::test]
async fn add_get_and_remove_like_sonarr() {
    let mut app = arr_app().await;
    let session = session_id(&app).await;
    let target = app._dir.path().display().to_string();

    app.mock_files_list(0, json!({"files": [], "parent": {"id": 0, "name": "root", "size": 0, "file_type": "FOLDER", "parent_id": 0}, "status": "OK"}))
        .await;
    let create = app
        .putio
        .mock("POST", "/files/create-folder")
        .match_body(Matcher::PartialJson(json!({"name": "sonarr", "parent_id": 0})))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({"file": {"id": 77, "name": "sonarr", "size": 0, "file_type": "FOLDER", "parent_id": 0}, "status": "OK"})
                .to_string(),
        )
        .expect(1)
        .create_async()
        .await;
    let transfer = json!({"id": 900, "name": "Show.S01E01", "size": 1000, "status": "IN_QUEUE",
        "created_at": "2026-10-26T08:00:00", "save_parent_id": 77, "is_private": false});
    let add = app
        .putio
        .mock("POST", "/transfers/add")
        .match_body(Matcher::PartialJson(json!({"save_parent_id": 77})))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({"transfer": transfer, "status": "OK"}).to_string())
        .expect(1)
        .create_async()
        .await;

    let magnet = format!("magnet:?xt=urn:btih:{}&dn=Show.S01E01", HASH.to_uppercase());
    let reply = call(
        &app,
        &session,
        json!({"method": "torrent-add", "arguments": {"filename": magnet, "download-dir": format!("{target}/sonarr"), "paused": false}}),
    )
    .await;
    assert_eq!(reply["result"], "success", "{reply}");
    let added = &reply["arguments"]["torrent-added"];
    assert_eq!(added["hashString"], HASH);
    assert_eq!(added["name"], "Show.S01E01");
    let id = added["id"].as_i64().unwrap();

    let reply = call(
        &app,
        &session,
        json!({"method": "torrent-add", "arguments": {"filename": magnet, "download-dir": format!("{target}/sonarr")}}),
    )
    .await;
    assert_eq!(reply["arguments"]["torrent-duplicate"]["id"], id);
    create.assert_async().await;
    add.assert_async().await;

    let reply = call(
        &app,
        &session,
        json!({"method": "torrent-add", "arguments": {"filename": magnet, "download-dir": "/elsewhere"}}),
    )
    .await;
    assert_eq!(
        reply["result"],
        format!(
            "download-dir must be inside {}",
            target.trim_end_matches('/')
        )
    );

    let reply = call(
        &app,
        &session,
        json!({"method": "torrent-add", "arguments": {"metainfo": "not base64!"}}),
    )
    .await;
    assert_eq!(reply["result"], "invalid torrent file");

    // torrent-get refreshes from Put.io first
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
        .await;
    let reply = call(
        &app,
        &session,
        json!({"method": "torrent-get", "arguments": {"fields": ["id", "hashString"], "ids": [HASH.to_uppercase()]}}),
    )
    .await;
    let torrents = reply["arguments"]["torrents"].as_array().unwrap();
    assert_eq!(torrents.len(), 1);
    assert_eq!(torrents[0]["hashString"], HASH);
    assert_eq!(torrents[0]["status"], 4);
    assert_eq!(torrents[0]["leftUntilDone"], 750);
    assert_eq!(
        torrents[0]["downloadDir"],
        format!("{}/sonarr", target.trim_end_matches('/'))
    );

    // Remove without data: the transfer is cancelled, nothing else on Put.io
    let cancel = app
        .putio
        .mock("POST", "/transfers/cancel")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(1)
        .create_async()
        .await;
    let reply = call(
        &app,
        &session,
        json!({"method": "torrent-remove", "arguments": {"ids": [id], "delete-local-data": false}}),
    )
    .await;
    assert_eq!(reply["result"], "success");
    cancel.assert_async().await;

    let reply = call(&app, &session, json!({"method": "torrent-get"})).await;
    assert_eq!(reply["arguments"]["torrents"], json!([]));
}

#[tokio::test]
async fn torrent_file_add() {
    let mut app = arr_app().await;
    let session = session_id(&app).await;
    let upload = app
        .putio
        .mock("POST", "/files/upload")
        .match_body(Matcher::Regex("name=\"parent_id\"\r\n\r\n0".to_string()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({"transfer": {"id": 5, "name": "Movie.mkv", "status": "IN_QUEUE",
                "created_at": "2026-10-26T08:00:00", "save_parent_id": 0, "is_private": false}, "status": "OK"})
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let torrent =
        b"d4:infod6:lengthi1e4:name9:Movie.mkv12:piece lengthi1e6:pieces20:aaaaaaaaaaaaaaaaaaaaee";
    let metainfo = base64::engine::general_purpose::STANDARD.encode(torrent);
    let reply = call(
        &app,
        &session,
        json!({"method": "torrent-add", "arguments": {"metainfo": metainfo}}),
    )
    .await;
    assert_eq!(reply["result"], "success", "{reply}");
    assert_eq!(reply["arguments"]["torrent-added"]["name"], "Movie.mkv");
    upload.assert_async().await;
}
