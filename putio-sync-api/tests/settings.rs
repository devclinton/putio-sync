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
            "bandwidth_schedule",
            "slow_limit_bytes_per_sec",
            "verify",
            "verify_max_redownloads",
            "verify_read_limit_mb_per_sec",
            "verify_follow_bandwidth_pause",
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

/// Feature 034: the four verify keys are shown, checked and saved
#[tokio::test]
async fn verify_settings_are_checked_and_saved() {
    let app = test_app().await;
    let path = "/settings/download";
    let body = get_ok(&app, path).await;
    assert_eq!(body["verify"], true);
    assert_eq!(body["verify_max_redownloads"], 2);

    put_changed(&app, path, body.clone(), "verify_max_redownloads", json!(4)).await;
    put_changed(&app, path, body.clone(), "verify", json!(false)).await;
    let saved_config = saved(&app);
    assert!(!saved_config.download.verify);
    put_invalid(&app, path, body.clone(), "verify_max_redownloads", json!(9)).await;
    put_invalid(
        &app,
        path,
        body.clone(),
        "verify_read_limit_mb_per_sec",
        json!(0),
    )
    .await;

    // A body from an older UI without the keys leaves them alone
    let mut old = get_ok(&app, path).await;
    for key in [
        "verify",
        "verify_max_redownloads",
        "verify_read_limit_mb_per_sec",
        "verify_follow_bandwidth_pause",
    ] {
        old.as_object_mut().unwrap().remove(key);
    }
    let (status, response) = put(&app, path, old).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(!saved(&app).download.verify);
}

#[tokio::test]
async fn bandwidth_schedule_is_checked_saved_and_live() {
    let app = test_app().await;
    let path = "/settings/download";
    let body = get_ok(&app, path).await;
    assert_eq!(body["bandwidth_schedule"], json!([]));
    assert_eq!(body["slow_limit_bytes_per_sec"], 1_048_576);

    // A bad entry names the entry and the field
    let mut bad = body.clone();
    bad["bandwidth_schedule"] =
        json!([{"days": "daily", "start": "25:00", "end": "07:00", "limit": "unlimited"}]);
    let (status, response) = put(&app, path, bad).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    let error = response["error"].as_str().unwrap_or_default();
    assert!(error.contains("entry 1: start '25:00'"), "{error}");
    assert!(saved(&app).download.bandwidth_schedule.is_empty());
    put_invalid(
        &app,
        path,
        body.clone(),
        "slow_limit_bytes_per_sec",
        json!(0),
    )
    .await;

    // A good one is saved and reaches the running limiter
    let all_day = json!([{"days": "daily", "start": "00:00", "end": "24:00", "limit": 2000}]);
    put_changed(&app, path, body.clone(), "bandwidth_schedule", all_day).await;
    assert_eq!(saved(&app).download.bandwidth_schedule.len(), 1);
    let current = app.state.download_manager.bandwidth().current();
    assert_eq!(current.limit.bytes_per_sec(), Some(2000));
    assert_eq!(current.entry, Some(1));
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
            "buffer_lines",
        ],
    );

    put_changed(&app, path, body.clone(), "level", json!("debug")).await;
    assert_eq!(saved(&app).logging.level, "debug");
    put_changed(&app, path, body.clone(), "buffer_lines", json!(500)).await;
    assert_eq!(saved(&app).logging.buffer_lines, 500);
    put_invalid(&app, path, body.clone(), "buffer_lines", json!(99)).await;
    put_invalid(&app, path, body.clone(), "buffer_lines", json!(50_001)).await;

    put_invalid(&app, path, body.clone(), "level", json!("loud")).await;
    put_invalid(&app, path, body.clone(), "console_format", json!("fancy")).await;
    put_invalid(&app, path, body, "module_filters", json!(["a=b=c"])).await;
}

