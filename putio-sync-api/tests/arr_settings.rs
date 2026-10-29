//! The Settings card's `/arr` routes (features 011 and 018, contracts/api.md)

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, body_text, test_app, test_app_with, TestApp};
use mockito::Matcher;
use putio_sync_core::config::{ArrConfig, ArrKind, ArrServer, Config};
use serde_json::{json, Value};

fn server(kind: ArrKind, url: String, api_key: Option<&str>, category: &str) -> ArrServer {
    ArrServer {
        kind,
        name: None,
        url,
        api_key: api_key.map(str::to_string),
        category: Some(category.to_string()),
    }
}

fn config_path(app: &TestApp) -> std::path::PathBuf {
    app._dir.path().join("config.toml")
}

fn file_config(app: &TestApp) -> Config {
    Config::load(config_path(app)).unwrap()
}

/// The config file without `[arr]`, to check nothing else changed
fn without_arr(app: &TestApp) -> toml::Value {
    let mut value: toml::Value = std::fs::read_to_string(config_path(app))
        .unwrap()
        .parse()
        .unwrap();
    value.as_table_mut().unwrap().remove("arr");
    value
}

async fn put_client(app: &TestApp, body: Value) -> axum::response::Response {
    app.req(Method::PUT, "/arr/client", Some(&app.token), Some(body))
        .await
}

#[tokio::test]
async fn disabled_shows_defaults() {
    let app = test_app().await;
    let body = body_json(app.get("/arr").await).await;
    let client = &body["client"];
    assert_eq!(client["enabled"], false);
    assert_eq!(client["available"], true);
    assert_eq!(client["password_set"], false);
    assert_eq!(client["username"], "putio-sync");
    assert_eq!(client["env"], json!({"password": false}));
    assert_eq!(client["connection"]["implementation"], "Transmission");
    assert_eq!(client["connection"]["url_base"], "/transmission/");
    assert_eq!(client["connection"]["port"], 15432);
    assert_eq!(client["connection"]["hosts"][0], "putio-sync");
    assert_eq!(body["applications"], json!([]));
    // The counts moved to the Status page
    assert!(body.get("items").is_none() && body.get("clients_seen").is_none());
}

#[tokio::test]
async fn enabled_lists_applications_without_keys() {
    let app = test_app_with(|config| {
        config.arr = ArrConfig {
            enabled: true,
            password: Some("pw-secret".into()),
            servers: vec![
                server(
                    ArrKind::Sonarr,
                    "http://nas:8989/".into(),
                    Some("k1-secret"),
                    "tv",
                ),
                server(ArrKind::Radarr, "https://nas/radarr".into(), None, "radarr"),
            ],
            ..ArrConfig::default()
        };
    })
    .await;
    let response = app.get("/arr").await;
    let text = body_text(response).await;
    assert!(!text.contains("secret"), "{}", text);
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["client"]["enabled"], true);
    assert_eq!(body["client"]["password_set"], true);
    assert_eq!(
        body["client"]["download_dir"],
        app._dir.path().display().to_string().trim_end_matches('/')
    );
    let apps = body["applications"].as_array().unwrap();
    assert_eq!(apps.len(), 2);
    assert_eq!(apps[0]["kind"], "sonarr");
    assert_eq!(apps[0]["name"], "Sonarr");
    assert_eq!(apps[0]["url"], "http://nas:8989");
    assert_eq!(apps[0]["host"], "nas");
    assert_eq!(apps[0]["port"], 8989);
    assert_eq!(apps[0]["api_key_set"], true);
    assert_eq!(apps[0]["api_key_env"], false);
    assert_eq!(apps[0]["legacy_category"], "tv");
    assert_eq!(apps[1]["name"], "Radarr");
    assert_eq!(apps[1]["ssl"], true);
    assert_eq!(apps[1]["port"], 443);
    assert_eq!(apps[1]["url_base"], "/radarr");
    assert_eq!(apps[1]["api_key_set"], false);
}

