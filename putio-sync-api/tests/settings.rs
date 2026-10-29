//! Settings routes round trip (feature 004, US6): GET, PUT one changed field, GET and the
//! config file show it, and invalid input is a 400 with an `error` that changes nothing.
//! A save never writes an env-only token, and its message says what applies now and what
//! needs a restart, for the engine the API runs on (FR-018).

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_daemon_app, TestApp, API_TOKEN};
use putio_sync_core::config::{AuthConfig, Config, ExistingFile};
use serde_json::{json, Value};

async fn get_ok(app: &TestApp, path: &str) -> Value {
    let response = app.get(path).await;
    assert_eq!(response.status(), StatusCode::OK, "GET {path}");
    body_json(response).await
}

async fn put(app: &TestApp, path: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .req(Method::PUT, path, Some(&app.token), Some(body))
        .await;
    let status = response.status();
    (status, body_json(response).await)
}

/// The config file as `AppState::new` was given it
fn saved(app: &TestApp) -> Config {
    let path = app.state.config_path.as_ref().expect("config path").clone();
    Config::load(path.as_ref()).expect("config file loads")
}

fn config_path(app: &TestApp) -> std::path::PathBuf {
    app.state
        .config_path
        .as_ref()
        .expect("config path")
        .to_path_buf()
}

/// Rewrite the config file as if `api.auth_token` and the Put.io token were set there
/// (or not); memory keeps the token the app was built with, as when it comes from
/// `PUTIO_SYNC_API_TOKEN`
async fn file_tokens(app: &TestApp, api_token: Option<&str>, putio_token: &str) {
    let mut on_disk = app.state.config.read().await.clone();
    on_disk.api.auth_token = api_token.map(str::to_string);
    on_disk.auth = Some(AuthConfig::with_token(putio_token.to_string()));
    on_disk.save(config_path(app)).unwrap();
}

/// PUT the current body with `field` replaced by `value`; expect 200, return the message
async fn message(app: &TestApp, path: &str, field: &str, value: Value) -> String {
    let mut body = get_ok(app, path).await;
    body[field] = value;
    let (status, response) = put(app, path, body).await;
    assert_eq!(status, StatusCode::OK, "PUT {path}: {response}");
    response["message"].as_str().unwrap_or_default().to_string()
}

fn assert_fields(body: &Value, fields: &[&str]) {
    let object = body.as_object().expect("an object");
    for field in fields {
        assert!(object.contains_key(*field), "missing field {field}");
    }
}

/// PUT `body` with `field` replaced by `value`; expect 200 and a message
async fn put_changed(app: &TestApp, path: &str, mut body: Value, field: &str, value: Value) {
    body[field] = value.clone();
    let (status, response) = put(app, path, body).await;
    assert_eq!(status, StatusCode::OK, "PUT {path}: {response}");
    let message = response["message"].as_str().unwrap_or_default();
    assert!(!message.is_empty(), "PUT {path} has no message");
    assert_eq!(
        get_ok(app, path).await[field],
        value,
        "GET {path} after PUT"
    );
}

/// PUT `body` with `field` replaced by `value`; expect 400 with an error and no change
async fn put_invalid(app: &TestApp, path: &str, mut body: Value, field: &str, value: Value) {
    let before = get_ok(app, path).await;
    body[field] = value;
    let (status, response) = put(app, path, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "PUT {path} {field}");
    let error = response["error"].as_str().unwrap_or_default();
    assert!(!error.is_empty(), "PUT {path} {field}: empty error");
    assert_eq!(
        get_ok(app, path).await,
        before,
        "invalid PUT changed {path}"
    );
}

