//! Sign-in options (feature 019): HTTP Basic, the password form and its session cookie,
//! CSRF, OAuth with Google, GitHub and Discord against a mock provider, the masked
//! settings, and that the token works as before.

mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::response::Response;
use base64::Engine as _;
use common::{body_json, body_text, test_app_with, TestApp, API_TOKEN};
use putio_sync_api::signin::hash_password;
use putio_sync_core::config::Config;
use putio_sync_core::sign_in::Provider;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

const USER: &str = "admin";
const PASSWORD: &str = "correct horse battery";
const CLIENT_SECRET: &str = "CLIENTSECRET0123456789";

/// One hash for the whole file: argon2 is slow on purpose
fn password_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| hash_password(PASSWORD).unwrap())
}

fn with_password(config: &mut Config) {
    config.sign_in.password.username = USER.to_string();
    config.sign_in.password.password_hash = password_hash().to_string();
}

fn with_provider(config: &mut Config, provider: Provider, allowed: &[&str]) {
    let p = config.sign_in.provider_mut(provider);
    p.enabled = true;
    p.client_id = format!("{}-client", provider.as_str());
    p.client_secret = CLIENT_SECRET.to_string();
    p.allowed_users = allowed.iter().map(|s| s.to_string()).collect();
}

fn basic(user: &str, pass: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
    format!("Basic {encoded}")
}

