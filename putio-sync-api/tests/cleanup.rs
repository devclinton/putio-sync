//! Empty-folder cleanup endpoint (feature 005, US4): auth, the required `dry_run`, a dry
//! run that deletes nothing, a real run, and 409 while a run is going. Put.io is mocked.
//! Also the `/settings/cleanup` group (US3): defaults, a save that keeps the file's
//! comments, validation, and the save messages per engine.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_daemon_app, TestApp};
use serde_json::{json, Value};

const PATH: &str = "/putio/cleanup/empty-folders";

fn folder(id: i64, name: &str) -> Value {
    json!({
        "id": id, "name": name, "size": 0, "file_type": "FOLDER", "parent_id": 0,
        "created_at": "2020-01-01T00:00:00", "is_folder": true, "is_shared": false
    })
}

fn listing(files: Vec<Value>) -> Value {
    json!({"status": "OK", "files": files, "parent": folder(0, "root")})
}

/// The root holds one old, empty folder `Old` (id 10)
async fn one_empty_folder(app: &mut TestApp) -> Vec<mockito::Mock> {
    vec![
        app.putio
            .mock("GET", "/transfers/list")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"OK","transfers":[]}"#)
            .create_async()
            .await,
        app.mock_files_list(0, listing(vec![folder(10, "Old")]))
            .await,
        app.mock_files_list(10, listing(vec![])).await,
    ]
}

async fn deletes(app: &mut TestApp, expect: usize) -> mockito::Mock {
    app.putio
        .mock("POST", "/files/delete")
        .match_body(mockito::Matcher::Json(json!({"file_ids": "10"})))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(expect)
        .create_async()
        .await
}

