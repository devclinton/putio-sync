//! Folder rule routes (feature 009): set, inherit and clear a rule by Put.io folder id,
//! presets, the dry-run preview and the recorded decisions. Saves keep the config file's
//! comments, and a refused save changes nothing.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_app_without_db, TestApp};
use putio_sync_core::config::Config;
use putio_sync_telemetry::RuleDecision;
use serde_json::{json, Value};

fn entry(id: i64, name: &str, file_type: &str, parent_id: i64) -> Value {
    json!({
        "id": id,
        "name": name,
        "size": 100,
        "file_type": file_type,
        "parent_id": parent_id,
        "is_folder": file_type == "FOLDER",
        "is_shared": false,
    })
}

const COMMENT: &str = "# kept by every rule save";

/// `Movies` (100) at the root with `film.mkv` (101), `setup.exe` (102) and `Extras`
/// (110), which holds `notes.txt` (111). The config file starts with a comment.
async fn tree_app(app: Option<TestApp>) -> (TestApp, Vec<mockito::Mock>) {
    let mut app = match app {
        Some(app) => app,
        None => test_app().await,
    };
    let movies = entry(100, "Movies", "FOLDER", 0);
    let extras = entry(110, "Extras", "FOLDER", 100);
    let mut mocks = vec![
        app.mock_file(100, json!({ "status": "OK", "file": movies }))
            .await,
        app.mock_file(110, json!({ "status": "OK", "file": extras }))
            .await,
        app.mock_file(
            101,
            json!({ "status": "OK", "file": entry(101, "film.mkv", "VIDEO", 100) }),
        )
        .await,
        app.mock_files_list(
            100,
            json!({
                "status": "OK",
                "parent": movies,
                "files": [
                    entry(101, "film.mkv", "VIDEO", 100),
                    entry(102, "setup.exe", "FILE", 100),
                    extras,
                ],
            }),
        )
        .await,
        app.mock_files_list(
            110,
            json!({
                "status": "OK",
                "parent": extras,
                "files": [entry(111, "notes.txt", "TEXT", 110)],
            }),
        )
        .await,
    ];
    mocks.push(
        app.putio
            .mock("GET", "/files/999")
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"ERROR","error_type":"NotFound","status_code":404}"#)
            .create_async()
            .await,
    );
    let path = config_path(&app);
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{COMMENT}\n{text}")).unwrap();
    (app, mocks)
}

fn config_path(app: &TestApp) -> std::path::PathBuf {
    app.state
        .config_path
        .as_ref()
        .expect("config path")
        .to_path_buf()
}

fn file_text(app: &TestApp) -> String {
    std::fs::read_to_string(config_path(app)).unwrap()
}

fn saved(app: &TestApp) -> Config {
    Config::load(config_path(app)).expect("config file loads")
}

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

async fn get(app: &TestApp, path: &str) -> Value {
    let (status, body) = call(app, Method::GET, path, None).await;
    assert_eq!(status, StatusCode::OK, "GET {path}: {body}");
    body
}

