//! Settings › Usage profile (feature 056): run the profile again, keep hand-made changes.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, TestApp};
use putio_sync_core::config::Config;
use serde_json::{json, Value};

fn root(names: &[&str]) -> Value {
    let files: Vec<Value> = names
        .iter()
        .enumerate()
        .map(|(i, n)| json!({ "id": i + 1, "name": n, "size": 0, "file_type": "FOLDER", "parent_id": 0, "is_folder": true, "is_shared": false }))
        .collect();
    json!({ "status": "OK", "parent": { "id": 0, "name": "Root", "size": 0, "file_type": "FOLDER", "parent_id": 0, "is_folder": true, "is_shared": false }, "files": files })
}

async fn send(app: &TestApp, method: Method, path: &str, body: Value) -> (StatusCode, Value) {
    let res = app.req(method, path, Some(&app.token), Some(body)).await;
    let status = res.status();
    (status, body_json(res).await)
}

fn saved(app: &TestApp) -> Config {
    let path = app.state.config_path.as_ref().expect("config path");
    Config::load(path.as_ref()).expect("config file loads")
}

#[tokio::test]
async fn preview_and_save_keep_hand_made_changes() {
    let mut app = test_app().await;
    let _list = app.mock_files_list(0, root(&["Movies", "Music"])).await;

    let (status, body) = send(&app, Method::GET, "/settings/profile", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["answers"], Value::Null);
    assert_eq!(body["found"]["music"], json!(["Music"]));

    let first =
        json!({ "answers": { "kinds": ["movies", "music"], "by_hand": true, "language": "en" } });
    let (status, preview) = send(
        &app,
        Method::POST,
        "/settings/profile/preview",
        first.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert!(preview["changes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["key"] == "download.rules[Music].preset"));
    // A preview writes nothing
    assert!(saved(&app).profile.is_none());

    let (status, body) = send(&app, Method::PUT, "/settings/profile", first).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let config = saved(&app);
    assert_eq!(config.profile.as_ref().unwrap().kinds.len(), 2);
    assert!(config
        .download
        .rules
        .iter()
        .any(|r| r.path == "Music" && r.preset.as_deref() == Some("audio")));

    // The music rule is changed by hand, then the profile runs again
    let mut edited = app.state.config.read().await.clone();
    edited
        .download
        .rules
        .iter_mut()
        .find(|r| r.path == "Music")
        .unwrap()
        .preset = Some("everything".to_string());
    *app.state.config.write().await = edited;
    let again = json!({ "answers": { "kinds": ["movies", "music", "tv"], "by_hand": true, "language": "en" } });
    let (status, preview) = send(
        &app,
        Method::POST,
        "/settings/profile/preview",
        again.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert_eq!(
        preview["kept"][0]["key"],
        json!("download.rules[Music].preset")
    );
    assert_eq!(preview["kept"][0]["current"], json!("everything"));
    let (status, body) = send(&app, Method::PUT, "/settings/profile", again).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let config = saved(&app);
    let preset = |p: &str| {
        config
            .download
            .rules
            .iter()
            .find(|r| r.path == p)
            .and_then(|r| r.preset.clone())
    };
    assert_eq!(preset("Music").as_deref(), Some("everything"));
    assert_eq!(preset("TV").as_deref(), Some("video"));
}

#[tokio::test]
async fn no_answers_is_refused() {
    let app = test_app().await;
    let (status, _) = send(
        &app,
        Method::PUT,
        "/settings/profile",
        json!({ "answers": { "kinds": [] } }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