#[tokio::test]
async fn download_settings_round_trip() {
    let app = test_app().await;
    let path = "/settings/download";
    let body = get_ok(&app, path).await;
    assert_fields(
        &body,
        &[
            "workers",
            "target_directory",
            "buffer_size_mb",
            "on_existing",
            "delete_after_download",
            "resume_enabled",
            "chunk_size_mb",
            "progress_style",
            "sort_order",
            "reverse_sort",
            "whitelist_patterns",
            "blacklist_patterns",
            "blocked_extensions",
            "bandwidth_limit_bytes_per_sec",
            "max_retries",
            "retry_initial_delay_secs",
            "delete_empty_folders",
        ],
    );

    put_changed(&app, path, body.clone(), "workers", json!(5)).await;
    assert_eq!(saved(&app).download.workers, 5);

    put_invalid(&app, path, body.clone(), "workers", json!(0)).await;
    put_invalid(&app, path, body.clone(), "buffer_size_mb", json!(0)).await;
    put_invalid(&app, path, body.clone(), "sort_order", json!("sideways")).await;
    // A wrong type is a 400 with a JSON error too, not a bare 422
    put_invalid(&app, path, body.clone(), "workers", json!(-1)).await;
    assert_eq!(saved(&app).download.workers, 5);

    // feature 040: three choices, the old bool still accepted
    assert_eq!(body["on_existing"], "resume");
    assert!(body.get("skip_existing").is_none(), "{body}");
    put_changed(&app, path, body.clone(), "on_existing", json!("keep_both")).await;
    assert_eq!(saved(&app).download.on_existing(), ExistingFile::KeepBoth);
    assert!(!std::fs::read_to_string(config_path(&app))
        .unwrap()
        .contains("skip_existing"));
    put_invalid(&app, path, body.clone(), "on_existing", json!("delete")).await;

    let mut old = get_ok(&app, path).await;
    old.as_object_mut().unwrap().remove("on_existing");
    old["skip_existing"] = json!(false);
    let (status, response) = put(&app, path, old).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(saved(&app).download.on_existing(), ExistingFile::Overwrite);
    assert_eq!(get_ok(&app, path).await["on_existing"], "overwrite");
}

#[tokio::test]
async fn watcher_settings_round_trip() {
    let app = test_app().await;
    let path = "/settings/watcher";
    let body = get_ok(&app, path).await;
    assert_fields(
        &body,
        &[
            "enabled",
            "paths",
            "refresh_cycle",
            "parent_folder_id",
            "archive_directory",
            "delete_after_upload",
        ],
    );

    put_changed(&app, path, body.clone(), "refresh_cycle", json!(42)).await;
    assert_eq!(saved(&app).watch.refresh_cycle, 42);

    put_invalid(&app, path, body.clone(), "refresh_cycle", json!(0)).await;
    put_invalid(&app, path, body, "parent_folder_id", json!(-5)).await;
}

#[tokio::test]
async fn logging_settings_round_trip() {
    let app = test_app().await;
    let path = "/settings/logging";
    let body = get_ok(&app, path).await;
    assert_fields(
        &body,
        &[
            "level",
            "file_enabled",
            "file_path",
            "console_enabled",
            "console_format",
            "module_filters",
        ],
    );

    put_changed(&app, path, body.clone(), "level", json!("debug")).await;
    assert_eq!(saved(&app).logging.level, "debug");

    put_invalid(&app, path, body.clone(), "level", json!("loud")).await;
    put_invalid(&app, path, body.clone(), "console_format", json!("fancy")).await;
    put_invalid(&app, path, body, "module_filters", json!(["a=b=c"])).await;
}

#[tokio::test]
async fn notification_settings_round_trip() {
    let app = test_app().await;
    let path = "/settings/notifications";
    let body = get_ok(&app, path).await;
    assert_fields(&body, &["enabled", "backend", "webhook_url", "email"]);

    put_changed(
        &app,
        path,
        body.clone(),
        "webhook_url",
        json!("http://hooks.local/x"),
    )
    .await;
    assert_eq!(
        saved(&app).notifications.webhook_url.as_deref(),
        Some("http://hooks.local/x")
    );

    put_invalid(&app, path, body, "backend", json!("pigeon")).await;
}

// ---- Tokens are never written from the environment -----------------------------------

#[tokio::test]
async fn env_only_api_token_is_not_written() {
    let app = test_app().await;
    file_tokens(&app, None, "file-putio-token").await;

    let body = get_ok(&app, "/settings/download").await;
    put_changed(&app, "/settings/download", body, "workers", json!(3)).await;

    let text = std::fs::read_to_string(config_path(&app)).unwrap();
    assert!(!text.contains(API_TOKEN), "env-only API token written");
    assert_eq!(saved(&app).api.auth_token, None);
    assert_eq!(saved(&app).download.workers, 3);
    // Memory keeps it, so the operator stays signed in
    assert_eq!(app.get("/settings/download").await.status(), StatusCode::OK);
}

#[tokio::test]
async fn file_tokens_are_kept() {
    let app = test_app().await;
    // The file has its own API token (the env one wins in memory) and a Put.io token
    // from a `login` run after startup
    file_tokens(&app, Some("file-api-token"), "rotated-putio-token").await;

    for path in [
        "/settings/download",
        "/settings/watcher",
        "/settings/logging",
        "/settings/notifications",
    ] {
        let body = get_ok(&app, path).await;
        let (status, response) = put(&app, path, body).await;
        assert_eq!(status, StatusCode::OK, "PUT {path}: {response}");

        let config = saved(&app);
        assert_eq!(
            config.api.auth_token.as_deref(),
            Some("file-api-token"),
            "{path}"
        );
        assert_eq!(
            config.auth.and_then(|a| a.token).as_deref(),
            Some("rotated-putio-token"),
            "{path}"
        );
        let text = std::fs::read_to_string(config_path(&app)).unwrap();
        assert!(!text.contains(API_TOKEN), "{path} wrote the env token");
    }
}