#[tokio::test]
async fn requires_a_token() {
    let mut app = test_app().await;
    let delete = deletes(&mut app, 0).await;

    let response = app
        .req(Method::POST, PATH, None, Some(json!({"dry_run": false})))
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = app
        .req(
            Method::POST,
            PATH,
            Some("wrong-token"),
            Some(json!({"dry_run": false})),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    delete.assert_async().await;
}

#[tokio::test]
async fn dry_run_is_required() {
    let mut app = test_app().await;
    let delete = deletes(&mut app, 0).await;

    for body in [json!({}), json!({"dry_run": "no"})] {
        let response = app.post_json(PATH, body.clone()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        assert!(body_json(response).await["error"].is_string());
    }
    delete.assert_async().await;
}

#[tokio::test]
async fn dry_run_reports_and_deletes_nothing() {
    let mut app = test_app().await;
    let _tree = one_empty_folder(&mut app).await;
    let delete = deletes(&mut app, 0).await;

    let response = app.post_json(PATH, json!({"dry_run": true})).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;

    delete.assert_async().await;
    assert_eq!(body["dry_run"], true);
    assert_eq!(body["removed_count"], 1);
    assert_eq!(body["removed"][0]["id"], 10);
    assert_eq!(body["removed"][0]["path"], "/Old");
    assert_eq!(body["scanned"], 2);
    assert!(body["duration_ms"].is_u64());
    assert_eq!(body["failures"], json!([]));
}

#[tokio::test]
async fn real_run_deletes() {
    let mut app = test_app().await;
    let _tree = one_empty_folder(&mut app).await;
    let delete = deletes(&mut app, 1).await;

    let response = app.post_json(PATH, json!({"dry_run": false})).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;

    delete.assert_async().await;
    assert_eq!(body["dry_run"], false);
    assert_eq!(body["removed_count"], 1);
}

#[tokio::test]
async fn second_run_while_one_is_going_is_a_conflict() {
    // Nothing is mocked: the first run keeps retrying the transfer list (5xx) while the
    // second request comes in
    let app = test_app().await;
    let cleanup = app.state.cleanup.clone();
    let first = tokio::spawn(async move {
        let _ = cleanup
            .run(putio_sync_core::cleanup::CleanupTrigger::Manual, true)
            .await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let response = app.post_json(PATH, json!({"dry_run": true})).await;
    first.abort();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(response).await["error"],
        "Empty-folder cleanup is already running"
    );
}

// ---- /settings/cleanup ----------------------------------------------------------------

const SETTINGS: &str = "/settings/cleanup";

async fn put_settings(app: &TestApp, body: Value) -> (StatusCode, Value) {
    let response = app
        .req(Method::PUT, SETTINGS, Some(&app.token), Some(body))
        .await;
    let status = response.status();
    (status, body_json(response).await)
}

fn config_file(app: &TestApp) -> std::path::PathBuf {
    app.state.config_path.as_ref().unwrap().to_path_buf()
}

#[tokio::test]
async fn settings_default() {
    let app = test_app().await;
    let response = app.get(SETTINGS).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await,
        json!({"enabled": false, "run_at": "03:00", "min_age_hours": 24, "nested": true, "keep_patterns": []})
    );
}

#[tokio::test]
async fn settings_save_keeps_comments_and_reach_the_service() {
    let app = test_daemon_app(false).await;
    let path = config_file(&app);
    let edited = format!(
        "# putio-sync on the NAS\n{}",
        std::fs::read_to_string(&path).unwrap()
    );
    std::fs::write(&path, &edited).unwrap();

    let body = json!({
        "enabled": true, "run_at": "04:15", "min_age_hours": 48, "nested": false,
        "keep_patterns": ["Season*"]
    });
    let (status, response) = put_settings(&app, body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let message = response["message"].as_str().unwrap();
    assert!(message.contains("Applies from the next run"), "{message}");
    assert!(!message.contains("restart"), "{message}");

    assert_eq!(body_json(app.get(SETTINGS).await).await, body);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# putio-sync on the NAS\n"), "{text}");
    assert!(text.contains("[cleanup]"), "{text}");
    assert!(text.contains("run_at = \"04:15\""), "{text}");
    let service = app.state.cleanup.config();
    assert!(service.enabled);
    assert_eq!(service.run_at, "04:15");
    assert_eq!(service.min_age_hours, 48);
    assert!(!service.nested);
    assert!(text.contains("nested = false"), "{text}");
    assert_eq!(service.keep_patterns, vec!["Season*"]);
}

/// Feature 021: a saved keep pattern keeps the folder, and the preview says why
#[tokio::test]
async fn keep_patterns_keep_folders_with_the_reason() {
    let mut app = test_app().await;
    let (status, response) = put_settings(
        &app,
        json!({"enabled": false, "run_at": "03:00", "min_age_hours": 24, "keep_patterns": ["Old", " "]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(app.state.cleanup.config().keep_patterns, vec!["Old"]);
    let _tree = one_empty_folder(&mut app).await;
    let delete = deletes(&mut app, 0).await;

    let body = body_json(app.post_json(PATH, json!({"dry_run": true})).await).await;

    delete.assert_async().await;
    assert_eq!(body["removed_count"], 0);
    assert_eq!(body["kept_by_user"], 1);
    assert_eq!(
        body["kept"],
        json!([{"id": 10, "path": "/Old", "reason": "matches keep pattern \"Old\""}])
    );
}

/// A body from before `nested` existed keeps the default (on)
#[tokio::test]
async fn settings_without_nested_default_to_on() {
    let app = test_app().await;
    let (status, response) = put_settings(
        &app,
        json!({"enabled": false, "run_at": "03:00", "min_age_hours": 12}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(app.state.cleanup.config().nested);
}

#[tokio::test]
async fn settings_invalid_change_nothing() {
    let app = test_app().await;
    let path = config_file(&app);
    let before = std::fs::read_to_string(&path).unwrap();

    for body in [
        json!({"enabled": true, "run_at": "25:00", "min_age_hours": 24}),
        json!({"enabled": true, "run_at": "3:00", "min_age_hours": 24}),
        json!({"enabled": true, "run_at": "03:00", "min_age_hours": 8761}),
        json!({"enabled": true, "run_at": "03:00"}),
        json!({"enabled": true, "run_at": "03:00", "min_age_hours": 24, "keep_patterns": ["\"Old"]}),
    ] {
        let (status, response) = put_settings(&app, body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {response}");
        assert!(response["error"].as_str().is_some_and(|e| !e.is_empty()));
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    assert!(!app.state.cleanup.config().enabled);
}

#[tokio::test]
async fn settings_under_server_say_the_daily_run_needs_daemon() {
    let app = test_app().await;
    let (status, response) = put_settings(
        &app,
        json!({"enabled": true, "run_at": "03:00", "min_age_hours": 24}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let message = response["message"].as_str().unwrap();
    assert!(message.contains("only under `daemon`"), "{message}");
}

#[tokio::test]
/// The handler hands the new parent_folder_id to the cleanup service (`configure`, unit
/// tested in core); here the save itself still succeeds
async fn watcher_save_updates_protected_folders() {
    let app = test_app().await;
    let mut body = body_json(app.get("/settings/watcher").await).await;
    body["parent_folder_id"] = json!(77);
    let response = app
        .req(
            Method::PUT,
            "/settings/watcher",
            Some(&app.token),
            Some(body),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        app.state.config.read().await.watch.parent_folder_id,
        Some(77)
    );
}
