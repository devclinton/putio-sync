//! Token required on every protected route, failed attempts rate limited, no CORS
//! (feature 004, R3).

mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use common::{body_json, body_text, test_app, test_app_without_token, API_TOKEN};
use serde_json::json;

#[tokio::test]
async fn health_is_public() {
    let app = test_app().await;
    let response = app.req(Method::GET, "/health", None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["status"], "ok");
}

#[tokio::test]
async fn no_token_configured_refuses_with_setting_name() {
    let app = test_app_without_token().await;
    let response = app
        .req(Method::GET, "/local/queue", Some("anything"), None)
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = body_json(response).await;
    assert_eq!(body["error"], "API token not configured");
    assert!(
        body["details"]
            .as_str()
            .unwrap()
            .contains("PUTIO_SYNC_API_TOKEN"),
        "{body}"
    );
}

#[tokio::test]
async fn missing_token_401_with_www_authenticate() {
    let app = test_app().await;
    let response = app.req(Method::GET, "/local/queue", None, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
    assert_eq!(body_json(response).await["error"], "Unauthorized");
}

#[tokio::test]
async fn wrong_token_401() {
    let app = test_app().await;
    let response = app
        .req(Method::GET, "/local/queue", Some("wrong-token"), None)
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(response).await["error"], "Unauthorized");
}

#[tokio::test]
async fn correct_token_200() {
    let app = test_app().await;
    assert_eq!(app.get("/local/queue").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn sixth_failure_429_with_retry_after() {
    let app = test_app().await;
    for n in 1..=5 {
        let response = app
            .req(Method::GET, "/local/queue", Some("wrong-token"), None)
            .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "attempt {n}");
    }

    let response = app
        .req(Method::GET, "/local/queue", Some("wrong-token"), None)
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after: u64 = response.headers()[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=60).contains(&retry_after), "{retry_after}");
    assert_eq!(
        body_json(response).await["error"],
        "Too many failed authentication attempts"
    );

    // Refused before the token is compared, so the right token is refused too
    let response = app.get("/local/queue").await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn legacy_api_routes_gone() {
    let app = test_app().await;
    for path in ["/api/files", "/api/transfers", "/api/queue"] {
        let status = app.get(path).await.status();
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::UNAUTHORIZED,
            "{path}: {status}"
        );
    }
}

#[tokio::test]
async fn no_cors_headers() {
    let app = test_app().await;
    for (method, path, token) in [
        (Method::GET, "/health", None),
        (Method::GET, "/local/queue", Some(common::API_TOKEN)),
        (Method::OPTIONS, "/local/queue", None),
    ] {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::ORIGIN, "http://evil")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET");
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = app.send(request.body(Body::empty()).unwrap()).await;
        assert!(
            !response
                .headers()
                .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            "{path}"
        );
    }
}

#[tokio::test]
async fn missing_token_never_rate_limited() {
    // A browser asks for paths nobody links to (robots.txt, touch icons) without a
    // token; those 401s must not lock the operator out (only wrong tokens count)
    let app = test_app().await;
    for n in 0..30 {
        let path = ["/robots.txt", "/apple-touch-icon.png", "/local/queue"][n % 3];
        let response = app.req(Method::GET, path, None, None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path} #{n}");
        assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
    }
    assert_eq!(app.get("/local/queue").await.status(), StatusCode::OK);

    // ...and they don't add to real failures either
    for n in 1..=4 {
        let response = app
            .req(Method::GET, "/local/queue", Some("wrong-token"), None)
            .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "attempt {n}");
    }
    for _ in 0..10 {
        let response = app.req(Method::GET, "/favicon.ico", None, None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    assert_eq!(app.get("/local/queue").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn unknown_path_without_token_401() {
    let app = test_app().await;
    let response = app.req(Method::GET, "/robots.txt", None, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(body_json(response).await["error"], "Unauthorized");
}

async fn login(app: &common::TestApp, token: &str) -> axum::response::Response {
    app.req(
        Method::POST,
        "/auth/login",
        None,
        Some(json!({ "token": token })),
    )
    .await
}

#[tokio::test]
async fn auth_status_public() {
    let app = test_app().await;
    let response = app.req(Method::GET, "/auth/status", None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await,
        json!({ "token_configured": true })
    );

    let app = test_app_without_token().await;
    let response = app.req(Method::GET, "/auth/status", None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await,
        json!({ "token_configured": false })
    );
}

#[tokio::test]
async fn login_204_on_match() {
    let app = test_app().await;
    let response = login(&app, API_TOKEN).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(body_text(response).await, "");
}

#[tokio::test]
async fn login_401_on_mismatch() {
    let app = test_app().await;
    for n in 1..=5 {
        let response = login(&app, "wrong-token").await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "attempt {n}");
        assert_eq!(
            body_json(response).await,
            json!({ "error": "Unauthorized" })
        );
    }

    let response = login(&app, "wrong-token").await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().contains_key(header::RETRY_AFTER));

    // Checked before the compare, so the right token waits too; so does the API
    assert_eq!(
        login(&app, API_TOKEN).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        app.get("/local/queue").await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn login_401_when_not_configured() {
    let app = test_app_without_token().await;
    let response = login(&app, "anything").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = body_json(response).await;
    assert_eq!(body["error"], "API token not configured");
    assert!(
        body["details"]
            .as_str()
            .unwrap()
            .contains("PUTIO_SYNC_API_TOKEN"),
        "{body}"
    );
}
