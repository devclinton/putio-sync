//! Tailscale awareness (feature 036) against a fake tailscaled on a temp unix socket:
//! `GET /tailscale`, Tailscale sign-in through `serve`'s headers and `whois`, and the
//! `tailscale` settings group. Nothing here needs Tailscale installed.

mod common;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use common::{body_json, test_app_with, TestApp, API_TOKEN};
use putio_sync_api::router;
use putio_sync_core::config::Config;
use putio_sync_tailscale::{Cli, TailscaleService};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const RUNNING: &str = r#"{
    "Version": "1.76.1",
    "TUN": true,
    "BackendState": "Running",
    "TailscaleIPs": ["100.101.102.103", "fd7a:115c:a1e0::1"],
    "Self": {"HostName": "nas", "DNSName": "nas.tail1234.ts.net.", "UserID": 7},
    "CurrentTailnet": {"Name": "example.com", "MagicDNSSuffix": "tail1234.ts.net", "MagicDNSEnabled": true},
    "CertDomains": ["nas.tail1234.ts.net"],
    "User": {"7": {"LoginName": "alice@example.com", "DisplayName": "Alice"}}
}"#;
const NEEDS_LOGIN: &str = r#"{"Version":"1.76.1","TUN":true,"BackendState":"NeedsLogin"}"#;
const SERVE: &str = r#"{"TCP":{"443":{"HTTPS":true}},"Web":{"nas.tail1234.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:15432"}}}}}"#;
const ALICE: &str = r#"{"Node":{"Name":"laptop.tail1234.ts.net.","Tags":[]},"UserProfile":{"LoginName":"alice@example.com","DisplayName":"Alice"}}"#;
const TAGGED: &str = r#"{"Node":{"Name":"server.tail1234.ts.net.","Tags":["tag:server"]},"UserProfile":{"LoginName":"tagged-devices"}}"#;

/// The tailnet peers the fake knows
const ALICE_IP: [u8; 4] = [100, 64, 0, 5];
const TAGGED_IP: [u8; 4] = [100, 64, 0, 6];
const STRANGER_IP: [u8; 4] = [100, 64, 0, 7];

/// A fake tailscaled LocalAPI answering by path
fn fake_tailscaled(dir: &std::path::Path, status: &'static str) -> PathBuf {
    let socket = dir.join("tailscaled.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    tokio::spawn(async move {
        while let Ok((mut conn, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut req = Vec::new();
                let mut buf = [0u8; 1024];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    match conn.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => req.extend_from_slice(&buf[..n]),
                    }
                }
                let req = String::from_utf8_lossy(&req).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("");
                let (code, body) = if path.starts_with("/localapi/v0/status") {
                    (200, status)
                } else if path.starts_with("/localapi/v0/serve-config") {
                    (200, SERVE)
                } else if path.ends_with("100.64.0.5") {
                    (200, ALICE)
                } else if path.ends_with("100.64.0.6") {
                    (200, TAGGED)
                } else {
                    (404, "not found")
                };
                let resp = format!("HTTP/1.0 {code} X\r\n\r\n{body}");
                let _ = conn.write_all(resp.as_bytes()).await;
            });
        }
    });
    socket
}

/// An app whose Tailscale service talks to `socket` only (never a real CLI)
async fn app(socket: PathBuf, customize: impl FnOnce(&mut Config)) -> TestApp {
    let mut app = test_app_with(|c| {
        c.tailscale.socket = Some(socket.display().to_string());
        customize(c);
    })
    .await;
    let service = TailscaleService::new(socket, Cli::Off).with_timeout(Duration::from_millis(500));
    app.state = app.state.clone().with_tailscale(Arc::new(service));
    app.router = router(app.state.clone());
    app
}

async fn running_app(customize: impl FnOnce(&mut Config)) -> (tempfile::TempDir, TestApp) {
    let dir = short_dir();
    let socket = fake_tailscaled(dir.path(), RUNNING);
    (dir, app(socket, customize).await)
}

fn sign_in_for(users: &[&str]) -> impl FnOnce(&mut Config) {
    let users: Vec<String> = users.iter().map(|u| u.to_string()).collect();
    move |c| {
        c.tailscale.identity_sign_in = true;
        c.tailscale.allowed_users = users;
    }
}