#[tokio::test]
async fn storage_settings_round_trip() {
    let app = test_app().await;
    let path = "/settings/storage";
    let body = get_ok(&app, path).await;
    assert_eq!(
        body,
        json!({"warn_free_percent": 10, "target_free_percent": 20, "auto_free": false, "run_at": "02:30"})
    );

    put_changed(&app, path, body.clone(), "target_free_percent", json!(30)).await;
    assert_eq!(saved(&app).storage.target_free_percent, 30);
    // The service sees the save at once
    assert_eq!(app.state.storage.config().target_free_percent, 30);

    let body = get_ok(&app, path).await;
    put_invalid(&app, path, body.clone(), "warn_free_percent", json!(0)).await;
    put_invalid(&app, path, body.clone(), "warn_free_percent", json!(30)).await;
    put_invalid(&app, path, body.clone(), "target_free_percent", json!(95)).await;
    put_invalid(&app, path, body, "run_at", json!("25:00")).await;

    let message = message(&app, path, "auto_free", json!(true)).await;
    assert!(message.contains("only under `daemon`"), "{message}");
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

    put_invalid(&app, path, body.clone(), "backend", json!("pigeon")).await;
    // Email turned on needs a server and addresses
    put_invalid(&app, path, body, "smtp", json!({"enabled": true})).await;
}

/// `[notifications.smtp]` (feature 030): the password is masked, the mask keeps it, and a
/// change applies without a restart
#[tokio::test]
async fn email_settings_keep_the_masked_password() {
    let app = test_daemon_app(true).await;
    let path = "/settings/notifications";
    let mut body = get_ok(&app, path).await;
    assert_eq!(body["smtp"]["enabled"], false);
    assert_eq!(body["smtp"]["events"].as_array().unwrap().len(), 7);
    assert_eq!(body["smtp_status"]["sent"], 0);
    body["smtp"] = json!({
        "enabled": true,
        "host": "smtp.example.com",
        "username": "nas",
        "password": "smtp-pass-1",
        "from": "nas@example.com",
        "to": ["me@example.com"],
    });
    let (status, response) = put(&app, path, body).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let text = response["message"].as_str().unwrap();
    assert!(text.contains("Applies now: smtp"), "{text}");
    assert_eq!(saved(&app).notifications.smtp.password, "smtp-pass-1");
    assert!(app.state.email.status().enabled);

    let mut body = get_ok(&app, path).await;
    assert_eq!(body["smtp"]["password"], "********");
    assert_eq!(body["smtp"]["port"], Value::Null);
    body["smtp"]["group_minutes"] = json!(0);
    let (status, response) = put(&app, path, body).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let smtp = saved(&app).notifications.smtp;
    assert_eq!(
        (smtp.password.as_str(), smtp.group_minutes),
        ("smtp-pass-1", 0)
    );
}

#[tokio::test]
async fn updates_settings_round_trip() {
    let app = test_app().await;
    let path = "/settings/updates";
    let body = get_ok(&app, path).await;
    assert_eq!(body, json!({"check": true, "auto_install": false}));

    put_changed(&app, path, body.clone(), "check", json!(false)).await;
    assert!(!saved(&app).updates.check);
    let message = message(&app, path, "auto_install", json!(true)).await;
    assert!(message.contains("Applies at the next check"), "{message}");
    assert!(saved(&app).updates.auto_install);

    put_invalid(&app, path, body, "check", json!("sometimes")).await;
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
            "hold_magnet_secs",
            "hold_retry_secs",
            "first_lookup_secs",
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

/// The hold keys (043) save, keep the file's comments, and are checked against each other
#[tokio::test]
async fn precheck_hold_keys_save_and_validate() {
    let app = test_app().await;
    let path = "/settings/precheck";
    let file = config_path(&app);
    let original = std::fs::read_to_string(&file).unwrap();
    let edited = original.replace("[precheck]\n", "# Hold what the caches miss\n[precheck]\n");
    std::fs::write(&file, &edited).unwrap();

    let mut body = get_ok(&app, path).await;
    assert_eq!(body["hold_magnet_secs"], json!(300));
    assert_eq!(body["hold_retry_secs"], json!(30));
    assert_eq!(body["first_lookup_secs"], json!(3));
    body.as_object_mut().unwrap().remove("effective");

    let mut bad = body.clone();
    bad["first_lookup_secs"] = json!(11);
    bad["lookup_timeout_secs"] = json!(10);
    let (status, response) = put(&app, path, bad).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    assert!(response["error"]
        .as_str()
        .unwrap()
        .contains("first_lookup_secs"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), edited);

    body["hold_magnet_secs"] = json!(600);
    body["hold_retry_secs"] = json!(60);
    body["first_lookup_secs"] = json!(2);
    let (status, response) = put(&app, path, body).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let config = saved(&app).precheck;
    assert_eq!(
        (
            config.hold_magnet_secs,
            config.hold_retry_secs,
            config.first_lookup_secs
        ),
        (600, 60, 2)
    );
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("# Hold what the caches miss\n[precheck]"),
        "{text}"
    );
}