#[tokio::test]
async fn a_rule_is_set_inherited_and_cleared() {
    let (app, _mocks) = tree_app(None).await;

    let (status, body) = call(
        &app,
        Method::PUT,
        "/rules/folder/100",
        Some(json!({ "preset": "video", "allow": [".NFO"], "delete_blocked": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .starts_with("Rule for Movies saved."),
        "{body}"
    );

    let movies = get(&app, "/rules/folder/100").await;
    assert_eq!(movies["path"], "Movies");
    assert_eq!(movies["rule"]["preset"], "video");
    assert_eq!(movies["rule"]["allow"], json!(["nfo"]));
    assert_eq!(movies["effective"]["filter"]["from"], "Movies");

    let extras = get(&app, "/rules/folder/110").await;
    assert_eq!(extras["path"], "Movies/Extras");
    assert_eq!(extras["rule"], Value::Null);
    let effective = &extras["effective"];
    assert_eq!(
        effective["preset"],
        json!({ "value": "video", "from": "Movies" })
    );
    assert_eq!(
        effective["delete_blocked"],
        json!({ "value": true, "from": "Movies" })
    );
    assert_eq!(
        effective["on_existing"],
        json!({ "value": "resume", "from": null })
    );
    let only: Vec<&str> = effective["filter"]["only"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(only.contains(&"mkv") && only.contains(&"nfo"), "{only:?}");

    // Saved to the file with its comment, and handed to the download manager
    let text = file_text(&app);
    assert!(text.starts_with(COMMENT), "{text}");
    assert!(text.contains("[[download.rules]]"), "{text}");
    assert_eq!(saved(&app).download.rules.len(), 1);
    let manager = app.state.download_manager.get_config().await;
    assert!(manager
        .effective("Movies/Extras")
        .filter
        .value
        .only
        .is_some());

    let (status, body) = call(&app, Method::DELETE, "/rules/folder/100", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["message"]
        .as_str()
        .unwrap()
        .starts_with("Rule for Movies removed."));
    assert_eq!(get(&app, "/rules").await["rules"], json!([]));
    assert!(saved(&app).download.rules.is_empty());
    assert!(file_text(&app).starts_with(COMMENT));

    let (_, body) = call(&app, Method::DELETE, "/rules/folder/100", None).await;
    assert_eq!(body["message"], "No rule for Movies; nothing changed.");
}

#[tokio::test]
async fn an_empty_body_clears_the_rule() {
    let (app, _mocks) = tree_app(None).await;
    call(
        &app,
        Method::PUT,
        "/rules/folder/100",
        Some(json!({ "skip": true })),
    )
    .await;
    let (status, body) = call(&app, Method::PUT, "/rules/folder/100", Some(json!({}))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(saved(&app).download.rules.is_empty());
}

#[tokio::test]
async fn root_files_and_unknown_folders_are_refused() {
    let (app, _mocks) = tree_app(None).await;

    let root = get(&app, "/rules/folder/0").await;
    assert_eq!(root["path"], "");
    assert_eq!(root["rule"], Value::Null);
    assert_eq!(root["effective"]["filter"]["from"], Value::Null);

    let (status, body) = call(
        &app,
        Method::PUT,
        "/rules/folder/0",
        Some(json!({ "skip": true })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "The root uses the Download settings");

    let (status, _) = call(&app, Method::GET, "/rules/folder/101", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(&app, Method::GET, "/rules/folder/999", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_bad_rule_saves_nothing() {
    let (app, _mocks) = tree_app(None).await;
    let before = file_text(&app);

    for rule in [json!({ "preset": "nope" }), json!({ "block": ["a b"] })] {
        let (status, body) = call(&app, Method::PUT, "/rules/folder/100", Some(rule)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(body["error"].is_string());
    }
    let (status, _) = call(
        &app,
        Method::PUT,
        "/rules/folder/100",
        Some(json!({ "skip": "yes" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    assert_eq!(file_text(&app), before);
    assert_eq!(get(&app, "/rules").await["rules"], json!([]));
}

#[tokio::test]
async fn rules_are_removed_by_path() {
    let (app, _mocks) = tree_app(None).await;
    call(
        &app,
        Method::PUT,
        "/rules/folder/110",
        Some(json!({ "skip": true })),
    )
    .await;

    let (status, body) = call(
        &app,
        Method::POST,
        "/rules/delete",
        Some(json!({ "path": "/Movies/Extras/" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _) = call(
        &app,
        Method::POST,
        "/rules/delete",
        Some(json!({ "path": "Movies/Extras" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn presets_are_added_overridden_reset_and_deleted() {
    let (app, _mocks) = tree_app(None).await;

    let all = get(&app, "/rules").await;
    let names: Vec<&str> = all["presets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["video", "audio", "ebooks", "no-binaries", "everything"]
    );
    assert_eq!(all["global"]["on_existing"], "resume");

    let (status, body) = call(
        &app,
        Method::PUT,
        "/rules/presets/comics",
        Some(json!({ "only": ["CBZ", ".cbr"] })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    call(
        &app,
        Method::PUT,
        "/rules/folder/100",
        Some(json!({ "preset": "comics" })),
    )
    .await;

    let comics = get(&app, "/rules").await["presets"][5].clone();
    assert_eq!(comics["name"], "comics");
    assert_eq!(comics["builtin"], false);
    assert_eq!(comics["only"], json!(["cbz", "cbr"]));
    assert_eq!(comics["used_by"], json!(["Movies"]));
    assert!(file_text(&app).contains("[download.presets.comics]"));

    let (status, body) = call(&app, Method::DELETE, "/rules/presets/comics", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "Preset comics is used by 1 rule");
    assert_eq!(body["details"], "Movies");

    call(&app, Method::DELETE, "/rules/folder/100", None).await;
    let (status, _) = call(&app, Method::DELETE, "/rules/presets/comics", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(&app, Method::DELETE, "/rules/presets/comics", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A built-in is overridden, then reset
    call(
        &app,
        Method::PUT,
        "/rules/presets/video",
        Some(json!({ "only": ["mkv"] })),
    )
    .await;
    let video = get(&app, "/rules").await["presets"][0].clone();
    assert_eq!(video["overridden"], true);
    assert_eq!(video["only"], json!(["mkv"]));
    let (status, body) = call(&app, Method::DELETE, "/rules/presets/video", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(get(&app, "/rules").await["presets"][0]["overridden"], false);

    for (name, preset) in [
        ("Bad%20Name", json!({ "block": ["iso"] })),
        ("both", json!({ "only": ["a"], "block": ["b"] })),
        ("empty", json!({ "only": [] })),
    ] {
        let (status, _) = call(
            &app,
            Method::PUT,
            &format!("/rules/presets/{name}"),
            Some(preset),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}");
    }
    assert!(saved(&app).download.presets.is_empty());
}

#[tokio::test]
async fn preview_decides_without_changing_anything() {
    let (mut app, _mocks) = tree_app(None).await;
    let delete = app
        .putio
        .mock("POST", "/files/delete")
        .expect(0)
        .create_async()
        .await;
    app.db()
        .queue()
        .add(
            111,
            "notes.txt",
            100,
            "http://unused",
            "Movies/Extras",
            0,
            putio_sync_db::models::QueueStatus::Pending,
        )
        .await
        .unwrap();
    let before = file_text(&app);

    // Saved rules: only the global blocklist
    let (status, saved_rules) = call(
        &app,
        Method::POST,
        "/rules/preview",
        Some(json!({ "folder_id": 100 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved_rules}");
    assert_eq!(saved_rules["path"], "Movies");
    assert_eq!(saved_rules["files"], 3);
    assert_eq!(saved_rules["blocked"], 1);
    assert_eq!(saved_rules["would_delete"], 0);

    let (status, body) = call(
        &app,
        Method::POST,
        "/rules/preview",
        Some(json!({ "folder_id": 100, "rule": { "preset": "video", "delete_blocked": true } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["download"], 1);
    assert_eq!(body["blocked"], 2);
    // notes.txt is queued, so only setup.exe would go
    assert_eq!(body["would_delete"], 1);
    let exe = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "setup.exe")
        .unwrap();
    assert_eq!(exe["decision"], "blocked");
    assert_eq!(exe["rule"], "Movies");
    assert_eq!(exe["would_delete"], true);
    assert_eq!(body["truncated"], false);

    let (status, _) = call(
        &app,
        Method::POST,
        "/rules/preview",
        Some(json!({ "folder_id": 100, "rule": { "preset": "nope" } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    assert_eq!(file_text(&app), before);
    assert_eq!(get(&app, "/rules").await["rules"], json!([]));
    delete.assert_async().await;
}

#[tokio::test]
async fn decisions_are_listed_and_cleared() {
    let (app, _mocks) = tree_app(None).await;
    app.db()
        .rule_decisions()
        .record(
            102,
            "setup.exe",
            "Movies",
            RuleDecision::Blocked,
            "extension exe is blocked",
            None,
        )
        .await
        .unwrap();

    let body = get(&app, "/rules/decisions").await;
    let item = &body["items"][0];
    assert_eq!(item["name"], "setup.exe");
    assert_eq!(item["decision"], "blocked");
    assert_eq!(item["rule"], Value::Null);
    assert!(item["decided_at"].as_str().unwrap().ends_with('Z'));

    for limit in ["0", "501"] {
        let (status, _) = call(
            &app,
            Method::GET,
            &format!("/rules/decisions?limit={limit}"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    let (status, body) = call(&app, Method::POST, "/rules/decisions/clear", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["message"], "Cleared 1 decision");
    assert_eq!(get(&app, "/rules/decisions").await["items"], json!([]));
}

#[tokio::test]
async fn decisions_need_the_database() {
    let app = test_app_without_db().await;
    let (status, _) = call(&app, Method::GET, "/rules/decisions", None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn rules_need_the_token() {
    let (app, _mocks) = tree_app(None).await;
    let response = app.req(Method::GET, "/rules", None, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// Feature 040: a folder's `on_existing` is set, inherited and cleared, and a body with
/// only the old `skip_existing` is mapped
#[tokio::test]
async fn on_existing_is_set_inherited_and_cleared() {
    let (app, _mocks) = tree_app(None).await;

    let (status, body) = call(
        &app,
        Method::PUT,
        "/rules/folder/100",
        Some(json!({ "on_existing": "keep_both" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let movies = get(&app, "/rules/folder/100").await;
    assert_eq!(movies["rule"]["on_existing"], "keep_both");
    assert_eq!(
        movies["effective"]["on_existing"],
        json!({ "value": "keep_both", "from": "Movies" })
    );
    let extras = get(&app, "/rules/folder/110").await;
    assert_eq!(
        extras["effective"]["on_existing"],
        json!({ "value": "keep_both", "from": "Movies" })
    );
    assert!(!file_text(&app).contains("skip_existing"));

    let (status, body) = call(
        &app,
        Method::PUT,
        "/rules/folder/110",
        Some(json!({ "skip_existing": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let extras = get(&app, "/rules/folder/110").await;
    assert_eq!(extras["rule"]["on_existing"], "overwrite");
    assert!(extras["rule"].get("skip_existing").is_none(), "{extras}");

    let (status, body) = call(
        &app,
        Method::PUT,
        "/rules/folder/100",
        Some(json!({ "on_existing": "sideways" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        get(&app, "/rules/folder/100").await["rule"]["on_existing"],
        "keep_both"
    );

    let (status, _) = call(&app, Method::DELETE, "/rules/folder/100", None).await;
    assert!(status.is_success(), "{status}");
    let movies = get(&app, "/rules/folder/100").await;
    assert_eq!(
        movies["effective"]["on_existing"],
        json!({ "value": "resume", "from": null })
    );
}

#[tokio::test]
async fn patterns_are_saved_inherited_and_previewed() {
    let (app, _mocks) = tree_app(None).await;
    let (status, body) = call(
        &app,
        Method::PUT,
        "/rules/folder/100",
        Some(json!({ "exclude": " Extras/* ", "delete_blocked": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(file_text(&app).contains("exclude = \"Extras/*\""));

    let movies = get(&app, "/rules/folder/100").await;
    assert_eq!(movies["rule"]["exclude"], "Extras/*");
    assert_eq!(movies["rule"]["include"], Value::Null);
    let extras = get(&app, "/rules/folder/110").await;
    assert_eq!(
        extras["effective"]["exclude"],
        json!({ "value": "Extras/*", "from": "Movies" })
    );
    assert_eq!(
        extras["effective"]["include"],
        json!({ "value": "", "from": null })
    );

    // the saved rule: notes.txt in the nested folder is refused by the pattern
    let (status, body) = call(
        &app,
        Method::POST,
        "/rules/preview",
        Some(json!({ "folder_id": 100 })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["download"], 1);
    assert_eq!(body["blocked"], 2);
    let notes = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "notes.txt")
        .unwrap();
    assert_eq!(notes["decision"], "blocked");
    assert_eq!(notes["reason"], "matches never-download pattern `Extras/*`");
    assert_eq!(notes["rule"], "Movies");
    assert_eq!(notes["would_delete"], true);
    assert_eq!(
        body["downloads"],
        json!([{ "file_id": 101, "name": "film.mkv", "path": "Movies" }])
    );

    // a candidate with AND / OR / NOT
    let (status, body) = call(
        &app,
        Method::POST,
        "/rules/preview",
        Some(json!({ "folder_id": 100, "rule": { "include": "*.mkv OR *.txt -notes" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["download"], 1);
    let notes = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "notes.txt")
        .unwrap();
    assert_eq!(
        notes["reason"],
        "doesn't match only-download pattern `*.mkv OR *.txt -notes`"
    );

    // "" clears the inherited pattern and keeps the rule
    let (status, body) = call(
        &app,
        Method::PUT,
        "/rules/folder/110",
        Some(json!({ "exclude": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let extras = get(&app, "/rules/folder/110").await;
    assert_eq!(extras["rule"]["exclude"], "");
    assert_eq!(
        extras["effective"]["exclude"],
        json!({ "value": "", "from": "Movies/Extras" })
    );
}

#[tokio::test]
async fn a_bad_pattern_is_refused() {
    let (app, _mocks) = tree_app(None).await;
    let before = file_text(&app);
    for body in [
        json!({ "exclude": "\"abc" }),
        json!({ "include": "*.{mkv" }),
    ] {
        let (status, err) = call(&app, Method::PUT, "/rules/folder/100", Some(body)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{err}");
        let text = err.to_string();
        assert!(
            text.contains("Never download: unclosed quote")
                || text.contains("Only download: unbalanced { }"),
            "{text}"
        );
    }
    let (status, _) = call(
        &app,
        Method::POST,
        "/rules/preview",
        Some(json!({ "folder_id": 100, "rule": { "exclude": "OR" } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(file_text(&app), before);
}

#[tokio::test]
async fn preview_takes_account_wide_patterns_at_the_root() {
    let (mut app, _mocks) = tree_app(None).await;
    let _root = app
        .mock_files_list(
            0,
            json!({ "status": "OK", "parent": entry(0, "Your Files", "FOLDER", -1), "files": [entry(100, "Movies", "FOLDER", 0)] }),
        )
        .await;
    let before = file_text(&app);
    let (status, body) = call(
        &app,
        Method::POST,
        "/rules/preview",
        Some(json!({ "folder_id": 0, "global": { "include": "*.mkv", "exclude": "" } })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["files"], 3);
    assert_eq!(body["download"], 1);
    let notes = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["name"] == "notes.txt")
        .unwrap();
    assert_eq!(
        notes["reason"],
        "doesn't match only-download pattern `*.mkv`"
    );
    assert_eq!(notes["rule"], Value::Null);

    let (status, err) = call(
        &app,
        Method::POST,
        "/rules/preview",
        Some(json!({ "folder_id": 0, "global": { "exclude": "\"x" } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        err.to_string()
            .contains("blacklist_patterns (Never download): unclosed quote"),
        "{err}"
    );
    let (status, _) = call(
        &app,
        Method::POST,
        "/rules/preview",
        Some(json!({ "folder_id": 0, "rule": { "exclude": "x" } })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(file_text(&app), before);
    assert_eq!(get(&app, "/rules").await["global"]["include"], "");
}
