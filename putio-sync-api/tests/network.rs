//! Listen addresses (feature 037): the supervisor serving the real router on several
//! addresses, and the network settings routes.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_app_with, TestApp, API_TOKEN};
use putio_sync_api::listen::{Bound, Supervisor};
use putio_sync_api::router;
use putio_sync_core::listen::ListenAddr;
use serde_json::{json, Value};
use std::net::{SocketAddr, TcpListener};
use std::time::Duration;

/// A port free on 127.0.0.1 and, when the host has IPv6 loopback, on [::1]
fn free_port() -> (u16, bool) {
    for _ in 0..20 {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        match TcpListener::bind(("::1", port)) {
            Ok(_) => return (port, true),
            Err(e) if e.kind() == std::io::ErrorKind::AddrNotAvailable => return (port, false),
            Err(_) => continue,
        }
    }
    panic!("no free port");
}

/// The test app's router served by a supervisor on `entries`
async fn serve(app: &TestApp, entries: &[&str]) -> (putio_sync_api::listen::ListenerHandle, Bound) {
    let entries: Vec<ListenAddr> = entries.iter().map(|e| e.parse().unwrap()).collect();
    let bound = Bound::default();
    let handle = Supervisor::spawn(
        "API",
        entries.clone(),
        app.state.tailscale.clone(),
        router(app.state.clone().with_listening(bound.clone())),
        bound.clone(),
    );
    for _ in 0..100 {
        if bound.get().len() == entries.len() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    (handle, bound)
}

#[tokio::test]
async fn listens_on_every_address() {
    let app = test_app().await;
    let (port, v6) = free_port();
    let v4 = format!("127.0.0.1:{port}");
    let v6e = format!("[::1]:{port}");
    let mut entries = vec![v4.as_str()];
    if v6 {
        entries.push(v6e.as_str());
    }
    let (handle, bound) = serve(&app, &entries).await;
    let mut want: Vec<SocketAddr> = vec![v4.parse().unwrap()];
    if v6 {
        want.push(v6e.parse().unwrap());
    }
    assert_eq!(bound.get(), want);

    let client = reqwest::Client::new();
    for addr in &want {
        let response = client
            .get(format!("http://{addr}/health"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{addr}");
    }

    // /listen reports the bound list and its port
    let listen: Value = client
        .get(format!("http://{v4}/listen"))
        .bearer_auth(API_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(listen["api"]["port"], port);
    assert_eq!(listen["api"]["listening"][0], v4);
    handle.stop(Duration::from_secs(2)).await;
}

/// POST /settings/network/check through a served router, so `ListenedOn` is set
async fn check_served(addr: &str, api_bind: Value) -> Value {
    reqwest::Client::new()
        .post(format!("http://{addr}/settings/network/check"))
        .bearer_auth(API_TOKEN)
        .json(&json!({ "api_bind": api_bind, "metrics_bind": ["0.0.0.0:9742"] }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn check_says_whether_this_page_stays() {
    let app = test_app().await;
    let (port, _) = free_port();
    let here = format!("127.0.0.1:{port}");
    let (handle, _) = serve(&app, &[here.as_str()]).await;

    let kept = check_served(&here, json!([here])).await;
    assert_eq!(kept["valid"], true, "{kept}");
    assert_eq!(kept["this_page"], "kept");
    assert_eq!(kept["listened_on"], here);
    assert_eq!(kept["ui_at"], json!([format!("http://{here}/")]));

    // A wildcard on the same port still takes 127.0.0.1
    let wild = check_served(&here, json!([format!("0.0.0.0:{port}")])).await;
    assert_eq!(wild["this_page"], "kept", "{wild}");

    // Another port is another listener
    let other = check_served(&here, json!([format!("127.0.0.1:{}", port + 1)])).await;
    assert_eq!(other["this_page"], "cut_off", "{other}");
    handle.stop(Duration::from_secs(2)).await;

    // Without a served listener there is nothing to compare with
    let response = app
        .post_json(
            "/settings/network/check",
            json!({ "api_bind": ["127.0.0.1:15432"], "metrics_bind": ["0.0.0.0:9742"] }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["this_page"], "unknown");
}

async fn put(app: &TestApp, body: Value) -> (StatusCode, Value) {
    let response = app
        .req(
            Method::PUT,
            "/settings/network",
            Some(&app.token),
            Some(body),
        )
        .await;
    let status = response.status();
    (status, body_json(response).await)
}

fn saved_file(app: &TestApp) -> toml::Value {
    let text = std::fs::read_to_string(app._dir.path().join("config.toml")).unwrap();
    text.parse().unwrap()
}

#[tokio::test]
async fn settings_round_trip() {
    let app = test_app().await;
    let body = body_json(app.get("/settings/network").await).await;
    assert_eq!(
        body,
        json!({ "api_bind": ["0.0.0.0:15432"], "metrics_bind": ["0.0.0.0:9742"] })
    );

    let (status, response) = put(
        &app,
        json!({ "api_bind": ["127.0.0.1:15432", " [::1]:15432 ", ""], "metrics_bind": ["127.0.0.1:9742"] }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let message = response["message"].as_str().unwrap();
    assert!(
        message.contains("Needs a restart: api_bind, metrics_bind."),
        "{message}"
    );
    let saved = body_json(app.get("/settings/network").await).await;
    assert_eq!(saved["api_bind"], json!(["127.0.0.1:15432", "[::1]:15432"]));

    // A list in the file; one entry as a plain string
    let file = saved_file(&app);
    assert_eq!(
        file["api"]["bind"],
        toml::Value::Array(vec!["127.0.0.1:15432".into(), "[::1]:15432".into()])
    );
    assert_eq!(file["metrics"]["bind"], toml::Value::from("127.0.0.1:9742"));
}

#[tokio::test]
async fn refusals() {
    let app = test_app().await;
    let (status, body) = put(
        &app,
        json!({ "api_bind": ["nas:port"], "metrics_bind": ["0.0.0.0:9742"] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("invalid api.bind 'nas:port'"),
        "{body}"
    );

    // Nothing to listen on now: an interface that doesn't exist
    let (status, body) = put(
        &app,
        json!({ "api_bind": ["nosuchif0:15432"], "metrics_bind": ["0.0.0.0:9742"] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let error = body["error"].as_str().unwrap();
    assert!(
        error.starts_with("No API address gives an address now: nosuchif0:15432 ("),
        "{error}"
    );
    // The check says the same without saving
    let check = body_json(
        app.post_json(
            "/settings/network/check",
            json!({ "api_bind": ["nosuchif0:15432"], "metrics_bind": ["0.0.0.0:9742"] }),
        )
        .await,
    )
    .await;
    assert_eq!(check["valid"], false);
    assert!(check["api"][0]["waiting"].is_string(), "{check}");

    // The lists must not overlap
    let (status, body) = put(
        &app,
        json!({ "api_bind": ["0.0.0.0:9742"], "metrics_bind": ["0.0.0.0:9742"] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // None of it was saved
    let saved = body_json(app.get("/settings/network").await).await;
    assert_eq!(saved["api_bind"], json!(["0.0.0.0:15432"]));

    // Every route needs the token
    for (method, path) in [
        (Method::GET, "/listen"),
        (Method::GET, "/settings/network"),
        (Method::PUT, "/settings/network"),
        (Method::POST, "/settings/network/check"),
        (Method::POST, "/settings/network/preview"),
    ] {
        let response = app.req(method, path, None, Some(json!({}))).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn listen_shape_and_loopback_identity_warning() {
    let app = test_app_with(|c| {
        c.tailscale.identity_sign_in = true;
        c.tailscale.allowed_users = vec!["alice@example.com".to_string()];
    })
    .await;
    let body = body_json(app.get("/listen").await).await;
    assert_eq!(body["api"]["configured"], json!(["0.0.0.0:15432"]));
    assert_eq!(body["api"]["effective"], json!(["0.0.0.0:15432"]));
    assert_eq!(
        body["api"]["entries"][0],
        json!({ "entry": "0.0.0.0:15432", "addresses": ["0.0.0.0:15432"] })
    );
    assert_eq!(body["api"]["listening"], json!([]));
    assert_eq!(body["api"]["port"], 15432);
    assert_eq!(body["api"]["tailnet_only"], false);
    assert_eq!(body["metrics"]["configured"], json!(["0.0.0.0:9742"]));
    assert_eq!(body["engine"], "server");
    assert_eq!(body["server_flags"], false);

    let check = body_json(
        app.post_json(
            "/settings/network/check",
            json!({ "api_bind": ["127.0.0.1:15432"], "metrics_bind": ["0.0.0.0:9742"] }),
        )
        .await,
    )
    .await;
    assert!(
        check["warnings"][0]
            .as_str()
            .unwrap()
            .contains("Only `tailscale serve` should reach putio-sync on loopback"),
        "{check}"
    );

    // The preview is the assistant's generic one
    let preview = body_json(
        app.post_json(
            "/settings/network/preview",
            json!({ "api_bind": ["127.0.0.1:15432"], "metrics_bind": ["0.0.0.0:9742"] }),
        )
        .await,
    )
    .await;
    assert_eq!(preview["valid"], true, "{preview}");
    assert_eq!(preview["changes"][0]["key"], "api_bind");
}
