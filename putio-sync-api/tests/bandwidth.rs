//! Bandwidth routes (feature 032): the limit in force, a timed pause or slow-down from
//! the API, the Status section and the workers total.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app_with, test_app_without_db, TestApp};
use serde_json::{json, Value};

async fn call(
    app: &TestApp,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let response = app.req(method, path, Some(&app.token), body).await;
    let status = response.status();
    (status, body_json(response).await)
}

async fn limited() -> TestApp {
    test_app_with(|c| {
        c.download.bandwidth_limit_bytes_per_sec = Some(5_000_000);
        c.download.slow_limit_bytes_per_sec = 1_000_000;
    })
    .await
}

#[tokio::test]
async fn get_shows_the_default_limit() {
    let app = limited().await;
    let (status, body) = call(&app, Method::GET, "/bandwidth", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["limit"], 5_000_000);
    assert_eq!(body["bytes_per_sec"], 5_000_000);
    assert_eq!(body["source"], "default");
    assert_eq!(body["entry"], Value::Null);
    assert_eq!(body["override"], Value::Null);
    assert_eq!(body["next_change"], Value::Null);
    assert_eq!(body["slow_limit_bytes_per_sec"], 1_000_000);
    assert!(body["local_time"].is_string(), "{body}");
    assert!(
        body["utc_offset"].as_str().is_some_and(|o| o.len() == 6),
        "{body}"
    );
}

#[tokio::test]
async fn pause_slow_and_resume() {
    let app = limited().await;

    // Pause for the default hour
    let (status, body) = call(
        &app,
        Method::PUT,
        "/bandwidth/override",
        Some(json!({"mode": "pause"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (&body["limit"], &body["source"]),
        (&json!("paused"), &json!("pause"))
    );
    assert_eq!(body["bytes_per_sec"], 0);
    assert_eq!(body["override"]["mode"], "pause");
    // The pause ends in about an hour and the default comes back
    assert_eq!(body["next_change"]["limit"], 5_000_000);
    assert_eq!(body["next_change"]["source"], "default");
    let until =
        chrono::DateTime::parse_from_rfc3339(body["override"]["until"].as_str().unwrap()).unwrap();
    let left = until
        .signed_duration_since(chrono::Utc::now())
        .num_minutes();
    assert!((58..=60).contains(&left), "{left} minutes left");
    // Stored, so a restart keeps it
    assert!(app
        .db()
        .flags()
        .get_text(putio_sync_db::pipeline::BANDWIDTH_OVERRIDE)
        .await
        .unwrap()
        .is_some());

    // The Status page and the workers total show it
    let (_, status_body) = call(&app, Method::GET, "/status", None).await;
    assert_eq!(status_body["bandwidth"]["source"], "pause", "{status_body}");
    let (_, workers) = call(&app, Method::GET, "/local/workers", None).await;
    assert_eq!(workers["total_bandwidth_limit_bps"], 0, "{workers}");

    // Slow replaces it
    let (status, body) = call(
        &app,
        Method::PUT,
        "/bandwidth/override",
        Some(json!({"mode": "slow", "minutes": 30})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (&body["limit"], &body["source"]),
        (&json!(1_000_000), &json!("slow"))
    );

    // Resume schedule: back to the default, and a second DELETE is fine too
    for _ in 0..2 {
        let (status, body) = call(&app, Method::DELETE, "/bandwidth/override", None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            (&body["limit"], &body["source"]),
            (&json!(5_000_000), &json!("default"))
        );
        assert_eq!(body["override"], Value::Null);
    }
    let (_, workers) = call(&app, Method::GET, "/local/workers", None).await;
    assert_eq!(workers["total_bandwidth_limit_bps"], 5_000_000, "{workers}");
}

#[tokio::test]
async fn bad_overrides_are_400() {
    let app = limited().await;
    for body in [
        json!({"mode": "pause", "minutes": 0}),
        json!({"mode": "pause", "minutes": 1441}),
        json!({"mode": "pause", "minutes": -5}),
        json!({"mode": "fast"}),
        json!({}),
    ] {
        let (status, response) =
            call(&app, Method::PUT, "/bandwidth/override", Some(body.clone())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {response}");
        assert!(
            response["error"].as_str().is_some_and(|e| !e.is_empty()),
            "{response}"
        );
    }
    let (_, response) = call(
        &app,
        Method::PUT,
        "/bandwidth/override",
        Some(json!({"mode": "pause", "minutes": 0})),
    )
    .await;
    assert!(
        response["error"].as_str().unwrap().contains("1 to 1440"),
        "{response}"
    );
    let (_, body) = call(&app, Method::GET, "/bandwidth", None).await;
    assert_eq!(body["source"], "default");
}

#[tokio::test]
async fn needs_the_token_and_works_without_a_database() {
    let app = limited().await;
    let response = app.req(Method::GET, "/bandwidth", None, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let app = test_app_without_db().await;
    let (status, body) = call(
        &app,
        Method::PUT,
        "/bandwidth/override",
        Some(json!({"mode": "pause", "minutes": 5})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["source"], "pause");
}