/// `GET path` from `peer` with no token, and `headers`
async fn from_peer(
    app: &TestApp,
    path: &str,
    peer: Option<SocketAddr>,
    headers: &[(&str, &str)],
) -> Response {
    let mut builder = Request::builder().method(Method::GET).uri(path);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let mut request = builder.body(Body::empty()).unwrap();
    if let Some(peer) = peer {
        request.extensions_mut().insert(ConnectInfo(peer));
    }
    app.send(request).await
}

fn loopback() -> Option<SocketAddr> {
    Some(SocketAddr::from(([127, 0, 0, 1], 40000)))
}

fn peer(ip: [u8; 4]) -> Option<SocketAddr> {
    Some(SocketAddr::from((ip, 40000)))
}

type Headers<'a> = &'a [(&'a str, &'a str)];

const SERVE_ALICE: Headers<'static> = &[
    ("Tailscale-User-Login", "alice@example.com"),
    ("Tailscale-User-Name", "Alice"),
];

#[tokio::test]
async fn details_when_running() {
    let (_dir, app) = running_app(|_| {}).await;
    let response = app.get("/tailscale").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["state"], "running");
    assert_eq!(body["dns_name"], "nas.tail1234.ts.net");
    assert_eq!(body["magic_dns"], true);
    assert_eq!(body["port"], 15432);
    assert_eq!(body["urls"][0], "https://nas.tail1234.ts.net");
    assert_eq!(body["tailnet_bind"], "100.101.102.103:15432");
    assert_eq!(body["tailnet_bind_error"], Value::Null);
    assert_eq!(body["settings"]["identity_sign_in"], false);
    assert_eq!(body["identity"], Value::Null);
}

#[tokio::test]
async fn details_when_not_found_or_signed_out() {
    let dir = short_dir();
    let app = app(dir.path().join("missing.sock"), |_| {}).await;
    let body = body_json(app.get("/tailscale").await).await;
    assert_eq!(body["state"], "not_found");
    assert_eq!(body["urls"], json!([]));
    assert!(
        body["hints"][0].as_str().unwrap().contains("socket"),
        "{body}"
    );
    assert!(body["tailnet_bind_error"]
        .as_str()
        .unwrap()
        .contains("isn't on"));

    let socket = fake_tailscaled(dir.path(), NEEDS_LOGIN);
    let app = self::app(socket, |_| {}).await;
    let body = body_json(app.get("/tailscale").await).await;
    assert_eq!(body["state"], "needs_login");
    assert!(
        body["hints"][0].as_str().unwrap().contains("tailscale up"),
        "{body}"
    );
}

#[tokio::test]
async fn details_need_the_token_when_sign_in_is_off() {
    let (_dir, app) = running_app(|_| {}).await;
    let response = from_peer(&app, "/tailscale", loopback(), SERVE_ALICE).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn serve_headers_from_loopback_sign_in() {
    let (_dir, app) = running_app(sign_in_for(&["alice@example.com"])).await;
    let response = from_peer(&app, "/tailscale", loopback(), SERVE_ALICE).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["identity"]["login"], "alice@example.com");
    assert_eq!(body["identity"]["via"], "serve");

    let response = from_peer(&app, "/auth/status", loopback(), SERVE_ALICE).await;
    let body = body_json(response).await;
    assert_eq!(body["tailscale"]["login"], "alice@example.com");
    assert_eq!(body["tailscale_sign_in"], true);
}

#[tokio::test]
async fn refusals() {
    let (_dir, app) = running_app(sign_in_for(&["alice@example.com"])).await;
    let bob = &[("Tailscale-User-Login", "bob@example.com")];
    let cases: &[(&str, Option<SocketAddr>, Headers)] = &[
        ("not on the allow-list", loopback(), bob),
        (
            "headers from the LAN",
            Some(SocketAddr::from(([192, 168, 1, 20], 40000))),
            SERVE_ALICE,
        ),
        ("no peer", None, SERVE_ALICE),
        ("tagged device", peer(TAGGED_IP), &[]),
        ("unknown peer", peer(STRANGER_IP), &[]),
        ("loopback without headers", loopback(), &[]),
    ];
    for (case, from, headers) in cases {
        let response = from_peer(&app, "/status", *from, headers).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{case}");
    }
}

#[tokio::test]
async fn whois_signs_in_tailnet_peers() {
    let (_dir, app) = running_app(sign_in_for(&["Alice@Example.com"])).await;
    let response = from_peer(&app, "/tailscale", peer(ALICE_IP), &[]).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["identity"]["via"], "whois");
    assert_eq!(body["identity"]["device"], "laptop");
}