#[tokio::test]
async fn status_has_the_counts_and_contacts() {
    let app = test_app_with(|config| {
        config.arr.enabled = true;
        config.arr.password = Some("secret-pass".into());
    })
    .await;
    let empty = body_json(app.get("/arr/status").await).await;
    assert_eq!(
        empty,
        json!({"enabled": true, "clients_seen": [],
               "items": {"putio": 0, "local": 0, "done": 0, "failed": 0}})
    );

    app.state
        .database
        .as_ref()
        .unwrap()
        .arr()
        .insert(&putio_sync_db::arr::NewArrItem {
            hash: "0123456789abcdef0123456789abcdef01234567",
            name: "Show.S01E01",
            category: "tv",
            transfer_id: 1,
            putio_status: "DOWNLOADING",
            total_size: 0,
        })
        .await
        .unwrap();
    let arr = app.state.arr.as_ref().unwrap();
    arr.service.contacts().record(Some("Sonarr/4.0.15.2941"));
    let text = body_text(app.get("/arr/status").await).await;
    assert!(!text.contains("secret-pass"), "{}", text);
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["items"]["putio"], 1);
    assert_eq!(body["clients_seen"][0]["agent"], "Sonarr");
    assert!(body["clients_seen"][0]["last_contact"].is_string());

    // Off: the card says so
    let off = test_app().await;
    let body = body_json(off.get("/arr/status").await).await;
    assert_eq!(body["enabled"], false);
}