async fn send(
    app: &TestApp,
    method: Method,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<Value>,
) -> Response {
    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let body = match body {
        Some(json) => {
            builder = builder.header("Content-Type", "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    app.send(builder.body(body).unwrap()).await
}

/// The `name=value` of cookie `name` in the response's `Set-Cookie` headers, with the
/// whole header
fn set_cookie(response: &Response, name: &str) -> Option<(String, String)> {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .map(|v| {
            let pair = v.split(';').next().unwrap().to_string();
            (pair, v.to_string())
        })
}

fn location(response: &Response) -> String {
    response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string()
}

/// Sign in with the password form; the `Cookie` header value to send afterwards
async fn password_session(app: &TestApp) -> String {
    let response = send(
        app,
        Method::POST,
        "/auth/password",
        &[],
        Some(json!({"username": USER, "password": PASSWORD})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let (pair, full) = set_cookie(&response, "putio_sync_session").expect("a session cookie");
    assert!(full.contains("HttpOnly"), "{full}");
    assert!(full.contains("SameSite=Strict"), "{full}");
    assert!(full.contains("Path=/"), "{full}");
    assert!(full.contains("Max-Age=604800"), "{full}");
    assert!(!full.contains("Secure"), "{full}");
    pair
}

async fn csrf_for(app: &TestApp, cookie: &str) -> String {
    let status = body_json(
        send(
            app,
            Method::GET,
            "/auth/status",
            &[("Cookie", cookie)],
            None,
        )
        .await,
    )
    .await;
    status["user"]["csrf"]
        .as_str()
        .expect("a CSRF token")
        .to_string()
}

#[tokio::test]
async fn basic_auth_lets_scripts_in() {
    let app = test_app_with(with_password).await;
    let good = basic(USER, PASSWORD);
    for _ in 0..2 {
        // The second is answered from the remembered credential
        let response = send(
            &app,
            Method::GET,
            "/settings/sign-in",
            &[("Authorization", &good)],
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    // Writes too: scripts send no Sec-Fetch-Site
    let response = send(
        &app,
        Method::POST,
        "/settings/sign-in/sign-out-everywhere",
        &[("Authorization", &good)],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    // A browser's cross-site write is refused
    let response = send(
        &app,
        Method::POST,
        "/settings/sign-in/sign-out-everywhere",
        &[("Authorization", &good), ("Sec-Fetch-Site", "cross-site")],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    for wrong in [
        basic(USER, "nope"),
        basic("root", PASSWORD),
        "Basic !!!".to_string(),
    ] {
        let response = send(
            &app,
            Method::GET,
            "/settings/sign-in",
            &[("Authorization", &wrong)],
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{wrong}");
    }
    // The token still works alongside
    let response = app.get("/settings/sign-in").await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn wrong_basic_credentials_are_rate_limited() {
    let app = test_app_with(with_password).await;
    let wrong = basic(USER, "nope");
    for _ in 0..5 {
        let response = send(
            &app,
            Method::GET,
            "/settings/sign-in",
            &[("Authorization", &wrong)],
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let response = send(
        &app,
        Method::GET,
        "/settings/sign-in",
        &[("Authorization", &basic(USER, PASSWORD))],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn the_password_form_signs_in_with_a_session() {
    let app = test_app_with(with_password).await;
    let status = body_json(send(&app, Method::GET, "/auth/status", &[], None).await).await;
    assert_eq!(status["sign_in"]["password"], true);
    assert_eq!(status["sign_in"]["providers"], json!([]), "{status}");
    assert!(status.get("user").is_none());

    let cookie = password_session(&app).await;
    let status = body_json(
        send(
            &app,
            Method::GET,
            "/auth/status",
            &[("Cookie", &cookie)],
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status["user"]["login"], USER);
    assert_eq!(status["user"]["via"], "password");
    let csrf = status["user"]["csrf"].as_str().unwrap().to_string();
    assert_eq!(csrf.len(), 32);

    // Reads need only the cookie
    let response = send(
        &app,
        Method::GET,
        "/settings/sign-in",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    // Writes need the CSRF token too
    let settings = body_json(response).await;
    let put = |headers: Vec<(&'static str, String)>| {
        let app = &app;
        let settings = settings.clone();
        async move {
            let headers: Vec<(&str, &str)> =
                headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
            send(
                app,
                Method::PUT,
                "/settings/sign-in",
                &headers,
                Some(settings),
            )
            .await
        }
    };
    let response = put(vec![("Cookie", cookie.clone())]).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(body_text(response)
        .await
        .contains("CSRF token missing or wrong"));
    let response = put(vec![
        ("Cookie", cookie.clone()),
        ("X-CSRF-Token", "0".repeat(32)),
    ])
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = put(vec![
        ("Cookie", cookie.clone()),
        ("X-CSRF-Token", csrf.clone()),
    ])
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "{}",
        body_text(response).await
    );

    // A forged cookie is no session
    let forged = format!("{}x", cookie);
    let response = send(
        &app,
        Method::GET,
        "/settings/sign-in",
        &[("Cookie", &forged)],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Sign out clears the cookie
    let response = send(
        &app,
        Method::POST,
        "/auth/logout",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let (pair, full) = set_cookie(&response, "putio_sync_session").unwrap();
    assert_eq!(pair, "putio_sync_session=");
    assert!(full.contains("Max-Age=0"));
}

#[tokio::test]
async fn the_password_form_refuses_and_counts() {
    let app = test_app_with(|_| {}).await;
    let response = send(
        &app,
        Method::POST,
        "/auth/password",
        &[],
        Some(json!({"username": USER, "password": PASSWORD})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let app = test_app_with(with_password).await;
    for body in [
        json!({"username": USER, "password": "wrong"}),
        json!({"username": "root", "password": PASSWORD}),
    ] {
        let response = send(&app, Method::POST, "/auth/password", &[], Some(body)).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(set_cookie(&response, "putio_sync_session").is_none());
    }
    for _ in 0..3 {
        send(
            &app,
            Method::POST,
            "/auth/password",
            &[],
            Some(json!({"username": USER, "password": "x"})),
        )
        .await;
    }
    let response = send(
        &app,
        Method::POST,
        "/auth/password",
        &[],
        Some(json!({"username": USER, "password": PASSWORD})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn behind_https_the_cookie_is_secure() {
    let app = test_app_with(with_password).await;
    let response = send(
        &app,
        Method::POST,
        "/auth/password",
        &[("X-Forwarded-Proto", "https")],
        Some(json!({"username": USER, "password": PASSWORD})),
    )
    .await;
    let (_, full) = set_cookie(&response, "putio_sync_session").unwrap();
    assert!(full.ends_with("; Secure"), "{full}");
}

#[tokio::test]
async fn a_new_password_ends_password_sessions() {
    let app = test_app_with(with_password).await;
    let cookie = password_session(&app).await;
    let csrf = csrf_for(&app, &cookie).await;
    let response = send(
        &app,
        Method::PUT,
        "/settings/sign-in",
        &[("Cookie", &cookie), ("X-CSRF-Token", &csrf)],
        Some(json!({"password": {"username": USER, "new_password": "another password"}})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let message = body_json(response).await["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(message.contains("password"), "{message}");
    assert!(
        message.contains("Password sessions signed in before this end."),
        "{message}"
    );
    let hash = app
        .state
        .config
        .read()
        .await
        .sign_in
        .password
        .password_hash
        .clone();
    assert!(hash.starts_with("$argon2id$"));
    assert_ne!(hash, password_hash());

    let response = send(
        &app,
        Method::GET,
        "/settings/sign-in",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    // The new one works for Basic, the old one doesn't
    let response = send(
        &app,
        Method::GET,
        "/settings/sign-in",
        &[("Authorization", &basic(USER, "another password"))],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = send(
        &app,
        Method::GET,
        "/settings/sign-in",
        &[("Authorization", &basic(USER, PASSWORD))],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Too short is refused; clear removes it
    let response = app
        .req(
            Method::PUT,
            "/settings/sign-in",
            Some(API_TOKEN),
            Some(json!({"password": {"username": USER, "new_password": "short"}})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = app
        .req(
            Method::PUT,
            "/settings/sign-in",
            Some(API_TOKEN),
            Some(json!({"password": {"username": USER, "clear": true}})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!app.state.config.read().await.sign_in.password.is_set());
}

#[tokio::test]
async fn sign_out_everywhere_ends_every_session() {
    let app = test_app_with(with_password).await;
    let first = password_session(&app).await;
    let second = password_session(&app).await;
    let key_file = app._dir.path().join("session.key");
    assert!(key_file.exists());
    let before = std::fs::read_to_string(&key_file).unwrap();
    let response = app
        .req(
            Method::POST,
            "/settings/sign-in/sign-out-everywhere",
            Some(API_TOKEN),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_ne!(std::fs::read_to_string(&key_file).unwrap(), before);
    for cookie in [first, second] {
        let response = send(
            &app,
            Method::GET,
            "/settings/sign-in",
            &[("Cookie", &cookie)],
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn the_token_works_as_before() {
    let app = test_app_with(|_| {}).await;
    let status = body_json(send(&app, Method::GET, "/auth/status", &[], None).await).await;
    assert_eq!(status["token_configured"], true);
    assert!(status.get("sign_in").is_none(), "{status}");
    assert_eq!(app.get("/settings/sign-in").await.status(), StatusCode::OK);
    // Token writes need no CSRF token
    let response = app
        .req(
            Method::PUT,
            "/settings/sign-in",
            Some(API_TOKEN),
            Some(json!({})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    // Without a password, Basic is just a wrong token
    let response = send(
        &app,
        Method::GET,
        "/settings/sign-in",
        &[("Authorization", &basic(USER, PASSWORD))],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    // A token request with a stale cookie still works
    let response = send(
        &app,
        Method::PUT,
        "/settings/sign-in",
        &[
            ("Authorization", &format!("Bearer {API_TOKEN}")),
            ("Cookie", "putio_sync_session=a.b"),
        ],
        Some(json!({})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn without_a_token_the_password_is_enough() {
    let app = test_app_with(|config| {
        config.api.auth_token = None;
        with_password(config);
    })
    .await;
    let response = send(
        &app,
        Method::GET,
        "/settings/sign-in",
        &[("Authorization", &basic(USER, PASSWORD))],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = send(&app, Method::GET, "/settings/sign-in", &[], None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!body_text(response).await.contains("not configured"));
    // Nothing configured at all still names the setting
    let app = test_app_with(|config| config.api.auth_token = None).await;
    let response = send(&app, Method::GET, "/settings/sign-in", &[], None).await;
    assert!(body_text(response)
        .await
        .contains("API token not configured"));
}

#[tokio::test]
async fn settings_mask_the_client_secrets() {
    let app = test_app_with(|config| {
        with_password(config);
        with_provider(config, Provider::Github, &["octocat"]);
        config.sign_in.public_url = Some("https://sync.example.com/".to_string());
    })
    .await;
    let response = app.get("/settings/sign-in").await;
    let text = body_text(response).await;
    assert!(!text.contains(CLIENT_SECRET), "{text}");
    assert!(!text.contains("$argon2"), "{text}");
    let settings: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(settings["password"], json!({"username": USER, "set": true}));
    let github = &settings["providers"]["github"];
    assert_eq!(github["client_secret"], "********");
    assert_eq!(
        github["redirect_url"],
        "https://sync.example.com/auth/oauth/github/callback"
    );
    assert_eq!(settings["providers"]["google"]["enabled"], false);

    // Saving the masked secret keeps the stored one
    let mut body = settings.clone();
    body["providers"]["github"]["allowed_users"] = json!(["octocat", "@example.com"]);
    let response = app
        .req(
            Method::PUT,
            "/settings/sign-in",
            Some(API_TOKEN),
            Some(body),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let saved = app.state.config.read().await.sign_in.clone();
    assert_eq!(saved.github.client_secret, CLIENT_SECRET);
    assert_eq!(saved.github.allowed_users, ["octocat", "@example.com"]);
    assert!(saved.password.is_set());

    // On, without a secret, is refused
    let mut body = settings.clone();
    body["providers"]["discord"] =
        json!({"enabled": true, "client_id": "id", "client_secret": "", "allowed_users": ["me"]});
    let response = app
        .req(
            Method::PUT,
            "/settings/sign-in",
            Some(API_TOKEN),
            Some(body),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // The export leaves [sign_in] out; the file keeps it
    let export = body_text(app.get("/settings/export").await).await;
    assert!(!export.contains(CLIENT_SECRET));
    assert!(!export.contains("\n[sign_in"), "{export}");
    assert!(!export.contains("sync.example.com"), "{export}");
    let file = std::fs::read_to_string(app._dir.path().join("config.toml")).unwrap();
    assert!(file.contains("[sign_in.github]"), "{file}");
}

#[tokio::test]
async fn without_a_public_url_the_redirect_follows_the_proxy() {
    let app =
        test_app_with(|config| with_provider(config, Provider::Google, &["a@example.com"])).await;
    let response = send(
        &app,
        Method::GET,
        "/settings/sign-in",
        &[
            ("Authorization", &format!("Bearer {API_TOKEN}")),
            ("X-Forwarded-Proto", "https"),
            ("X-Forwarded-Host", "nas.example.net"),
            ("Host", "127.0.0.1:15432"),
        ],
        None,
    )
    .await;
    let settings = body_json(response).await;
    assert_eq!(
        settings["providers"]["google"]["redirect_url"],
        "https://nas.example.net/auth/oauth/google/callback"
    );
}

// OAuth against a mock provider

struct Started {
    state: String,
    binding: String,
    query: HashMap<String, String>,
}

async fn start(app: &TestApp, provider: &str, ret: &str) -> Started {
    let response = send(
        app,
        Method::GET,
        &format!(
            "/auth/oauth/{provider}?return={}",
            ret.replace('#', "%23").replace('/', "%2F")
        ),
        &[("Host", "127.0.0.1:15432")],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FOUND);
    let url = reqwest::Url::parse(&location(&response)).unwrap();
    assert!(
        url.path().ends_with(&format!("/{provider}/authorize")),
        "{url}"
    );
    let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
    let (pair, full) = set_cookie(&response, "putio_sync_oauth").unwrap();
    assert!(
        full.contains("HttpOnly; SameSite=Lax; Path=/auth/oauth; Max-Age=600"),
        "{full}"
    );
    Started {
        state: query["state"].clone(),
        binding: pair,
        query,
    }
}

async fn callback(app: &TestApp, provider: &str, query: &str, cookie: Option<&str>) -> Response {
    let mut headers = vec![("Host", "127.0.0.1:15432")];
    if let Some(cookie) = cookie {
        headers.push(("Cookie", cookie));
    }
    send(
        app,
        Method::GET,
        &format!("/auth/oauth/{provider}/callback?{query}"),
        &headers,
        None,
    )
    .await
}

async fn oauth_app(provider: Provider, allowed: &[&str]) -> (TestApp, mockito::ServerGuard) {
    let server = mockito::Server::new_async().await;
    let app = test_app_with(|config| with_provider(config, provider, allowed)).await;
    let _ = app.state.clone().with_oauth_base(&server.url());
    (app, server)
}

async fn mock_token(server: &mut mockito::ServerGuard, provider: &str) -> mockito::Mock {
    server
        .mock("POST", format!("/{provider}/token").as_str())
        .match_body(mockito::Matcher::AllOf(vec![
            mockito::Matcher::Regex("grant_type=authorization_code".into()),
            mockito::Matcher::Regex("code=the-code".into()),
            mockito::Matcher::Regex("code_verifier=[A-Za-z0-9_-]{43}".into()),
            mockito::Matcher::Regex(format!("client_id={provider}-client")),
            mockito::Matcher::Regex(format!(
                "redirect_uri=http%3A%2F%2F127.0.0.1%3A15432%2Fauth%2Foauth%2F{provider}%2Fcallback"
            )),
        ]))
        .with_header("content-type", "application/json")
        .with_body(r#"{"access_token":"at-1","token_type":"bearer"}"#)
        .create_async()
        .await
}

async fn mock_user(server: &mut mockito::ServerGuard, path: &str, body: Value) -> mockito::Mock {
    server
        .mock("GET", path)
        .match_header("authorization", "Bearer at-1")
        .with_header("content-type", "application/json")
        .with_body(body.to_string())
        .create_async()
        .await
}

/// A signed-in callback: the session cookie, and that it lands on `ret`
async fn signed_in(app: &TestApp, provider: &str, started: &Started, ret: &str) -> String {
    let response = callback(
        app,
        provider,
        &format!("code=the-code&state={}", started.state),
        Some(&started.binding),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(location(&response), format!("../../../{ret}"));
    let (_, flow) = set_cookie(&response, "putio_sync_oauth").unwrap();
    assert!(flow.contains("Max-Age=0"));
    set_cookie(&response, "putio_sync_session")
        .expect("a session")
        .0
}

#[tokio::test]
async fn github_sign_in_with_pkce_and_state() {
    let (app, mut server) = oauth_app(Provider::Github, &["@example.com"]).await;
    let token = mock_token(&mut server, "github").await;
    let user = mock_user(
        &mut server,
        "/github/user",
        json!({"login": "octocat", "name": "The Octocat"}),
    )
    .await;
    let emails = mock_user(
        &mut server,
        "/github/emails",
        json!([
            {"email": "octo@example.com", "verified": true, "primary": false},
            {"email": "spoof@example.com", "verified": false, "primary": true}
        ]),
    )
    .await;

    let started = start(&app, "github", "#/settings/sign-in").await;
    assert_eq!(started.query["response_type"], "code");
    assert_eq!(started.query["client_id"], "github-client");
    assert_eq!(started.query["code_challenge_method"], "S256");
    assert_eq!(started.query["code_challenge"].len(), 43);
    assert_eq!(started.query["scope"], "read:user user:email");
    assert_eq!(
        started.query["redirect_uri"],
        "http://127.0.0.1:15432/auth/oauth/github/callback"
    );
    let cookie = signed_in(&app, "github", &started, "#/settings/sign-in").await;
    token.assert_async().await;
    user.assert_async().await;
    emails.assert_async().await;

    let status = body_json(
        send(
            &app,
            Method::GET,
            "/auth/status",
            &[("Cookie", &cookie)],
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status["user"]["via"], "github");
    // The email that matched is kept, so checking it again asks the same question
    assert_eq!(status["user"]["login"], "octo@example.com");
    assert_eq!(status["user"]["name"], "The Octocat");
    assert_eq!(status["sign_in"]["providers"], json!(["github"]));
    let response = send(
        &app,
        Method::GET,
        "/settings/sign-in",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);

    // The state works once
    let response = callback(
        &app,
        "github",
        &format!("code=the-code&state={}", started.state),
        Some(&started.binding),
    )
    .await;
    assert_eq!(location(&response), "../../../#/login?error=state");

    // Taking the account off the list ends the session
    app.state.config.write().await.sign_in.github.allowed_users = vec!["someone-else".into()];
    let response = send(
        &app,
        Method::GET,
        "/settings/sign-in",
        &[("Cookie", &cookie)],
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn google_needs_a_verified_email() {
    let (app, mut server) = oauth_app(Provider::Google, &["alice@example.com"]).await;
    let _token = mock_token(&mut server, "google").await;
    let user = mock_user(
        &mut server,
        "/google/user",
        json!({"email": "alice@example.com", "email_verified": false, "name": "Alice"}),
    )
    .await;
    let started = start(&app, "google", "#/status").await;
    assert_eq!(started.query["scope"], "openid email profile");
    let response = callback(
        &app,
        "google",
        &format!("code=the-code&state={}", started.state),
        Some(&started.binding),
    )
    .await;
    assert_eq!(location(&response), "../../../#/login?error=not_allowed");
    assert!(set_cookie(&response, "putio_sync_session").is_none());
    user.assert_async().await;

    server.reset();
    let _token = mock_token(&mut server, "google").await;
    let _user = mock_user(
        &mut server,
        "/google/user",
        json!({"email": "Alice@Example.com", "email_verified": true, "name": "Alice"}),
    )
    .await;
    let started = start(&app, "google", "#/status").await;
    let cookie = signed_in(&app, "google", &started, "#/status").await;
    let status = body_json(
        send(
            &app,
            Method::GET,
            "/auth/status",
            &[("Cookie", &cookie)],
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status["user"]["via"], "google");
    assert_eq!(status["user"]["name"], "Alice");
}

#[tokio::test]
async fn discord_sign_in_by_username() {
    let (app, mut server) = oauth_app(Provider::Discord, &["nasfan"]).await;
    let _token = mock_token(&mut server, "discord").await;
    let _user = mock_user(
        &mut server,
        "/discord/user",
        json!({"username": "nasfan", "global_name": "NAS Fan", "email": null, "verified": false}),
    )
    .await;
    let started = start(&app, "discord", "#/status").await;
    assert_eq!(started.query["scope"], "identify email");
    let cookie = signed_in(&app, "discord", &started, "#/status").await;
    let status = body_json(
        send(
            &app,
            Method::GET,
            "/auth/status",
            &[("Cookie", &cookie)],
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status["user"]["login"], "nasfan");
    assert_eq!(status["user"]["name"], "NAS Fan");
}

#[tokio::test]
async fn an_account_not_listed_is_refused_and_counted() {
    let (app, mut server) = oauth_app(Provider::Discord, &["someone"]).await;
    let _token = mock_token(&mut server, "discord").await;
    let _user = mock_user(
        &mut server,
        "/discord/user",
        json!({"username": "stranger", "email": "s@example.com", "verified": true}),
    )
    .await;
    for _ in 0..5 {
        let started = start(&app, "discord", "#/status").await;
        let response = callback(
            &app,
            "discord",
            &format!("code=the-code&state={}", started.state),
            Some(&started.binding),
        )
        .await;
        assert_eq!(location(&response), "../../../#/login?error=not_allowed");
    }
    let response = send(&app, Method::GET, "/auth/oauth/discord", &[], None).await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn bad_callbacks_are_refused() {
    let (app, mut server) = oauth_app(Provider::Github, &["octocat"]).await;
    let token = server
        .mock("POST", "/github/token")
        .expect(0)
        .create_async()
        .await;

    // Another browser: no binding cookie, or someone else's
    let started = start(&app, "github", "#/status").await;
    let response = callback(
        &app,
        "github",
        &format!("code=the-code&state={}", started.state),
        None,
    )
    .await;
    assert_eq!(location(&response), "../../../#/login?error=state");
    let started = start(&app, "github", "#/status").await;
    let response = callback(
        &app,
        "github",
        &format!("code=the-code&state={}", started.state),
        Some("putio_sync_oauth=other"),
    )
    .await;
    assert_eq!(location(&response), "../../../#/login?error=state");
    // An unknown state
    let response = callback(
        &app,
        "github",
        "code=the-code&state=made-up",
        Some(&started.binding),
    )
    .await;
    assert_eq!(location(&response), "../../../#/login?error=state");
    // A state for another provider's callback
    let started = start(&app, "github", "#/status").await;
    let response = callback(
        &app,
        "google",
        &format!("code=the-code&state={}", started.state),
        Some(&started.binding),
    )
    .await;
    assert_eq!(location(&response), "../../../#/login?error=state");
    // The provider said no
    let started = start(&app, "github", "#/status").await;
    let response = callback(
        &app,
        "github",
        &format!("error=access_denied&state={}", started.state),
        Some(&started.binding),
    )
    .await;
    assert_eq!(location(&response), "../../../#/login?error=denied");
    token.assert_async().await;

    // A provider that's off, or unknown
    for path in ["/auth/oauth/google", "/auth/oauth/myspace"] {
        assert_eq!(
            send(&app, Method::GET, path, &[], None).await.status(),
            StatusCode::NOT_FOUND
        );
    }
    // An unsafe return lands on the status page
    let started = start(&app, "github", "https://evil.example/").await;
    assert!(started.query.contains_key("state"));
}

#[tokio::test]
async fn a_failed_exchange_is_reported() {
    let (app, mut server) = oauth_app(Provider::Github, &["octocat"]).await;
    // GitHub answers 200 with an error
    let _token = server
        .mock("POST", "/github/token")
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":"bad_verification_code","error_description":"The code passed is incorrect or expired."}"#)
        .create_async()
        .await;
    let started = start(&app, "github", "#/status").await;
    let response = callback(
        &app,
        "github",
        &format!("code=the-code&state={}", started.state),
        Some(&started.binding),
    )
    .await;
    assert_eq!(location(&response), "../../../#/login?error=exchange");
}