#[tokio::test]
async fn a_bad_download_pattern_is_400() {
    // feature 020: Only / Never download are checked strictly and named in the error
    let app = test_app().await;
    let path = "/settings/download";
    let before = std::fs::read_to_string(config_path(&app)).unwrap();
    let mut body = get_ok(&app, path).await;
    body["blacklist_patterns"] = json!("\"sample");
    let (status, response) = put(&app, path, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    let error = response["error"].as_str().unwrap();
    assert!(
        error.contains("blacklist_patterns (Never download): unclosed quote"),
        "{error}"
    );
    assert_eq!(std::fs::read_to_string(config_path(&app)).unwrap(), before);

    message(&app, path, "whitelist_patterns", json!("*.mkv,*.mp4")).await;
    assert_eq!(
        saved(&app).download.whitelist_patterns.as_deref(),
        Some("*.mkv,*.mp4")
    );
}

// ---- Event hooks (feature 027) --------------------------------------------------------

fn url_hook() -> Value {
    json!({
        "name": "ha",
        "events": ["download.finished"],
        "url": "https://ha.lan/api/webhook/abc",
        "headers": { "Authorization": "Bearer hook-secret" },
        "timeout_secs": 10
    })
}

#[tokio::test]
async fn hooks_save_masks_values_and_applies_now() {
    let app = test_app().await;
    assert_eq!(
        get_ok(&app, "/settings/hooks").await,
        json!({ "hooks": [] })
    );

    let (status, body) = put(&app, "/settings/hooks", json!({ "hooks": [url_hook()] })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("Applies now: hooks"),
        "{body}"
    );
    // The running service has it at once
    assert_eq!(app.state.hooks.status()[0].name, "ha");

    let view = get_ok(&app, "/settings/hooks").await;
    assert_eq!(view["hooks"][0]["headers"]["Authorization"], "********");
    assert!(!view.to_string().contains("hook-secret"));
    assert_eq!(
        saved(&app).hooks[0].headers["Authorization"],
        "Bearer hook-secret"
    );

    // Sending the mask back keeps the stored value; a changed value alone is a change
    let mut hook = view["hooks"][0].clone();
    hook["timeout_secs"] = json!(20);
    let (status, _) = put(&app, "/settings/hooks", json!({ "hooks": [hook] })).await;
    assert_eq!(status, StatusCode::OK);
    let stored = saved(&app);
    assert_eq!(stored.hooks[0].timeout_secs, 20);
    assert_eq!(
        stored.hooks[0].headers["Authorization"],
        "Bearer hook-secret"
    );

    let mut hook = view["hooks"][0].clone();
    hook["timeout_secs"] = json!(20);
    hook["headers"]["Authorization"] = json!("Bearer new");
    let (_, body) = put(&app, "/settings/hooks", json!({ "hooks": [hook] })).await;
    assert!(
        body["message"].as_str().unwrap().contains("hooks"),
        "{body}"
    );
    assert_eq!(saved(&app).hooks[0].headers["Authorization"], "Bearer new");
}

/// Feature 038: a hook that is a pipeline step keeps its stage, order and flags, and a bad
/// step is refused naming the hook
#[tokio::test]
async fn pipeline_step_hooks_save_in_order_and_bad_ones_are_refused() {
    let app = test_app().await;
    let step = |name: &str, stage: &str| {
        json!({
            "name": name, "events": [], "command": ["/bin/true"], "timeout_secs": 10,
            "stage": stage, "required": false, "categories": ["radarr"]
        })
    };
    let hooks = json!({ "hooks": [step("b", "post_processing"), step("a", "post_processing"), step("c", "pre_download")] });
    let (status, body) = put(&app, "/settings/hooks", hooks).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let view = get_ok(&app, "/settings/hooks").await;
    let names: Vec<_> = view["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["b", "a", "c"],
        "list order is the order inside a stage"
    );
    assert_eq!(view["hooks"][0]["stage"], "post_processing");
    assert_eq!(view["hooks"][0]["required"], false);
    assert_eq!(view["hooks"][0]["categories"], json!(["radarr"]));

    // A step takes no events
    let mut bad = step("x", "post_processing");
    bad["events"] = json!(["download.finished"]);
    let (status, body) = put(&app, "/settings/hooks", json!({ "hooks": [bad] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["error"].as_str().unwrap().contains("\"x\""), "{body}");
    assert_eq!(saved(&app).hooks.len(), 3, "a refused save changes nothing");
}

#[tokio::test]
async fn pipeline_settings_validate_and_apply_now() {
    let mut app = test_app().await;
    let pipeline = app.attach_pipeline().await;
    let view = get_ok(&app, "/settings/pipeline").await;
    assert_eq!(view["concurrency"], 2);
    assert_eq!(view["optional_retries"], json!([60, 300, 600]));
    assert_eq!(view["staging"], "auto");
    assert_eq!(view["history_days"], 30);

    let (status, body) = put(
        &app,
        "/settings/pipeline",
        json!({"concurrency": 4, "optional_retries": [5, 10], "staging": "off", "history_days": 90}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("Applies now"),
        "{body}"
    );
    let stored = saved(&app).pipeline;
    assert_eq!(
        (stored.concurrency, stored.optional_retries.clone()),
        (4, vec![5, 10])
    );
    assert_eq!(stored.history_days, 90);
    let _ = pipeline;

    for bad in [
        json!({"concurrency": 0}),
        json!({"concurrency": 17}),
        json!({"optional_retries": [1,1,1,1,1,1,1,1,1,1,1]}),
        json!({"staging": "maybe"}),
        json!({"history_days": 0}),
        json!({"history_days": 3651}),
    ] {
        let (status, body) = put(&app, "/settings/pipeline", bad.clone()).await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
            "{bad}: {status} {body}"
        );
    }
    assert_eq!(
        saved(&app).pipeline.concurrency,
        4,
        "refused saves change nothing"
    );
}

#[tokio::test]
async fn subtitles_settings_validate_mask_the_key_and_apply_now() {
    let app = test_app().await;
    let view = get_ok(&app, "/settings/subtitles").await;
    assert_eq!(view["languages"], json!([]));
    assert_eq!(view["transcription"]["enabled"], false);
    assert_eq!(view["transcription"]["api_key"], "");
    assert!(view["known_languages"].as_array().unwrap().len() > 10);
    assert_eq!(view["live"].as_array().unwrap().len(), 8);
    assert!(view["applies_now"]
        .as_str()
        .unwrap()
        .contains("waiting steps"));

    let mut body = json!({
        "languages": ["EN", "es"],
        "excluded_categories": ["radarr"],
        "transcription": {"enabled": true, "url": "http://nas:8000", "api_key": "whisper-secret-key",
                          "model": "m", "timeout_secs": 900, "chunk_secs": 600},
    });
    let (status, saved_body) = put(&app, "/settings/subtitles", body.clone()).await;
    assert_eq!(status, StatusCode::OK, "{saved_body}");
    assert!(
        saved_body["message"]
            .as_str()
            .unwrap()
            .contains("Applies now"),
        "{saved_body}"
    );
    assert!(
        !saved_body.to_string().contains("whisper-secret-key"),
        "{saved_body}"
    );
    let view = get_ok(&app, "/settings/subtitles").await;
    assert_eq!(view["languages"], json!(["en", "es"]));
    assert_eq!(view["transcription"]["api_key"], "********");
    assert_eq!(
        saved(&app).subtitles.transcription.api_key,
        "whisper-secret-key"
    );

    // The page sends the mask back: the stored key stays
    body["transcription"]["api_key"] = json!("********");
    body["transcription"]["model"] = json!("m2");
    let (status, _) = put(&app, "/settings/subtitles", body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        saved(&app).subtitles.transcription.api_key,
        "whisper-secret-key"
    );
    assert_eq!(saved(&app).subtitles.transcription.model, "m2");

    // An empty key clears it
    body["transcription"]["api_key"] = json!("");
    let (status, _) = put(&app, "/settings/subtitles", body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved(&app).subtitles.transcription.api_key, "");

    for bad in [
        json!({"languages": ["xx-not-a-language"]}),
        json!({"languages": ["en","fr","de","es","it","pt","nl","sv","da","no","fi"]}),
        json!({"transcription": {"enabled": true, "url": "ftp://nas"}}),
        json!({"transcription": {"chunk_secs": 5}}),
        json!({"transcription": {"timeout_secs": 1}}),
    ] {
        let (status, body) = put(&app, "/settings/subtitles", bad.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}: {body}");
        assert!(
            body["error"].as_str().unwrap().contains("subtitles"),
            "{body}"
        );
    }
    assert_eq!(
        saved(&app).subtitles.languages,
        ["en", "es"],
        "refused saves change nothing"
    );
}

#[tokio::test]
async fn subtitles_preview_lists_changes_and_file_names() {
    let app = test_app().await;
    let (status, body) = put(&app, "/settings/subtitles", json!({"languages": ["en"]})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let response = app
        .post_json(
            "/settings/subtitles/preview",
            json!({"video": "Movie.2024.1080p.mkv", "settings": {"languages": ["en", "es"]}}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let preview = body_json(response).await;
    assert_eq!(preview["valid"], true, "{preview}");
    assert_eq!(preview["changes"][0]["key"], "languages", "{preview}");
    assert_eq!(
        preview["files"],
        json!(["Movie.2024.1080p.en.srt", "Movie.2024.1080p.es.srt"])
    );
    assert!(preview["forced_sdh_note"]
        .as_str()
        .unwrap()
        .contains("forced"));
    assert_eq!(
        saved(&app).subtitles.languages,
        ["en"],
        "a preview saves nothing"
    );

    let response = app
        .post_json(
            "/settings/subtitles/preview",
            json!({"settings": {"languages": ["zz-nope"]}}),
        )
        .await;
    let preview = body_json(response).await;
    assert_eq!(preview["valid"], false, "{preview}");
}

#[tokio::test]
async fn a_bad_hook_is_400_naming_hook_and_field() {
    let app = test_app().await;
    let mut hook = url_hook();
    hook["events"] = json!(["download.done"]);
    let (status, body) = put(&app, "/settings/hooks", json!({ "hooks": [hook] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body["error"],
        "hooks \"ha\".events: unknown event \"download.done\""
    );

    // A mask with no stored value to stand for
    let mut hook = url_hook();
    hook["headers"]["Authorization"] = json!("********");
    let (status, body) = put(&app, "/settings/hooks", json!({ "hooks": [hook] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("still masked"),
        "{body}"
    );
    assert!(saved(&app).hooks.is_empty());
    assert!(app.state.hooks.status().is_empty());
}

// ---- Lua plugins (feature 028) --------------------------------------------------------

fn ntfy(token: &str) -> Value {
    json!({
        "name": "ntfy",
        "enabled": true,
        "config": { "server": "https://ntfy.example.org", "topic": "nas" },
        "secrets": { "token": token }
    })
}

fn ntfy_status(app: &TestApp) -> putio_sync_plugins::PluginStatus {
    app.state
        .plugins
        .status()
        .into_iter()
        .find(|p| p.name == "ntfy")
        .unwrap()
}

#[tokio::test]
async fn plugins_save_masks_secrets_approves_hosts_and_applies_now() {
    let app = test_app().await;
    assert_eq!(
        get_ok(&app, "/settings/plugins").await,
        json!({ "plugins_dir": null, "plugins": [] })
    );

    let body = json!({ "plugins_dir": null, "plugins": [ntfy("plugin-secret-1")] });
    let (status, body) = put(&app, "/settings/plugins", body).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("Applies now: plugins"),
        "{body}"
    );
    // Enabling records the hosts it needs, and the running service has it at once
    let stored = saved(&app);
    assert_eq!(stored.plugins[0].approved_hosts, vec!["ntfy.example.org"]);
    assert_eq!(stored.plugins[0].secrets["token"], "plugin-secret-1");
    let status = ntfy_status(&app);
    assert_eq!(status.state, putio_sync_plugins::PluginState::Enabled);
    assert_eq!(status.approved_hosts, vec!["ntfy.example.org"]);

    let view = get_ok(&app, "/settings/plugins").await;
    assert_eq!(view["plugins"][0]["secrets"]["token"], "********");
    assert!(!view.to_string().contains("plugin-secret-1"));

    // Sending the mask back keeps the stored value
    let mut plugin = view["plugins"][0].clone();
    plugin["config"]["topic"] = json!("other");
    let body = json!({ "plugins_dir": null, "plugins": [plugin] });
    let (status, _) = put(&app, "/settings/plugins", body).await;
    assert_eq!(status, StatusCode::OK);
    let stored = saved(&app);
    assert_eq!(stored.plugins[0].config["topic"], "other");
    assert_eq!(stored.plugins[0].secrets["token"], "plugin-secret-1");

    // A changed secret alone is a change
    let mut plugin = view["plugins"][0].clone();
    plugin["config"]["topic"] = json!("other");
    plugin["secrets"]["token"] = json!("plugin-secret-2");
    let body = json!({ "plugins_dir": null, "plugins": [plugin] });
    let (_, body) = put(&app, "/settings/plugins", body).await;
    assert!(
        body["message"].as_str().unwrap().contains("plugins"),
        "{body}"
    );
    assert_eq!(saved(&app).plugins[0].secrets["token"], "plugin-secret-2");
}

#[tokio::test]
async fn a_bad_plugin_value_is_400_naming_plugin_and_field() {
    let app = test_app().await;
    let mut plugin = ntfy("x");
    plugin["config"]["server"] = json!(5);
    let body = json!({ "plugins_dir": null, "plugins": [plugin] });
    let (status, body) = put(&app, "/settings/plugins", body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("plugins \"ntfy\".config.server"),
        "{body}"
    );

    let mut plugin = ntfy("********");
    plugin["enabled"] = json!(false);
    let body = json!({ "plugins_dir": null, "plugins": [plugin] });
    let (status, body) = put(&app, "/settings/plugins", body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("still masked"),
        "{body}"
    );
    assert!(saved(&app).plugins.is_empty());
    assert_eq!(
        ntfy_status(&app).state,
        putio_sync_plugins::PluginState::Disabled
    );
}

#[tokio::test]
async fn a_new_plugins_folder_is_read_at_once() {
    let app = test_app().await;
    let dir = tempfile::tempdir().unwrap();
    let mine = dir.path().join("mine");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::write(
        mine.join("plugin.toml"),
        "name = \"mine\"\nversion = \"1\"\nevents = [\"*\"]\n[[config]]\nkey = \"n\"\ntype = \"integer\"\n",
    )
    .unwrap();
    std::fs::write(mine.join("main.lua"), "return {on_event = function() end}").unwrap();

    // Values are checked against the folder being saved, not the current one
    let body = json!({
        "plugins_dir": dir.path(),
        "plugins": [{ "name": "mine", "enabled": true, "config": { "n": "x" } }]
    });
    let (status, body) = put(&app, "/settings/plugins", body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let body = json!({
        "plugins_dir": dir.path(),
        "plugins": [{ "name": "mine", "enabled": true, "config": { "n": 3 } }]
    });
    let (status, body) = put(&app, "/settings/plugins", body).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(app.state.plugins.dir().as_deref(), Some(dir.path()));
    let mine = app
        .state
        .plugins
        .status()
        .into_iter()
        .find(|p| p.name == "mine")
        .unwrap();
    assert_eq!(mine.state, putio_sync_plugins::PluginState::Enabled);
}