#[tokio::test]
async fn client_needs_a_password_to_turn_on() {
    let app = test_app().await;
    let before = std::fs::read_to_string(config_path(&app)).unwrap();
    let response = put_client(
        &app,
        json!({"enabled": true, "username": "putio-sync", "password": ""}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        body_json(response).await["error"],
        "Set or generate a password first"
    );
    assert_eq!(std::fs::read_to_string(config_path(&app)).unwrap(), before);
    assert!(!app.state.arr.as_ref().unwrap().service.settings().enabled);
}

#[tokio::test]
async fn client_saves_and_applies_without_a_restart() {
    let app = test_app_with(|config| config.cleanup.enabled = true).await;
    let others = without_arr(&app);

    let response = put_client(
        &app,
        json!({"enabled": true, "username": "arr-user", "password": "first-password"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    assert!(!text.contains("first-password"));
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["password_changed"], true);

    let settings = app.state.arr.as_ref().unwrap().service.settings();
    assert!(settings.enabled);
    assert_eq!(settings.username, "arr-user");
    assert_eq!(settings.password.as_deref(), Some("first-password"));
    let file = file_config(&app);
    assert!(file.arr.enabled);
    assert_eq!(file.arr.password.as_deref(), Some("first-password"));
    assert_eq!(without_arr(&app), others);
    assert!(app.state.config.read().await.arr.enabled);

    // An empty password keeps the current one
    let body = body_json(
        put_client(
            &app,
            json!({"enabled": false, "username": "arr-user", "password": ""}),
        )
        .await,
    )
    .await;
    assert_eq!(body["password_changed"], false);
    let settings = app.state.arr.as_ref().unwrap().service.settings();
    assert!(!settings.enabled);
    assert_eq!(settings.password.as_deref(), Some("first-password"));
    assert_eq!(
        file_config(&app).arr.password.as_deref(),
        Some("first-password")
    );
}

#[tokio::test]
async fn a_write_failure_changes_nothing() {
    let app = test_app().await;
    // A directory where the config file was: reading it fails
    std::fs::remove_file(config_path(&app)).unwrap();
    std::fs::create_dir(config_path(&app)).unwrap();
    let response = put_client(
        &app,
        json!({"enabled": true, "username": "putio-sync", "password": "pw"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body_json(response).await["error"],
        "Could not write the config file"
    );
    let settings = app.state.arr.as_ref().unwrap().service.settings();
    assert!(!settings.enabled);
    assert_eq!(settings.password, None);
    assert!(!app.state.config.read().await.arr.enabled);
}

#[tokio::test]
async fn setup_routes_need_auth() {
    let app = test_app().await;
    for path in ["/arr/discover", "/arr/setup/preview", "/arr/setup"] {
        let response = app.req(Method::POST, path, None, Some(json!({}))).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{}", path);
    }
    let response = app
        .req(Method::PUT, "/arr/client", None, Some(json!({})))
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_second_discovery_is_refused() {
    let app = test_app().await;
    let service = app.state.arr.as_ref().unwrap().service.clone();
    let _running = service.discovery_lock().lock().await;
    let response = app.post_json("/arr/discover", json!({})).await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        body_json(response).await["error"],
        "Discovery already running"
    );
}

const ARR_KEY: &str = "radarr-api-key-0123";

async fn mock_radarr(arr: &mut mockito::ServerGuard) {
    arr.mock("GET", "/api/v3/system/status")
        .match_header("x-api-key", ARR_KEY)
        .with_body(r#"{"appName":"Radarr","version":"5.14.0.9383"}"#)
        .create_async()
        .await;
    arr.mock("GET", "/api/v3/downloadclient")
        .with_body("[]")
        .create_async()
        .await;
    arr.mock("GET", "/api/v3/downloadclient/schema")
        .with_body(
            json!([{"implementation": "Transmission", "configContract": "TransmissionSettings",
                    "fields": [{"name": "host"}, {"name": "movieCategory"}]}])
            .to_string(),
        )
        .create_async()
        .await;
}

#[tokio::test]
async fn preview_reports_what_will_change() {
    let mut arr = mockito::Server::new_async().await;
    mock_radarr(&mut arr).await;
    let app = test_app().await;
    let body = body_json(
        app.post_json(
            "/arr/setup/preview",
            json!({"target": {"url": arr.url()}, "api_key": ARR_KEY}),
        )
        .await,
    )
    .await;
    assert_eq!(body["ok"], true, "{}", body);
    assert_eq!(body["kind"], "radarr");
    assert_eq!(body["action"], "create");
    assert_eq!(body["client_name"], "putio-sync");
    assert_eq!(body["category"], "radarr");
    assert_eq!(
        body["download_client"],
        json!({"will_enable": true, "will_generate_password": true})
    );
    // A preview changes nothing
    assert!(!app.state.arr.as_ref().unwrap().service.settings().enabled);

    let bad_key = body_json(
        app.post_json(
            "/arr/setup/preview",
            json!({"target": {"url": arr.url()}, "api_key": "wrong"}),
        )
        .await,
    )
    .await;
    assert_eq!(bad_key["ok"], false);

    let no_key = app
        .post_json("/arr/setup/preview", json!({"target": {"url": arr.url()}}))
        .await;
    assert_eq!(no_key.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn setup_turns_the_client_on_and_creates_it() {
    let mut arr = mockito::Server::new_async().await;
    mock_radarr(&mut arr).await;
    arr.mock("POST", "/api/v3/downloadclient/test")
        .create_async()
        .await;
    // The generated password is 64 hex characters
    let create = arr
        .mock("POST", "/api/v3/downloadclient")
        .match_body(Matcher::Regex(
            r#"\{"name":"password","value":"[0-9a-f]{64}"\}"#.into(),
        ))
        .with_status(201)
        .with_body(r#"{"id": 5, "name": "putio-sync"}"#)
        .expect(1)
        .create_async()
        .await;
    let app = test_app().await;
    let folder = app._dir.path().display().to_string();
    mock_folder(&mut arr, &folder, json!([]), true).await;
    let response = app
        .post_json(
            "/arr/setup",
            json!({"target": {"url": arr.url()}, "api_key": ARR_KEY}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let text = body_text(response).await;
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["outcome"], "created", "{}", body);
    assert_eq!(body["app"], "Radarr");
    assert_eq!(body["version"], "5.14.0.9383");
    assert_eq!(body["host"], "putio-sync");
    assert_eq!(body["remembered"], false);
    assert_eq!(body["path_check"]["status"], "visible", "{}", body);
    create.assert_async().await;

    let settings = app.state.arr.as_ref().unwrap().service.settings();
    assert!(settings.enabled);
    let password = settings.password.clone().unwrap();
    assert_eq!(password.len(), 64);
    assert!(!text.contains(&password) && !text.contains(ARR_KEY));
    let file = file_config(&app);
    assert!(file.arr.enabled);
    assert_eq!(file.arr.password.as_deref(), Some(password.as_str()));
    // Without "Remember", the application isn't saved
    assert!(file.arr.servers.is_empty());
}

#[tokio::test]
async fn remember_saves_the_application() {
    let mut arr = mockito::Server::new_async().await;
    mock_radarr(&mut arr).await;
    arr.mock("POST", "/api/v3/downloadclient/test")
        .create_async()
        .await;
    arr.mock("POST", "/api/v3/downloadclient")
        .with_body(r#"{"id": 5}"#)
        .create_async()
        .await;
    let url = arr.url();
    let app = test_app_with(|config| {
        config.arr.enabled = true;
        config.arr.password = Some("existing-pw".into());
        // The same address, saved with an old key: it's updated, not added
        config.arr.servers = vec![server(
            ArrKind::Radarr,
            format!("{}/", url),
            Some("old-key"),
            "radarr",
        )];
    })
    .await;
    let body = body_json(
        app.post_json(
            "/arr/setup",
            json!({"target": {"url": url}, "api_key": ARR_KEY, "remember": true}),
        )
        .await,
    )
    .await;
    assert_eq!(body["outcome"], "created", "{}", body);
    assert_eq!(body["remembered"], true);
    let file = file_config(&app);
    assert_eq!(file.arr.password.as_deref(), Some("existing-pw"));
    assert_eq!(file.arr.servers.len(), 1);
    assert_eq!(file.arr.servers[0].api_key.as_deref(), Some(ARR_KEY));
    let settings = app.state.arr.as_ref().unwrap().service.settings();
    assert_eq!(settings.servers[0].api_key.as_deref(), Some(ARR_KEY));
}

#[tokio::test]
async fn an_address_without_a_scheme_is_saved_as_the_url_that_answered() {
    let mut arr = mockito::Server::new_async().await;
    mock_radarr(&mut arr).await;
    arr.mock("POST", "/api/v3/downloadclient/test")
        .create_async()
        .await;
    arr.mock("POST", "/api/v3/downloadclient")
        .with_body(r#"{"id": 5}"#)
        .create_async()
        .await;
    let url = arr.url();
    let typed = url.trim_start_matches("http://").to_string();
    let app = test_app_with(|config| {
        config.arr.enabled = true;
        config.arr.password = Some("existing-pw".into());
    })
    .await;
    let body = body_json(
        app.post_json(
            "/arr/setup",
            json!({"target": {"url": typed}, "api_key": ARR_KEY, "remember": true}),
        )
        .await,
    )
    .await;
    assert_eq!(body["outcome"], "created", "{}", body);
    assert_eq!(body["url"], url.as_str());
    let file = file_config(&app);
    assert_eq!(file.arr.servers[0].url, url);
}

#[tokio::test]
async fn setup_failure_leaves_nothing_in_the_arr() {
    let mut arr = mockito::Server::new_async().await;
    mock_radarr(&mut arr).await;
    arr.mock("POST", "/api/v3/downloadclient/test")
        .with_status(400)
        .with_body(r#"[{"errorMessage":"Unable to connect to Transmission"}]"#)
        .create_async()
        .await;
    let create = arr
        .mock("POST", "/api/v3/downloadclient")
        .expect(0)
        .create_async()
        .await;
    let app = test_app().await;
    let body = body_json(
        app.post_json(
            "/arr/setup",
            json!({"target": {"url": arr.url()}, "api_key": ARR_KEY, "host": "10.9.9.9"}),
        )
        .await,
    )
    .await;
    assert_eq!(body["outcome"], "failed");
    assert_eq!(body["reason"], "arr_cannot_reach");
    assert_eq!(body["hosts_tried"], json!(["10.9.9.9"]));
    create.assert_async().await;
}

#[tokio::test]
async fn saved_application_test_results() {
    let mut arr = mockito::Server::new_async().await;
    let status = arr
        .mock("GET", Matcher::Regex("/api/v3/system/status$".into()))
        .match_header("x-api-key", "good")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"appName":"Sonarr","version":"4.0.15.2941"}"#)
        .create_async()
        .await;
    arr.mock("GET", Matcher::Regex("/api/v3/system/status$".into()))
        .match_header("x-api-key", "bad")
        .with_status(401)
        .create_async()
        .await;
    arr.mock("GET", "/api/v3/downloadclient")
        .match_header("x-api-key", "good")
        .with_body(putio_sync_client("tvCategory", "tv"))
        .create_async()
        .await;
    let url = arr.url();
    let app = test_app_with(|config| {
        config.arr.servers = vec![
            server(ArrKind::Sonarr, url.clone(), Some("good"), "sonarr"),
            server(ArrKind::Sonarr, format!("{}/b", url), Some("bad"), "tv"),
            server(
                ArrKind::Radarr,
                format!("{}/c", url),
                Some("good"),
                "radarr",
            ),
            server(ArrKind::Radarr, format!("{}/d", url), None, "movies"),
        ];
    })
    .await;

    let folder = app._dir.path().display().to_string();
    mock_folder(&mut arr, &folder, json!([]), true).await;

    let test = |i: usize| format!("/arr/applications/{i}/test");
    let ok = body_json(app.post_json(&test(0), json!({})).await).await;
    assert_eq!(
        ok,
        json!({"ok": true, "app": "Sonarr", "version": "4.0.15.2941",
               "categories": ["tv"], "client_present": true, "read_error": null,
               "path_check": {"status": "visible", "folder": folder, "arr_path": folder,
                              "mappings": [], "detail": null}})
    );
    status.assert_async().await;
    // The test refreshed what the Settings card shows; the legacy category stays visible
    let listed = body_json(app.get("/arr").await).await;
    let first = &listed["applications"][0];
    assert_eq!(first["categories"], json!(["tv"]));
    assert_eq!(first["client_present"], true);
    assert!(first["read_at"].is_string(), "{}", first);
    assert_eq!(first["legacy_category"], "sonarr");
    assert_eq!(first["path_check"]["status"], "visible");
    assert_eq!(listed["applications"][1]["read_at"], Value::Null);

    let bad = body_json(app.post_json(&test(1), json!({})).await).await;
    assert_eq!(bad["ok"], false);
    assert_eq!(bad["reason"], "unauthorized");

    let wrong_app = body_json(app.post_json(&test(2), json!({})).await).await;
    assert_eq!(wrong_app["reason"], "not_arr");
    assert_eq!(wrong_app["detail"], "not Radarr: found Sonarr");

    let no_key = body_json(app.post_json(&test(3), json!({})).await).await;
    assert_eq!(no_key["reason"], "no_api_key");
    assert!(no_key["detail"]
        .as_str()
        .unwrap()
        .contains("PUTIO_SYNC_RADARR_API_KEY"));

    assert_eq!(
        app.post_json(&test(9), json!({})).await.status(),
        StatusCode::NOT_FOUND
    );
    // Replaced by the route above
    assert_eq!(
        app.post_json("/arr/servers/0/test", json!({}))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

/// A list with one putio-sync client whose `field` is `category`
fn putio_sync_client(field: &str, category: &str) -> String {
    json!([{"name": "putio-sync", "implementation": "Transmission", "enable": true,
            "fields": [{"name": "host", "value": "putio-sync"}, {"name": "port", "value": 15432},
                       {"name": "urlBase", "value": "/transmission/"},
                       {"name": field, "value": category}]}])
    .to_string()
}

/// The *arr's mappings, and a listing of `folder`'s parent that has it or not
async fn mock_folder(arr: &mut mockito::Server, folder: &str, mappings: Value, listed: bool) {
    let (parent, name) = folder.rsplit_once('/').unwrap();
    let parent = if parent.is_empty() { "/" } else { parent };
    arr.mock("GET", "/api/v3/remotepathmapping")
        .with_body(mappings.to_string())
        .create_async()
        .await;
    let directories = if listed {
        json!([{"type": "folder", "name": name}])
    } else {
        json!([{"type": "folder", "name": "media"}])
    };
    arr.mock("GET", "/api/v3/filesystem")
        .match_query(Matcher::UrlEncoded("path".into(), parent.into()))
        .with_body(json!({"directories": directories, "files": []}).to_string())
        .create_async()
        .await;
}

#[tokio::test]
async fn a_folder_the_arr_cannot_see_is_reported_with_its_mappings() {
    let mut arr = mockito::Server::new_async().await;
    arr.mock("GET", "/api/v3/system/status")
        .with_body(r#"{"appName":"Radarr","version":"5.14.0.9383"}"#)
        .create_async()
        .await;
    arr.mock("GET", "/api/v3/downloadclient")
        .with_body(putio_sync_client("movieCategory", "radarr"))
        .create_async()
        .await;
    let app = test_app().await;
    let folder = app._dir.path().display().to_string();
    let mappings =
        json!([{"host": "putio-sync", "remotePath": "/elsewhere/", "localPath": "/data/"}]);
    mock_folder(&mut arr, &folder, mappings, false).await;
    let url = arr.url();
    let body = body_json(
        app.post_json(
            "/arr/applications/test",
            json!({"kind": "radarr", "host": "127.0.0.1", "port": port_of(&url),
                   "api_key": ARR_KEY}),
        )
        .await,
    )
    .await;
    let check = &body["path_check"];
    assert_eq!(check["status"], "not_visible", "{}", body);
    assert_eq!(check["arr_path"], folder);
    assert_eq!(
        check["mappings"],
        json!([{"host": "putio-sync", "remote_path": "/elsewhere/", "local_path": "/data/"}])
    );
}

#[tokio::test]
async fn no_putio_sync_client_means_no_path_check() {
    let mut arr = mockito::Server::new_async().await;
    // Its client list is empty
    mock_radarr(&mut arr).await;
    let never = arr
        .mock("GET", "/api/v3/remotepathmapping")
        .expect(0)
        .create_async()
        .await;
    let app = test_app().await;
    let url = arr.url();
    let body = body_json(
        app.post_json(
            "/arr/applications/test",
            json!({"kind": "radarr", "host": "127.0.0.1", "port": port_of(&url),
                   "api_key": ARR_KEY}),
        )
        .await,
    )
    .await;
    assert_eq!(body["client_present"], false);
    assert_eq!(body["path_check"], Value::Null);
    never.assert_async().await;
}

fn port_of(url: &str) -> u16 {
    url.rsplit(':').next().unwrap().parse().unwrap()
}

#[tokio::test]
async fn form_test_uses_the_saved_key_when_the_field_is_empty() {
    let mut arr = mockito::Server::new_async().await;
    mock_radarr(&mut arr).await;
    let url = arr.url();
    let app = test_app_with(|config| {
        config.arr.servers = vec![server(
            ArrKind::Radarr,
            url.clone(),
            Some(ARR_KEY),
            "radarr",
        )];
    })
    .await;
    let form = |extra: Value| {
        let mut body = json!({"kind": "radarr", "host": "127.0.0.1", "port": port_of(&url),
                              "ssl": false, "url_base": "", "api_key": ""});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        body
    };

    let ok = body_json(
        app.post_json("/arr/applications/test", form(json!({"index": 0})))
            .await,
    )
    .await;
    assert_eq!(ok["ok"], true, "{}", ok);
    assert_eq!(ok["app"], "Radarr");
    assert_eq!(ok["client_present"], false);
    // An unsaved test doesn't touch the saved application's categories
    assert_eq!(
        body_json(app.get("/arr").await).await["applications"][0]["read_at"],
        Value::Null
    );

    let typed = body_json(
        app.post_json("/arr/applications/test", form(json!({"api_key": ARR_KEY})))
            .await,
    )
    .await;
    assert_eq!(typed["ok"], true);

    let no_key = body_json(
        app.post_json("/arr/applications/test", form(json!({})))
            .await,
    )
    .await;
    assert_eq!(no_key["reason"], "no_api_key");

    let bad_host = app
        .post_json(
            "/arr/applications/test",
            form(json!({"host": "http://radarr"})),
        )
        .await;
    assert_eq!(bad_host.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn applications_are_added_edited_and_deleted() {
    let mut arr = mockito::Server::new_async().await;
    mock_radarr(&mut arr).await;
    let port = port_of(&arr.url());
    let app = test_app().await;
    let others = without_arr(&app);
    let body = |key: &str| {
        json!({"kind": "radarr", "name": "Movies", "host": "127.0.0.1", "port": port,
               "ssl": false, "url_base": "/", "api_key": key})
    };

    let added = app.post_json("/arr/applications", body(ARR_KEY)).await;
    assert_eq!(added.status(), StatusCode::CREATED);
    let text = body_text(added).await;
    assert!(!text.contains(ARR_KEY));
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["index"], 0);
    let saved = file_config(&app).arr.servers;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].url, format!("http://127.0.0.1:{}", port));
    assert_eq!(saved[0].name.as_deref(), Some("Movies"));
    assert_eq!(saved[0].api_key.as_deref(), Some(ARR_KEY));
    assert_eq!(saved[0].category, None);
    assert_eq!(without_arr(&app), others);
    // Applied at once, with the categories read on save
    let service = &app.state.arr.as_ref().unwrap().service;
    assert_eq!(service.settings().servers.len(), 1);
    let listed = body_json(app.get("/arr").await).await;
    assert!(listed["applications"][0]["read_at"].is_string());

    // A duplicate URL, a missing key, a bad host
    for (bad, why) in [
        (body(ARR_KEY), "duplicate"),
        (body(""), "no key"),
        (
            json!({"kind": "radarr", "host": "", "api_key": ARR_KEY}),
            "no host",
        ),
    ] {
        let response = app.post_json("/arr/applications", bad).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{}", why);
    }
    assert_eq!(file_config(&app).arr.servers.len(), 1);

    // An empty key keeps the saved one
    let mut edit = body("");
    edit["name"] = json!("Films");
    edit["ssl"] = json!(true);
    edit["url_base"] = json!("radarr");
    let response = app
        .req(
            Method::PUT,
            "/arr/applications/0",
            Some(&app.token),
            Some(edit.clone()),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let saved = &file_config(&app).arr.servers[0];
    assert_eq!(saved.url, format!("https://127.0.0.1:{}/radarr", port));
    assert_eq!(saved.name.as_deref(), Some("Films"));
    assert_eq!(saved.api_key.as_deref(), Some(ARR_KEY));
    let missing = app
        .req(
            Method::PUT,
            "/arr/applications/5",
            Some(&app.token),
            Some(edit),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let deleted = app
        .req(
            Method::DELETE,
            "/arr/applications/0",
            Some(&app.token),
            None,
        )
        .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    assert!(file_config(&app).arr.servers.is_empty());
    assert!(service.settings().servers.is_empty());
    let again = app
        .req(
            Method::DELETE,
            "/arr/applications/0",
            Some(&app.token),
            None,
        )
        .await;
    assert_eq!(again.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_an_application_leaves_the_arr_alone() {
    let mut arr = mockito::Server::new_async().await;
    let untouched = arr.mock("GET", Matcher::Any).expect(0).create_async().await;
    let any_write = arr
        .mock("DELETE", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let url = arr.url();
    let app = test_app_with(|config| {
        config.arr.servers = vec![server(
            ArrKind::Sonarr,
            url.clone(),
            Some(ARR_KEY),
            "sonarr",
        )];
    })
    .await;
    let response = app
        .req(
            Method::DELETE,
            "/arr/applications/0",
            Some(&app.token),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    untouched.assert_async().await;
    any_write.assert_async().await;
}

/// One setup that should fail with `reason`. Returns the result.
async fn failed_setup(app: &TestApp, req: Value, reason: &str) -> Value {
    let body = body_json(app.post_json("/arr/setup", req).await).await;
    assert_eq!(body["outcome"], "failed", "{}", body);
    assert_eq!(body["reason"], reason, "{}", body);
    let text = body.to_string();
    assert!(!text.contains(ARR_KEY), "{}", text);
    body
}

/// An *arr that must end with no new client: no create, no update, nothing left to delete
async fn no_writes(arr: &mut mockito::ServerGuard) -> Vec<mockito::Mock> {
    let mut mocks = Vec::new();
    for method in ["POST", "PUT", "DELETE"] {
        mocks.push(
            arr.mock(
                method,
                Matcher::Regex("^/api/v3/downloadclient(/\\d+)?$".into()),
            )
            .expect(0)
            .create_async()
            .await,
        );
    }
    mocks
}

async fn status_only(arr: &mut mockito::ServerGuard, status: usize, body: &str) {
    arr.mock("GET", "/api/v3/system/status")
        .with_status(status)
        .with_body(body)
        .create_async()
        .await;
}

#[tokio::test]
async fn each_setup_failure_has_its_own_reason_and_writes_nothing() {
    let app = test_app_with(|config| {
        config.arr.enabled = true;
        config.arr.password = Some("existing-pw".into());
    })
    .await;
    let req = |url: &str| json!({"target": {"url": url}, "api_key": ARR_KEY, "host": "10.9.9.9"});

    // unreachable, with the address tried
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let closed_url = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let body = failed_setup(&app, req(&closed_url), "unreachable").await;
    assert!(
        body["detail"].as_str().unwrap().contains(&closed_url),
        "{}",
        body
    );

    // unauthorized, not_arr, unsupported_version: refused at the status call
    for (status, answer, reason) in [
        (401, "", "unauthorized"),
        (200, r#"{"appName":"Lidarr","version":"2.5.0"}"#, "not_arr"),
        (
            200,
            r#"{"appName":"Radarr","version":"2.0.0.5344"}"#,
            "unsupported_version",
        ),
    ] {
        let mut arr = mockito::Server::new_async().await;
        status_only(&mut arr, status, answer).await;
        let writes = no_writes(&mut arr).await;
        failed_setup(&app, req(&arr.url()), reason).await;
        for m in writes {
            m.assert_async().await;
        }
    }

    // invalid_category: refused before any request
    let mut arr = mockito::Server::new_async().await;
    let any = arr.mock("GET", Matcher::Any).expect(0).create_async().await;
    let mut bad = req(&arr.url());
    bad["category"] = json!("a/../b");
    failed_setup(&app, bad, "invalid_category").await;
    any.assert_async().await;

    // arr_cannot_reach (with the hosts tried) and arr_test_failed (with the *arr's words)
    for (message, reason) in [
        (
            "Unable to connect to Transmission, please check your settings",
            "arr_cannot_reach",
        ),
        ("Authentication failure", "arr_test_failed"),
    ] {
        let mut arr = mockito::Server::new_async().await;
        mock_radarr(&mut arr).await;
        arr.mock("POST", "/api/v3/downloadclient/test")
            .with_status(400)
            .with_body(json!([{"errorMessage": message}]).to_string())
            .create_async()
            .await;
        let writes = no_writes(&mut arr).await;
        let body = failed_setup(&app, req(&arr.url()), reason).await;
        assert_eq!(body["hosts_tried"], json!(["10.9.9.9"]));
        assert!(
            body["detail"].as_str().unwrap().contains(message),
            "{}",
            body
        );
        for m in writes {
            m.assert_async().await;
        }
    }

    // arr_refused: the test passed, the save was refused; nothing was added to remove
    let mut arr = mockito::Server::new_async().await;
    mock_radarr(&mut arr).await;
    arr.mock("POST", "/api/v3/downloadclient/test")
        .create_async()
        .await;
    let create = arr
        .mock("POST", "/api/v3/downloadclient")
        .with_status(400)
        .with_body(r#"[{"errorMessage":"Category must be set"}]"#)
        .create_async()
        .await;
    let delete = arr
        .mock("DELETE", Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let body = failed_setup(&app, req(&arr.url()), "arr_refused").await;
    assert!(body["detail"]
        .as_str()
        .unwrap()
        .contains("Category must be set"));
    create.assert_async().await;
    delete.assert_async().await;
}

#[tokio::test]
async fn setup_reports_a_config_write_failure_before_touching_the_arr() {
    // The download client is off, so setup must turn it on first, and that write fails
    let app = test_app().await;
    std::fs::remove_file(config_path(&app)).unwrap();
    std::fs::create_dir(config_path(&app)).unwrap();
    let mut arr = mockito::Server::new_async().await;
    let any = arr.mock("GET", Matcher::Any).expect(0).create_async().await;
    let writes = no_writes(&mut arr).await;
    failed_setup(
        &app,
        json!({"target": {"url": arr.url()}, "api_key": ARR_KEY}),
        "config_write_failed",
    )
    .await;
    any.assert_async().await;
    for m in writes {
        m.assert_async().await;
    }
    assert!(!app.state.arr.as_ref().unwrap().service.settings().enabled);
}
