//! A config published to the hub reaches the running services without a restart: a hand
//! edit of the file (Source::File) through the subscriber, a settings save (Source::Api)
//! through its own route push, with the hub told in both cases.

mod common;

use axum::http::{Method, StatusCode};
use common::{test_daemon_app, TestApp};
use putio_sync_api::live::spawn_config_subscriber;
use putio_sync_core::config_hub::Source;
use serde_json::json;
use std::time::Duration;

async fn wait_until<F: std::future::Future<Output = bool>>(mut done: impl FnMut() -> F) -> bool {
    for _ in 0..100 {
        if done().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    done().await
}

async fn workers(app: &TestApp) -> usize {
    app.state.download_manager.get_config().await.workers
}

#[tokio::test]
async fn a_hand_edit_reaches_the_manager_the_watcher_and_the_config() {
    let app = test_daemon_app(true).await;
    let subscriber = spawn_config_subscriber(app.state.clone());
    let before = workers(&app).await;
    let folder = tempfile::tempdir().unwrap();

    let mut edited = (*app.state.hub.current()).clone();
    edited.download.workers = before + 3;
    edited.watch.paths = vec![folder.path().to_path_buf()];
    edited.watch.refresh_cycle = 17;
    edited.watch.archive_directory = Some(folder.path().join("done"));
    app.state.hub.publish(edited, Source::File);

    assert!(wait_until(|| async { workers(&app).await == before + 3 }).await);
    let watcher = app.state.file_watcher.as_ref().unwrap().get_config();
    assert_eq!(watcher.watch_dirs, vec![folder.path().to_path_buf()]);
    assert_eq!(watcher.interval, Duration::from_secs(17));
    assert_eq!(watcher.archive_dir, Some(folder.path().join("done")));
    let config = app.state.config.read().await;
    assert_eq!(config.download.workers, before + 3);
    assert_eq!(config.watch.refresh_cycle, 17);
    subscriber.abort();
}

#[tokio::test]
async fn a_settings_save_is_published_and_not_applied_twice() {
    let app = test_daemon_app(true).await;
    let subscriber = spawn_config_subscriber(app.state.clone());
    let generation = app.state.hub.snapshot().generation;
    let before = workers(&app).await;

    let mut body = common::body_json(app.get("/settings/download").await).await;
    body["workers"] = json!(before + 2);
    let response = app
        .req(
            Method::PUT,
            "/settings/download",
            Some(&app.token),
            Some(body),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let snapshot = app.state.hub.snapshot();
    assert_eq!(snapshot.generation, generation + 1);
    assert_eq!(snapshot.source, Source::Api);
    assert_eq!(snapshot.config.download.workers, before + 2);
    // The route pushed it itself
    assert_eq!(workers(&app).await, before + 2);
    subscriber.abort();
}

#[tokio::test]
async fn an_api_snapshot_is_left_to_its_route() {
    let app = test_daemon_app(true).await;
    let subscriber = spawn_config_subscriber(app.state.clone());
    let before = workers(&app).await;

    let mut config = (*app.state.hub.current()).clone();
    config.download.workers = before + 5;
    app.state.hub.publish(config, Source::Api);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        workers(&app).await,
        before,
        "the subscriber skips Source::Api"
    );
    subscriber.abort();
}

#[tokio::test]
async fn the_watcher_start_and_stop_endpoints_publish_the_flag() {
    let app = test_daemon_app(true).await;
    let was_on = app.state.hub.current().watch.enabled;
    let path = if was_on {
        "/local/watcher/stop"
    } else {
        "/local/watcher/start"
    };
    let response = app.post_json(path, json!({})).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(app.state.hub.current().watch.enabled, !was_on);
    assert_eq!(app.state.hub.snapshot().source, Source::Api);
}

#[tokio::test]
async fn an_import_applies_and_publishes() {
    let app = test_daemon_app(true).await;
    let exported = common::body_text(app.get("/settings/export").await).await;
    let before = workers(&app).await;
    let generation = app.state.hub.snapshot().generation;
    let mut edited = (*app.state.hub.current()).clone();
    edited.download.workers = before + 2;
    // Put the edit on disk and in memory through the settings route
    let mut body = common::body_json(app.get("/settings/download").await).await;
    body["workers"] = json!(before + 2);
    app.req(
        Method::PUT,
        "/settings/download",
        Some(&app.token),
        Some(body),
    )
    .await;
    assert_eq!(workers(&app).await, before + 2);

    let response = app
        .post_json("/settings/import", json!({"toml": exported, "apply": true}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(workers(&app).await, before, "the import applied");
    assert_eq!(app.state.config.read().await.download.workers, before);
    assert_eq!(app.state.hub.current().download.workers, before);
    assert_eq!(app.state.hub.snapshot().generation, generation + 2);
}
