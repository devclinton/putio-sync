//! Organising on Put.io (feature 023): `GET`/`PUT /settings/organise`, its preview and the
//! `/organise` moves routes.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, TestApp};
use putio_sync_api::router;
use putio_sync_db::NewMove;
use putio_sync_download::Organiser;
use serde_json::{json, Value};
use std::sync::Arc;

/// An app whose state holds an organiser, as the daemon's does
async fn app() -> TestApp {
    let mut app = test_app().await;
    let state = app.state.clone();
    let organiser = Arc::new(Organiser::new(
        state.client.clone(),
        app.db.clone().unwrap(),
        &state.download_manager,
        state.config.read().await.organise.clone(),
    ));
    app.state = state.with_organiser(organiser);
    app.router = router(app.state.clone());
    app
}

async fn call(
    app: &TestApp,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let r = app.req(method, path, Some(&app.token), body).await;
    let status = r.status();
    (status, body_json(r).await)
}

#[tokio::test]
async fn auth_is_required() {
    let app = app().await;
    for (method, path) in [
        (Method::GET, "/settings/organise"),
        (Method::PUT, "/settings/organise"),
        (Method::POST, "/settings/organise/preview"),
        (Method::GET, "/organise"),
        (Method::POST, "/organise/apply"),
        (Method::POST, "/organise/undo"),
        (Method::POST, "/organise/dismiss"),
        (Method::POST, "/organise/scan"),
    ] {
        let r = app.req(method, path, None, Some(json!({}))).await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn settings_round_trip_and_reach_the_organiser() {
    let app = app().await;
    let (status, body) = call(&app, Method::GET, "/settings/organise", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({"tv": false, "tv_folder": "TV Shows", "apply": "preview", "use_plugins": true})
    );

    let wanted = json!({"tv": true, "tv_folder": "Series", "apply": "auto", "use_plugins": false});
    let (status, preview) = call(
        &app,
        Method::POST,
        "/settings/organise/preview",
        Some(wanted.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["valid"], true);

    let (status, saved) = call(
        &app,
        Method::PUT,
        "/settings/organise",
        Some(wanted.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert!(saved["message"].as_str().unwrap().contains("next sync"));
    let (_, body) = call(&app, Method::GET, "/settings/organise", None).await;
    assert_eq!(body, wanted);
    let live = app.state.organiser.as_ref().unwrap().config();
    assert!(live.tv);
    assert_eq!(live.tv_folder, "Series");

    // A folder outside the Put.io root is refused and nothing changes
    let (status, _) = call(
        &app,
        Method::PUT,
        "/settings/organise",
        Some(json!({"tv": true, "tv_folder": "../up"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        app.state.organiser.as_ref().unwrap().config().tv_folder,
        "Series"
    );
}

#[tokio::test]
async fn moves_are_listed_and_dismissed() {
    let app = app().await;
    app.db()
        .organise_moves()
        .plan(&NewMove {
            file_id: 5,
            kind: "tv".into(),
            from_folder: "Show.S01E01".into(),
            from_name: "Show.S01E01.mkv".into(),
            to_folder: "TV Shows/Show/Season 01".into(),
            to_name: "Show - S01E01.mkv".into(),
            by_plugin: false,
        })
        .await
        .unwrap();

    let (status, body) = call(&app, Method::GET, "/organise", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["available"], true);
    assert_eq!(body["moves"][0]["status"], "planned");
    assert_eq!(body["moves"][0]["to_name"], "Show - S01E01.mkv");

    let (status, out) = call(
        &app,
        Method::POST,
        "/organise/dismiss",
        Some(json!({"file_ids": [5, 7]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(out["results"][0]["status"], "dismissed");
    assert_eq!(out["results"][1]["status"], "missing");
    let (_, body) = call(&app, Method::GET, "/organise", None).await;
    assert_eq!(body["moves"], json!([]));

    // Empty batches are refused
    let (status, _) = call(
        &app,
        Method::POST,
        "/organise/apply",
        Some(json!({"file_ids": []})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = call(&app, Method::POST, "/organise/scan", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["planned"], 0);
}

#[tokio::test]
async fn without_the_daemon_the_moves_are_unavailable() {
    let app = test_app().await;
    let (status, body) = call(&app, Method::GET, "/organise", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["available"], false);
    let (status, _) = call(
        &app,
        Method::POST,
        "/organise/apply",
        Some(json!({"file_ids": [1]})),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let (status, _) = call(&app, Method::POST, "/organise/scan", None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    // The settings still save
    let (status, saved) = call(
        &app,
        Method::PUT,
        "/settings/organise",
        Some(json!({"tv": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(saved["message"].as_str().unwrap().contains("daemon"));
}
