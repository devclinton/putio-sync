//! Security hardening: the Host allow-list (DNS rebinding), the password burst guard,
//! the separate limiter for the Transmission RPC, and the response headers.

mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::response::Response;
use base64::Engine as _;
use common::{body_text, test_app, test_app_with, TestApp, API_TOKEN};
use putio_sync_api::signin::hash_password;
use putio_sync_core::config::{ArrConfig, Config};
use std::sync::OnceLock;

fn password_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| hash_password("correct horse battery").unwrap())
}

fn with_password(config: &mut Config) {
    config.sign_in.password.username = "admin".to_string();
    config.sign_in.password.password_hash = password_hash().to_string();
}

fn basic(user: &str, pass: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
    format!("Basic {encoded}")
}

async fn get(app: &TestApp, path: &str, host: Option<&str>, token: bool) -> Response {
    let mut request = Request::builder().method(Method::GET).uri(path);
    if let Some(host) = host {
        request = request.header(header::HOST, host);
    }
    if token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {API_TOKEN}"));
    }
    app.send(request.body(Body::empty()).unwrap()).await
}

// AUTH-01

#[tokio::test]
async fn rebinding_host_names_get_421_everywhere() {
    let app = test_app().await;
    for path in [
        "/health",
        "/",
        "/auth/status",
        "/status",
        "/transmission/rpc",
    ] {
        let response = get(&app, path, Some("evil.example.com:15432"), true).await;
        assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST, "{path}");
    }
    let response = get(&app, "/status", Some("evil.example.com"), true).await;
    let text = body_text(response).await;
    assert!(text.contains("allowed_hosts"), "{text}");
}

#[tokio::test]
async fn lan_names_and_addresses_still_work() {
    let app = test_app().await;
    for host in [
        "192.168.1.5:15432",
        "[::1]:15432",
        "localhost:15432",
        "nas:15432",
        "nas.local:15432",
        "nas.tail1234.ts.net",
    ] {
        let response = get(&app, "/status", Some(host), true).await;
        assert_eq!(response.status(), StatusCode::OK, "{host}");
    }
    // No Host header at all (oneshot, HTTP/2 without authority): not a browser attack
    assert_eq!(
        get(&app, "/status", None, true).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn allowed_hosts_and_public_url_are_accepted() {
    let app = test_app_with(|c| {
        c.api.allowed_hosts = vec!["sync.example.org".to_string(), "*.lan.example.org".into()];
        c.sign_in.public_url = Some("https://public.example.net/".to_string());
    })
    .await;
    for host in [
        "sync.example.org",
        "sync.example.org:8443",
        "x.lan.example.org",
        "public.example.net",
    ] {
        let response = get(&app, "/status", Some(host), true).await;
        assert_eq!(response.status(), StatusCode::OK, "{host}");
    }
    let response = get(&app, "/status", Some("other.example.org"), true).await;
    assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);
}

// AUTH-03

#[tokio::test]
async fn a_burst_of_wrong_passwords_is_cut_off_before_hashing() {
    let app = test_app_with(with_password).await;
    let mut calls = Vec::new();
    for _ in 0..16 {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/auth/password")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"username":"admin","password":"nope"}"#))
            .unwrap();
        calls.push(request);
    }
    let statuses: Vec<StatusCode> = futures_join(calls, &app).await;
    let wrong = statuses
        .iter()
        .filter(|s| **s == StatusCode::UNAUTHORIZED)
        .count();
    let limited = statuses
        .iter()
        .filter(|s| **s == StatusCode::TOO_MANY_REQUESTS)
        .count();
    assert!(
        wrong <= 5,
        "{wrong} wrong passwords were hashed: {statuses:?}"
    );
    assert_eq!(wrong + limited, 16, "{statuses:?}");
}

#[tokio::test]
async fn a_burst_of_wrong_basic_credentials_is_cut_off_before_hashing() {
    let app = test_app_with(with_password).await;
    let mut calls = Vec::new();
    for _ in 0..16 {
        let request = Request::builder()
            .uri("/status")
            .header(header::AUTHORIZATION, basic("admin", "nope"))
            .body(Body::empty())
            .unwrap();
        calls.push(request);
    }
    let statuses = futures_join(calls, &app).await;
    let wrong = statuses
        .iter()
        .filter(|s| **s == StatusCode::UNAUTHORIZED)
        .count();
    assert!(
        wrong <= 5,
        "{wrong} wrong credentials were hashed: {statuses:?}"
    );
}

#[tokio::test]
async fn a_correct_password_does_not_use_up_the_failures() {
    let app = test_app_with(with_password).await;
    for _ in 0..8 {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/auth/password")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"username":"admin","password":"correct horse battery"}"#,
            ))
            .unwrap();
        let response = app.send(request).await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}

async fn futures_join(requests: Vec<Request<Body>>, app: &TestApp) -> Vec<StatusCode> {
    use tower::ServiceExt;
    let mut set = tokio::task::JoinSet::new();
    for request in requests {
        let router = app.router.clone();
        set.spawn(async move { router.oneshot(request).await.unwrap().status() });
    }
    let mut out = Vec::new();
    while let Some(status) = set.join_next().await {
        out.push(status.unwrap());
    }
    out
}

// AUTH-04

#[tokio::test]
async fn wrong_rpc_credentials_do_not_lock_out_the_ui() {
    let app = test_app_with(|config| {
        config.arr = ArrConfig {
            enabled: true,
            password: Some("arr-password".to_string()),
            ..ArrConfig::default()
        };
        config.precheck.magnet_lookup = false;
    })
    .await;
    for _ in 0..8 {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/transmission/rpc")
            .header(header::AUTHORIZATION, basic("putio-sync", "wrong"))
            .body(Body::from("{}"))
            .unwrap();
        let status = app.send(request).await.status();
        assert!(
            status == StatusCode::UNAUTHORIZED || status == StatusCode::TOO_MANY_REQUESTS,
            "{status}"
        );
    }
    assert_eq!(
        get(&app, "/status", None, true).await.status(),
        StatusCode::OK
    );
}

// AUTH-06 and UI-03

#[tokio::test]
async fn api_responses_carry_the_hardening_headers() {
    let app = test_app().await;
    let response = get(&app, "/status", Some("nas"), true).await;
    let h = response.headers();
    assert_eq!(h["cache-control"], "no-store");
    assert_eq!(h["cross-origin-opener-policy"], "same-origin");
    assert_eq!(
        h["permissions-policy"],
        "camera=(), microphone=(), geolocation=()"
    );
    assert!(!h.contains_key("strict-transport-security"));
    // Errors too
    let response = get(&app, "/status", Some("nas"), false).await;
    assert_eq!(response.headers()["cache-control"], "no-store");
    // The UI assets keep their own caching
    let response = get(&app, "/assets/app.js", Some("nas"), false).await;
    assert_eq!(response.headers()["cache-control"], "no-cache");
}

#[tokio::test]
async fn hsts_only_with_an_https_public_url() {
    let https =
        test_app_with(|c| c.sign_in.public_url = Some("https://sync.example.org".into())).await;
    let response = get(&https, "/health", Some("sync.example.org"), false).await;
    assert!(response
        .headers()
        .get("strict-transport-security")
        .is_some_and(|v| v.to_str().unwrap().starts_with("max-age=")));
    let http = test_app_with(|c| c.sign_in.public_url = Some("http://nas:15432".into())).await;
    let response = get(&http, "/health", Some("nas"), false).await;
    assert!(!response.headers().contains_key("strict-transport-security"));
}
