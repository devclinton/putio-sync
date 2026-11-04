//! Lua plugin routes (feature 028): the plugin list, test events and reload

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, body_text, test_app_with, TestApp};
use putio_sync_core::plugins::PluginSettings;
use serde_json::json;
use std::path::Path;

const ECHO: &str = r#"
local plugin = {}
function plugin.on_event(event)
  putio.log.info("got " .. event.type)
  if event.data.plugin == "slow" then
    local t = putio.time.now()
    while putio.time.now() - t < 1 do end
  end
end
return plugin
"#;

fn write(dir: &Path, name: &str, script: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("plugin.toml"),
        format!("name = \"{name}\"\nversion = \"1.2.0\"\nevents = [\"download.finished\"]\n"),
    )
    .unwrap();
    std::fs::write(path.join("main.lua"), script).unwrap();
}

fn enabled(name: &str) -> PluginSettings {
    PluginSettings {
        name: name.to_string(),
        enabled: true,
        timeout_secs: 5,
        ..PluginSettings::default()
    }
}

async fn app(dir: &Path, plugins: Vec<PluginSettings>) -> TestApp {
    let dir = dir.to_path_buf();
    let app = test_app_with(move |config| {
        config.plugins_dir = Some(dir);
        config.plugins = plugins;
    })
    .await;
    app.state.plugins.start();
    app
}

#[tokio::test]
async fn needs_a_token() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path(), Vec::new()).await;
    for (method, path) in [
        (Method::GET, "/plugins"),
        (Method::POST, "/plugins/test"),
        (Method::POST, "/plugins/reload"),
    ] {
        let response = app.req(method, path, None, None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn test_event_runs_the_plugin_and_shows_in_the_list() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "echo", ECHO);
    let app = app(dir.path(), vec![enabled("echo")]).await;

    let response = app
        .post_json("/plugins/test", json!({ "name": "echo" }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let run = body_json(response).await;
    assert_eq!(run["result"], "success");
    assert_eq!(run["event_type"], "plugin.test");
    assert_eq!(run["logs"][0]["message"], "got plugin.test");

    let list = body_json(app.get("/plugins").await).await;
    assert_eq!(list["dir"], dir.path().display().to_string());
    let plugins = list["plugins"].as_array().unwrap();
    let echo = plugins.iter().find(|p| p["name"] == "echo").unwrap();
    assert_eq!(echo["version"], "1.2.0");
    assert_eq!(echo["source"], "folder");
    assert_eq!(echo["state"], "enabled");
    assert_eq!(echo["events"], json!(["download.finished"]));
    assert_eq!(
        echo["runs"].as_array().unwrap().last().unwrap()["result"],
        "success"
    );
    let ntfy = plugins.iter().find(|p| p["name"] == "ntfy").unwrap();
    assert_eq!(ntfy["source"], "bundled");
    assert_eq!(ntfy["state"], "disabled");
    assert_eq!(ntfy["hosts_needed"], json!(["ntfy.sh"]));
}

#[tokio::test]
async fn unknown_is_404_broken_is_422_and_a_second_test_is_409() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "slow", ECHO);
    let broken = dir.path().join("broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("plugin.toml"), "name = \"broken\"\n").unwrap();
    let app = app(dir.path(), vec![enabled("slow"), enabled("broken")]).await;

    let response = app
        .post_json("/plugins/test", json!({ "name": "nope" }))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let response = app
        .post_json("/plugins/test", json!({ "name": "broken" }))
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body_text(response).await.contains("plugin.toml"));

    let response = app.post_json("/plugins/test", json!({})).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let first = app.post_json("/plugins/test", json!({ "name": "slow" }));
    let second = async {
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        app.post_json("/plugins/test", json!({ "name": "slow" }))
            .await
    };
    let (first, second) = tokio::join!(first, second);
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn reload_finds_a_new_plugin() {
    let dir = tempfile::tempdir().unwrap();
    let app = app(dir.path(), vec![enabled("late")]).await;
    let list = body_json(app.get("/plugins").await).await;
    assert!(!list["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == "late"));

    write(dir.path(), "late", ECHO);
    let response = app
        .req(Method::POST, "/plugins/reload", Some(&app.token), None)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["message"], "Reloaded 5 plugins");

    let list = body_json(app.get("/plugins").await).await;
    let late = list["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "late")
        .cloned()
        .unwrap();
    assert_eq!(late["state"], "enabled");
}