/// A save edits only the changed key: comments, blank lines and keys this version
/// doesn't know survive a settings PUT
#[tokio::test]
async fn save_keeps_comments_and_unknown_keys() {
    let app = test_app().await;
    let path = config_path(&app);
    let original = std::fs::read_to_string(&path).unwrap();
    let workers = format!("\nworkers = {}\n", saved(&app).download.workers);
    assert!(original.contains(&workers), "{original}");
    let edited = format!(
        "# putio-sync on the NAS\n\n{}\n# Added by hand\n[future]\nknob = \"keep me\"\n",
        original.replace(
            "[download]\n",
            "[download]\n# Two is plenty on this box\n\nunknown_setting = 1 # later version\n",
        )
    );
    std::fs::write(&path, &edited).unwrap();

    let body = get_ok(&app, "/settings/download").await;
    put_changed(&app, "/settings/download", body, "workers", json!(6)).await;

    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text, edited.replace(&workers, "\nworkers = 6\n"));
}

// ---- Save messages: what applies now, what needs a restart ---------------------------

#[tokio::test]
async fn unchanged_save_says_so() {
    let app = test_daemon_app(true).await;
    let body = get_ok(&app, "/settings/download").await;
    let (_, response) = put(&app, "/settings/download", body).await;
    let text = response["message"].as_str().unwrap();
    assert!(text.contains("nothing changed"), "{text}");
    assert!(!text.contains("restart"), "{text}");
}

#[tokio::test]
async fn daemon_download_messages() {
    let app = test_daemon_app(true).await;
    let path = "/settings/download";

    let text = message(&app, path, "workers", json!(4)).await;
    assert!(text.contains("next download: workers"), "{text}");
    assert!(!text.contains("restart"), "{text}");

    let text = message(&app, path, "sort_order", json!("ByName")).await;
    assert!(text.contains("Needs a restart: sort_order"), "{text}");
    assert!(!text.contains("Applies"), "{text}");

    let target = app._dir.path().join("elsewhere");
    let text = message(&app, path, "target_directory", json!(target)).await;
    assert!(
        text.contains("New downloads go to the new target_directory"),
        "{text}"
    );
}

#[tokio::test]
async fn server_download_messages() {
    let app = test_app().await;
    let path = "/settings/download";

    let text = message(&app, path, "workers", json!(4)).await;
    assert!(text.contains("API's own download manager"), "{text}");
    assert!(text.contains("workers"), "{text}");
    assert!(text.contains("--download"), "{text}");
    assert!(text.contains("restart"), "{text}");
}

#[tokio::test]
async fn daemon_watcher_messages() {
    let app = test_daemon_app(true).await;
    let path = "/settings/watcher";

    let text = message(&app, path, "parent_folder_id", json!(7)).await;
    assert!(text.contains("next scan: parent_folder_id"), "{text}");
    assert!(!text.contains("restart"), "{text}");

    let current = get_ok(&app, path).await["delete_after_upload"] == json!(true);
    let text = message(&app, path, "delete_after_upload", json!(!current)).await;
    assert!(
        text.contains("Needs a restart: delete_after_upload"),
        "{text}"
    );

    let text = message(&app, path, "refresh_cycle", json!(42)).await;
    assert!(text.contains("--watch-interval"), "{text}");
}

#[tokio::test]
async fn watcher_messages_without_a_live_watcher() {
    // Daemon started with no watch folder, and `server`: everything needs a restart
    for app in [test_daemon_app(false).await, test_app().await] {
        let path = "/settings/watcher";
        let text = message(&app, path, "parent_folder_id", json!(7)).await;
        assert!(text.contains("Needs a restart: parent_folder_id"), "{text}");
        assert!(!text.contains("Applies"), "{text}");
    }
}

#[tokio::test]
async fn logging_messages() {
    let app = test_daemon_app(true).await;
    let path = "/settings/logging";

    let text = message(&app, path, "level", json!("debug")).await;
    if std::env::var_os("RUST_LOG").is_some() {
        assert!(text.contains("ignored while RUST_LOG is set"), "{text}");
    } else {
        assert!(text.contains("within 10 seconds: level"), "{text}");
        assert!(!text.contains("restart"), "{text}");
    }

    let text = message(&app, path, "console_format", json!("Json")).await;
    assert!(text.contains("Needs a restart: console_format"), "{text}");
}