#[tokio::test]
async fn daemon_down_or_nobody_allowed_falls_back_to_the_token() {
    let dir = short_dir();
    let app = app(
        dir.path().join("missing.sock"),
        sign_in_for(&["alice@example.com"]),
    )
    .await;
    let response = from_peer(&app, "/status", peer(ALICE_IP), &[]).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let (_dir, app) = running_app(sign_in_for(&[])).await;
    let response = from_peer(&app, "/status", loopback(), SERVE_ALICE).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    // The token always works
    assert_eq!(app.get("/status").await.status(), StatusCode::OK);
    let response = app.req(Method::GET, "/status", Some(API_TOKEN), None).await;
    assert_eq!(response.status(), StatusCode::OK);
}

async fn put(app: &TestApp, body: Value) -> (StatusCode, Value) {
    let response = app
        .req(
            Method::PUT,
            "/settings/tailscale",
            Some(&app.token),
            Some(body),
        )
        .await;
    let status = response.status();
    (status, body_json(response).await)
}

#[tokio::test]
async fn settings_round_trip() {
    let (_dir, app) = running_app(|_| {}).await;
    let mut body = body_json(app.get("/settings/tailscale").await).await;
    assert_eq!(body["tailnet_only"], false);
    assert_eq!(body["allowed_users"], json!([]));

    body["identity_sign_in"] = json!(true);
    body["allowed_users"] = json!([" alice@example.com ", ""]);
    let (status, response) = put(&app, body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let message = response["message"].as_str().unwrap();
    assert!(message.contains("Applies now"), "{message}");
    let saved = body_json(app.get("/settings/tailscale").await).await;
    assert_eq!(saved["allowed_users"], json!(["alice@example.com"]));
    // Sign-in applies at once
    let response = from_peer(&app, "/status", loopback(), SERVE_ALICE).await;
    assert_eq!(response.status(), StatusCode::OK);

    body["tailnet_only"] = json!(true);
    let (status, response) = put(&app, body).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(
        response["message"].as_str().unwrap().contains("restart"),
        "{response}"
    );
}

#[tokio::test]
async fn tailnet_only_needs_a_tailnet_address() {
    let dir = short_dir();
    let socket = fake_tailscaled(dir.path(), NEEDS_LOGIN);
    let app = app(socket, |_| {}).await;
    let mut body = body_json(app.get("/settings/tailscale").await).await;
    body["tailnet_only"] = json!(true);
    let (status, response) = put(&app, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        response["error"].as_str().unwrap().contains("signed out"),
        "{response}"
    );
    let after = body_json(app.get("/settings/tailscale").await).await;
    assert_eq!(after["tailnet_only"], false);
}

/// The port reported follows what is listened on (feature 037): the configured port
/// until something binds, then the bound one (e.g. `server --port N`)
#[tokio::test]
async fn port_follows_the_listener() {
    use putio_sync_api::listen::{Bound, Supervisor};
    use putio_sync_core::listen::BindList;

    let (_dir, app) = running_app(|c| {
        c.api.bind = Some(BindList::from_entries(vec!["127.0.0.1:18001".into()]));
    })
    .await;
    assert_eq!(app.state.api_port().await, 18001);
    let body = body_json(app.get("/tailscale").await).await;
    assert_eq!(body["port"], 18001);

    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let bound = Bound::default();
    let state = app.state.clone().with_listening(bound.clone());
    let handle = Supervisor::spawn(
        "API",
        vec![format!("127.0.0.1:{port}").parse().unwrap()],
        state.tailscale.clone(),
        axum::Router::new(),
        bound,
    );
    assert!(handle.first_bound(Duration::from_secs(2)).await.is_some());
    assert_eq!(state.api_port().await, port);
    let app = TestApp {
        router: router(state.clone()),
        state,
        ..app
    };
    let body = body_json(app.get("/tailscale").await).await;
    assert_eq!(body["port"], port);
    assert_eq!(body["tailnet_bind"], format!("100.101.102.103:{port}"));
    handle.stop(Duration::from_millis(200)).await;
}

/// A temp dir under /tmp: unix socket paths must stay under about 104 bytes, which a
/// long `TMPDIR` breaks
fn short_dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("ts")
        .tempdir_in("/tmp")
        .unwrap()
}
