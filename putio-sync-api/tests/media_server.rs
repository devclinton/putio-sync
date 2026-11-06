//! DLNA media server settings and status (feature 022): `GET`/`PUT /settings/media-server`,
//! its preview and `GET /media-server/status`.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app_with, TestApp, API_TOKEN};
use putio_sync_api::router;
use putio_sync_dlna::{MediaServer, Options};
use serde_json::{json, Value};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;

fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

/// An app whose state holds a media server of its own (private SSDP port)
async fn app(
    customize: impl FnOnce(&mut putio_sync_core::config::Config),
) -> (TestApp, Arc<MediaServer>) {
    let mut app = test_app_with(customize).await;
    let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let group = SocketAddrV4::new(
        Ipv4Addr::new(239, 255, 255, 250),
        udp.local_addr().unwrap().port(),
    );
    drop(udp);
    let server = Arc::new(MediaServer::with_options(
        None,
        app._dir.path(),
        Options {
            ssdp_group: group,
            ..Default::default()
        },
    ));
    app.state = app.state.clone().with_media_server(server.clone());
    app.router = router(app.state.clone());
    (app, server)
}

async fn put(app: &TestApp, body: Value) -> (StatusCode, Value) {
    let r = app
        .req(
            Method::PUT,
            "/settings/media-server",
            Some(&app.token),
            Some(body),
        )
        .await;
    let status = r.status();
    (status, body_json(r).await)
}