#[tokio::test]
async fn notification_messages() {
    let app = test_daemon_app(true).await;
    let path = "/settings/notifications";
    let text = message(&app, path, "enabled", json!(true)).await;
    assert!(text.contains("Needs a restart: enabled"), "{text}");
}

/// `POST /local/watcher/start|stop` save only the flag: they keep the file's tokens too,
/// and say a restart is needed
#[tokio::test]
async fn watcher_start_stop_keep_tokens_and_say_restart() {
    let app = test_daemon_app(true).await;
    file_tokens(&app, None, "file-putio-token").await;

    for (path, enabled) in [
        ("/local/watcher/start", true),
        ("/local/watcher/stop", false),
    ] {
        let response = app.post_json(path, json!({})).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let body = body_json(response).await;
        let text = body["message"].as_str().unwrap_or_default();
        assert!(text.contains("restart"), "{path}: {text}");

        let config = saved(&app);
        assert_eq!(config.watch.enabled, enabled, "{path}");
        assert_eq!(config.api.auth_token, None, "{path} wrote the env token");
    }
}

// ---- Release check (feature 042, T025) ------------------------------------------------

/// `GET /settings/precheck` gives `[precheck]` and, read-only, what it comes to per
/// category: the video default, or the types of the 009 rule on the category's folder
#[tokio::test]
async fn precheck_settings_and_effective() {
    let app = test_app().await;
    app.state
        .config
        .write()
        .await
        .download
        .rules
        .push(putio_sync_core::rules::FolderRule {
            path: "lidarr".into(),
            preset: Some("audio".into()),
            ..Default::default()
        });
    let path = "/settings/precheck";
    let mut body = get_ok(&app, path).await;
    assert_fields(
        &body,
        &[
            "enabled",
            "dry_run",
            "min_size_mb",
            "allow_archives",
            "magnet_lookup",
            "lookup_timeout_secs",
            "caches",
            "categories",
            "watch",
            "effective",
        ],
    );
    assert_eq!(body["dry_run"], json!(false));

    body["categories"] = json!({"lidarr": {}, "software": {"enabled": false, "min_size_mb": 5}});
    body.as_object_mut().unwrap().remove("effective");
    let (status, response) = put(&app, path, body).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let text = response["message"].as_str().unwrap();
    assert!(text.contains("Needs a restart: categories"), "{text}");
    assert!(saved(&app).precheck.categories["software"].enabled == Some(false));

    let effective = get_ok(&app, path).await["effective"].clone();
    let row = |key: &str| {
        effective
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["key"] == key)
            .unwrap_or_else(|| panic!("no row for {key}: {effective}"))
            .clone()
    };
    let software = row("software");
    assert_eq!(software["enabled"], json!(false));
    assert_eq!(software["min_size_mb"], json!(5));
    assert_eq!(software["wanted_from"], Value::Null);
    assert!(software["wanted"]
        .as_array()
        .unwrap()
        .contains(&json!("mkv")));
    let lidarr = row("lidarr");
    assert_eq!(lidarr["wanted_from"], json!("lidarr"));
    assert!(lidarr["wanted"]
        .as_array()
        .unwrap()
        .contains(&json!("flac")));
}

/// A bad value is a 400 that changes nothing; a save keeps the file's comments
#[tokio::test]
async fn precheck_save_validates_and_keeps_comments() {
    let app = test_app().await;
    let path = "/settings/precheck";
    let file = config_path(&app);
    let original = std::fs::read_to_string(&file).unwrap();
    assert!(original.contains("[precheck]\n"), "{original}");
    let edited = original
        .replace("[precheck]\n", "# Fakes stay off the NAS\n[precheck]\n")
        .replace(
            "\nmin_size_mb = 50\n",
            "\n# Big enough for an episode\nmin_size_mb = 50\n",
        );
    std::fs::write(&file, &edited).unwrap();

    let mut body = get_ok(&app, path).await;
    body["lookup_timeout_secs"] = json!(0);
    let (status, response) = put(&app, path, body.clone()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    assert!(response["error"].as_str().unwrap().contains("1..=30"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), edited);

    body["lookup_timeout_secs"] = json!(10);
    body["dry_run"] = json!(true);
    let (status, response) = put(&app, path, body).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("# Fakes stay off the NAS"), "{text}");
    assert!(
        text.contains("# Big enough for an episode\nmin_size_mb = 50\n"),
        "{text}"
    );
    assert!(saved(&app).precheck.dry_run);
}