#[tokio::test]
async fn auth_is_required() {
    let (app, _s) = app(|_| {}).await;
    for (method, path) in [
        (Method::GET, "/settings/media-server"),
        (Method::PUT, "/settings/media-server"),
        (Method::POST, "/settings/media-server/preview"),
        (Method::GET, "/media-server/status"),
    ] {
        let none = app.req(method.clone(), path, None, Some(json!({}))).await;
        assert_eq!(none.status(), StatusCode::UNAUTHORIZED, "{path}");
        let bad = app.req(method, path, Some("wrong"), Some(json!({}))).await;
        assert_eq!(bad.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
    assert_ne!(API_TOKEN, "wrong");
}

#[tokio::test]
async fn the_view_lists_what_can_be_shared() {
    let (app, _s) = app(|c| {
        c.watch.enabled = true;
        c.watch.paths = vec!["/watches/sonarr".into()];
    })
    .await;
    std::fs::create_dir(app._dir.path().join("radarr")).unwrap();
    std::fs::create_dir(app._dir.path().join(".hidden")).unwrap();
    let body = body_json(app.get("/settings/media-server").await).await;
    assert_eq!(body["enabled"], false);
    assert_eq!(body["name"], "putio-sync");
    assert_eq!(body["port"], 8200);
    assert_eq!(body["roots"], json!(["downloads"]));
    assert_eq!(body["putio_folder"], Value::Null);
    let keys: Vec<&str> = body["available_roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        ["downloads", "cat:radarr", "watch:/watches/sonarr", "putio"]
    );
    assert_eq!(body["available_roots"][3]["path"], Value::Null);
}

#[tokio::test]
async fn watch_folders_are_listed_only_while_watching() {
    let (app, _s) = app(|c| {
        c.watch.enabled = false;
        c.watch.paths = vec!["/watches/sonarr".into()];
    })
    .await;
    let body = body_json(app.get("/settings/media-server").await).await;
    let keys: Vec<&str> = body["available_roots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["downloads", "putio"]);
}

#[tokio::test]
async fn bad_values_are_400_naming_the_field() {
    let (app, _s) = app(|_| {}).await;
    let base = body_json(app.get("/settings/media-server").await).await;
    let cases = [
        ("port", json!(0), "dlna.port"),
        ("port", json!(1900), "dlna.port"),
        ("roots", json!(["bogus"]), "dlna.roots"),
        ("roots", json!(["putio"]), "putio_folder"),
        ("roots", json!(["downloads", "downloads"]), "dlna.roots"),
        ("roots", json!(["watch:/not/watched"]), "[watch] paths"),
        ("bind", json!(["not an address"]), "dlna.bind"),
    ];
    for (field, value, expect) in cases {
        let mut body = base.clone();
        body["enabled"] = json!(true);
        body[field] = value.clone();
        let (status, response) = put(&app, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{field}={value}");
        let text = response.to_string();
        assert!(text.contains(expect), "{field}={value}: {text}");
    }
    // Nothing was saved
    let after = body_json(app.get("/settings/media-server").await).await;
    assert_eq!(after["enabled"], false);
}

#[tokio::test]
async fn the_api_port_cannot_be_the_media_port() {
    let (app, _s) = app(|c| {
        c.api.bind = Some(putio_sync_core::listen::BindList::One(
            "127.0.0.1:15999".into(),
        ));
    })
    .await;
    let mut body = body_json(app.get("/settings/media-server").await).await;
    body["enabled"] = json!(true);
    body["port"] = json!(15999);
    let (status, response) = put(&app, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(response.to_string().contains("api"), "{response}");
}

#[tokio::test]
async fn a_save_applies_without_a_restart() {
    let (app, server) = app(|_| {}).await;
    std::fs::write(app._dir.path().join("movie.mp4"), b"abc").unwrap();
    let port = free_port();
    let mut body = body_json(app.get("/settings/media-server").await).await;
    body["enabled"] = json!(true);
    body["port"] = json!(port);
    body["name"] = json!("Den");
    body["bind"] = json!([format!("127.0.0.1:{port}")]);
    let (status, response) = put(&app, body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(response["message"]
        .as_str()
        .unwrap()
        .contains("Applies now"));
    assert!(!response["message"].as_str().unwrap().contains("restart"));

    let desc = reqwest::get(format!("http://127.0.0.1:{port}/description.xml"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(desc.contains("<friendlyName>Den</friendlyName>"));
    let status = body_json(app.get("/media-server/status").await).await;
    assert_eq!(status["state"], "running");
    assert_eq!(
        status["addresses"],
        json!([format!("http://127.0.0.1:{port}/description.xml")])
    );
    assert_eq!(status["streams_active"], 0);
    assert_eq!(status["putio"]["state"], "off");

    // Saved to the config file too
    let saved = std::fs::read_to_string(app._dir.path().join("config.toml")).unwrap();
    assert!(saved.contains("[dlna]") && saved.contains("Den"), "{saved}");

    // Renaming applies to the running server
    body["name"] = json!("Den 2");
    let (status, _) = put(&app, body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let desc = reqwest::get(format!("http://127.0.0.1:{port}/description.xml"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(desc.contains("<friendlyName>Den 2</friendlyName>"));

    // Turning it off closes the port
    body["enabled"] = json!(false);
    let (status, _) = put(&app, body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body_json(app.get("/media-server/status").await).await["state"],
        "off"
    );
    assert!(
        reqwest::get(format!("http://127.0.0.1:{port}/description.xml"))
            .await
            .is_err()
    );
    server.stop().await;
}

#[tokio::test]
async fn a_taken_port_shows_as_failed_and_recovers() {
    let (app, server) = app(|_| {}).await;
    let blocker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = blocker.local_addr().unwrap().port();
    let mut body = body_json(app.get("/settings/media-server").await).await;
    body["enabled"] = json!(true);
    body["port"] = json!(port);
    body["bind"] = json!([format!("127.0.0.1:{port}")]);
    // The save succeeds: the daemon stays healthy
    let (status, _) = put(&app, body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let s = body_json(app.get("/media-server/status").await).await;
    assert_eq!(s["state"], "failed");
    assert!(
        s["reason"].as_str().unwrap().contains(&port.to_string()),
        "{s}"
    );
    assert_eq!(app.get("/status").await.status(), StatusCode::OK);

    drop(blocker);
    let (status, _) = put(&app, body).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body_json(app.get("/media-server/status").await).await["state"],
        "running"
    );
    server.stop().await;
}

#[tokio::test]
async fn preview_says_what_a_save_would_change_and_changes_nothing() {
    let (app, _s) = app(|_| {}).await;
    let mut body = body_json(app.get("/settings/media-server").await).await;
    body["name"] = json!("Preview");
    let r = app
        .req(
            Method::POST,
            "/settings/media-server/preview",
            Some(&app.token),
            Some(body.clone()),
        )
        .await;
    assert_eq!(r.status(), StatusCode::OK);
    let preview = body_json(r).await;
    assert_eq!(preview["valid"], true);
    assert_eq!(preview["changes"][0]["key"], "name");
    assert_eq!(preview["changes"][0]["live"], true);
    body["port"] = json!(0);
    let r = app
        .req(
            Method::POST,
            "/settings/media-server/preview",
            Some(&app.token),
            Some(body),
        )
        .await;
    assert_eq!(body_json(r).await["valid"], false);
    let after = body_json(app.get("/settings/media-server").await).await;
    assert_eq!(after["name"], "putio-sync");
}

#[tokio::test]
async fn status_without_a_media_server_is_off() {
    let app = test_app_with(|_| {}).await;
    let s = body_json(app.get("/media-server/status").await).await;
    assert_eq!(s["state"], "off");
    assert_eq!(s["addresses"], json!([]));
}
