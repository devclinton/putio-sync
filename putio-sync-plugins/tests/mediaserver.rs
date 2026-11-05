//! The bundled Plex, Jellyfin and Emby plugins against mock servers: grouping per folder,
//! the longest wait, flushing on a completed sync, retry then drop, folders outside the
//! libraries, path rules, the Plex sign-in, the pickers, health, and tokens kept out of
//! everything shown.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mockito::{Matcher, Mock, ServerGuard};
use putio_sync_core::events::{self, EventType, Payload};
use putio_sync_core::plugins::PluginSettings;
use putio_sync_plugins::action::HealthStatus;
use putio_sync_plugins::{PluginsOptions, PluginsService};
use serde_json::{json, Map, Value};

/// The bus is global, so these tests take turns
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const TOKEN: &str = "plexTOKENsecret123";
const KEY: &str = "jellyKEYsecret456";

struct Clock(Arc<AtomicU64>);

impl Clock {
    fn new(at: f64) -> Clock {
        Clock(Arc::new(AtomicU64::new(at.to_bits())))
    }
    fn advance(&self, secs: f64) {
        let now = f64::from_bits(self.0.load(Ordering::SeqCst));
        self.0.store((now + secs).to_bits(), Ordering::SeqCst);
    }
    fn options(&self) -> Option<putio_sync_plugins::host::Clock> {
        let at = self.0.clone();
        Some(Arc::new(move || f64::from_bits(at.load(Ordering::SeqCst))))
    }
}

fn service(settings: Vec<PluginSettings>, clock: &Clock) -> PluginsService {
    let data = tempfile::tempdir().unwrap().keep();
    let plugins = PluginsService::new(PluginsOptions {
        clock: clock.options(),
        data_dir: Some(data),
        download_dir: Some("/downloads".into()),
        ..PluginsOptions::default()
    });
    let checked = plugins.check(&settings).unwrap();
    plugins.configure(checked);
    plugins
}

fn plugin(name: &str, enabled: bool, config: Value, secrets: &[(&str, &str)]) -> PluginSettings {
    PluginSettings {
        name: name.to_string(),
        enabled,
        config: serde_json::from_value(config).unwrap(),
        secrets: secrets
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        ..PluginSettings::default()
    }
}

fn plex(server: &ServerGuard, extra: Value) -> PluginSettings {
    let mut config = json!({
        "server": server.url(),
        "plex_tv_url": server.url(),
        "path_map": [{"from": "/downloads", "to": "/data"}],
    });
    for (k, v) in extra.as_object().unwrap() {
        config[k] = v.clone();
    }
    plugin("plex", true, config, &[("token", TOKEN)])
}

fn finished(path: &str) -> Payload {
    let mut p = events::samples()
        .into_iter()
        .find(|p| p.event_type() == EventType::DownloadFinished)
        .unwrap();
    if let Payload::DownloadFinished(d) = &mut p {
        d.local_path = path.to_string();
        d.name = path.rsplit('/').next().unwrap().to_string();
    }
    p
}

fn sync_completed() -> Payload {
    events::samples()
        .into_iter()
        .find(|p| p.event_type() == EventType::SyncCompleted)
        .unwrap()
}

fn runs_of(plugins: &PluginsService, name: &str, event: &str) -> usize {
    plugins
        .status()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap()
        .runs
        .iter()
        .filter(|r| r.event_type == event)
        .count()
}

async fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {}", what);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_matched(what: &str, mock: &Mock) {
    let until = Instant::now() + Duration::from_secs(10);
    while !mock.matched_async().await {
        assert!(Instant::now() < until, "timed out waiting for {}", what);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Wait until the plugin's queue is empty (ticks are not kept as runs)
async fn settle(plugins: &PluginsService, name: &str) {
    wait_for("an empty queue", || {
        plugins
            .status()
            .into_iter()
            .find(|p| p.name == name)
            .is_some_and(|p| p.queued == 0)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(150)).await;
}

async fn sections(server: &mut ServerGuard) -> Mock {
    server
        .mock("GET", "/library/sections")
        .match_header("x-plex-token", TOKEN)
        .with_status(200)
        .with_body(
            json!({"MediaContainer": {"Directory": [
                {"key": "1", "title": "TV Shows", "Location": [{"id": 1, "path": "/data/tv"}]},
                {"key": "2", "title": "Movies", "Location": [{"id": 2, "path": "/data/movies"}]},
            ]}})
            .to_string(),
        )
        .create_async()
        .await
}

fn counter(name: &str, plugin: &str) -> f64 {
    let text = putio_sync_telemetry::global().encode();
    let line = format!(
        "putio_sync_plugin_counter_total{{name=\"{}\",plugin=\"{}\"}} ",
        name, plugin
    );
    text.lines()
        .find_map(|l| l.strip_prefix(line.as_str()))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plex_groups_files_per_folder_and_skips_folders_outside_libraries() {
    let _turn = SERIAL.lock().await;
    let mut server = mockito::Server::new_async().await;
    let _sections = sections(&mut server).await;
    let refresh = server
        .mock("GET", "/library/sections/1/refresh")
        .match_query(Matcher::UrlEncoded(
            "path".into(),
            "/data/tv/Show/Season 01".into(),
        ))
        .match_header("x-plex-token", TOKEN)
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    let leaked = server
        .mock("GET", Matcher::Regex(TOKEN.to_string()))
        .expect(0)
        .create_async()
        .await;
    let clock = Clock::new(1_793_784_600.0);
    let skipped = counter("skipped", "plex");
    let grouped = counter("folders_grouped", "plex");
    let plugins = service(vec![plex(&server, json!({}))], &clock);
    plugins.start();

    // 20 files, then one outside any library; history keeps the last 20 runs
    for i in 0..20 {
        events::emit(finished(&format!("/downloads/tv/Show/Season 01/e{:02}.mkv", i)));
        clock.advance(0.5);
    }
    events::emit(finished("/downloads/other/x.mkv"));
    settle(&plugins, "plex").await;

    // Files are still landing: nothing yet
    plugins.tick();
    settle(&plugins, "plex").await;
    assert!(!refresh.matched_async().await);

    clock.advance(21.0);
    plugins.tick();
    wait_matched("the folder refresh", &refresh).await;
    settle(&plugins, "plex").await;
    // Sent once: a later tick finds nothing waiting
    clock.advance(30.0);
    plugins.tick();
    settle(&plugins, "plex").await;
    refresh.assert_async().await;
    leaked.assert_async().await;
    assert_eq!(counter("skipped", "plex") - skipped, 1.0);
    assert_eq!(counter("folders_grouped", "plex") - grouped, 19.0);
    assert!(!serde_json::to_string(&plugins.status()).unwrap().contains(TOKEN));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plex_refreshes_after_the_longest_wait_and_on_a_completed_sync() {
    let _turn = SERIAL.lock().await;
    let mut server = mockito::Server::new_async().await;
    let _sections = sections(&mut server).await;
    let busy = server
        .mock("GET", "/library/sections/1/refresh")
        .match_query(Matcher::UrlEncoded("path".into(), "/data/tv/Busy".into()))
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    let movie = server
        .mock("GET", "/library/sections/2/refresh")
        .match_query(Matcher::UrlEncoded("path".into(), "/data/movies/Film (2026)".into()))
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    let clock = Clock::new(1_793_784_600.0);
    let plugins = service(
        vec![plex(&server, json!({"delay_seconds": 20, "max_wait_seconds": 60}))],
        &clock,
    );
    plugins.start();

    // A file every 15 s keeps the delay from passing; the longest wait still does
    for i in 0..4 {
        events::emit(finished(&format!("/downloads/tv/Busy/e{}.mkv", i)));
        wait_for("the event", || runs_of(&plugins, "plex", "download.finished") == i + 1).await;
        plugins.tick();
        settle(&plugins, "plex").await;
        assert!(!busy.matched_async().await, "refreshed early at file {}", i);
        clock.advance(15.0);
    }
    clock.advance(1.0);
    plugins.tick();
    wait_matched("the longest wait", &busy).await;

    // A completed sync sends what waits without waiting for the delay
    events::emit(finished("/downloads/movies/Film (2026)/film.mkv"));
    events::emit(sync_completed());
    wait_matched("the flush on sync.completed", &movie).await;
    busy.assert_async().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_refresh_is_tried_once_more_then_dropped() {
    let _turn = SERIAL.lock().await;
    let mut server = mockito::Server::new_async().await;
    let _sections = sections(&mut server).await;
    let refresh = server
        .mock("GET", "/library/sections/1/refresh")
        .match_query(Matcher::Any)
        .with_status(500)
        .expect(2)
        .create_async()
        .await;
    let clock = Clock::new(1_793_784_600.0);
    let failures = counter("refresh_failures", "plex");
    let plugins = service(vec![plex(&server, json!({}))], &clock);
    plugins.start();
    events::emit(finished("/downloads/tv/Show/e1.mkv"));
    wait_for("the event", || runs_of(&plugins, "plex", "download.finished") == 1).await;
    for _ in 0..3 {
        clock.advance(21.0);
        plugins.tick();
        settle(&plugins, "plex").await;
    }
    refresh.assert_async().await;
    assert_eq!(counter("refresh_failures", "plex") - failures, 1.0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plex_sign_in_saves_the_token_and_shows_it_nowhere() {
    let _turn = SERIAL.lock().await;
    let mut server = mockito::Server::new_async().await;
    let client = Matcher::Regex("^putio-sync-[0-9a-f]{32}$".to_string());
    let pin = server
        .mock("POST", "/api/v2/pins")
        .match_header("x-plex-product", "putio-sync")
        .match_header("x-plex-client-identifier", client.clone())
        .match_header("accept", "application/json")
        .with_status(201)
        .with_body(json!({"id": 42, "code": "AB12"}).to_string())
        .expect(1)
        .create_async()
        .await;
    let waiting = server
        .mock("GET", "/api/v2/pins/42")
        .match_header("x-plex-client-identifier", client.clone())
        .with_status(200)
        .with_body(json!({"id": 42, "code": "AB12", "authToken": null}).to_string())
        .create_async()
        .await;
    let clock = Clock::new(1_793_784_600.0);
    let settings = plugin(
        "plex",
        false,
        json!({"plex_tv_url": server.url()}),
        &[],
    );
    let plugins = service(vec![settings], &clock);

    let first = plugins
        .action("plex", "sign_in", json!({}), Map::new())
        .await
        .unwrap();
    pin.assert_async().await;
    assert_eq!(first.result.code.as_deref(), Some("AB12"));
    let link = first.result.open_url.clone().unwrap();
    assert!(link.starts_with("https://app.plex.tv/auth#?clientID=putio-sync-"), "{}", link);
    assert!(link.contains("&code=AB12"), "{}", link);
    let next = first.result.next.clone().unwrap();
    assert_eq!(next.action, "sign_in_poll");

    let again = plugins
        .action("plex", &next.action, next.input.clone(), Map::new())
        .await
        .unwrap();
    assert!(!again.result.done);
    assert!(again.settings.is_none());
    waiting.remove_async().await;

    let _approved = server
        .mock("GET", "/api/v2/pins/42")
        .match_header("x-plex-client-identifier", client.clone())
        .with_status(200)
        .with_body(json!({"id": 42, "code": "AB12", "authToken": TOKEN}).to_string())
        .create_async()
        .await;
    let user = server
        .mock("GET", "/api/v2/user")
        .match_header("x-plex-token", TOKEN)
        .with_status(200)
        .with_body(json!({"username": "clint"}).to_string())
        .expect(1)
        .create_async()
        .await;
    let done = plugins
        .action("plex", "sign_in_poll", again.result.next.unwrap().input, Map::new())
        .await
        .unwrap();
    user.assert_async().await;
    assert!(done.result.done);
    assert_eq!(done.result.message.as_deref(), Some("Signed in as clint"));
    assert_eq!(done.settings.unwrap().secrets["token"], TOKEN);
    let shown = format!("{:?} {:?}", serde_json::to_string(&done.result), done.run);
    assert!(!shown.contains(TOKEN), "{}", shown);

    // An expired code ends the flow
    clock.advance(601.0);
    let expired_input = json!({"id": 43, "started": 1_793_784_600.0});
    let _old = server
        .mock("GET", "/api/v2/pins/43")
        .with_status(200)
        .with_body(json!({"id": 43, "authToken": null}).to_string())
        .create_async()
        .await;
    let expired = plugins
        .action("plex", "sign_in_poll", expired_input, Map::new())
        .await
        .unwrap();
    assert!(expired.result.done);
    assert!(expired.result.message.unwrap().contains("expired"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plex_lists_servers_and_libraries_to_pick_from() {
    let _turn = SERIAL.lock().await;
    let mut server = mockito::Server::new_async().await;
    let _sections = sections(&mut server).await;
    let _resources = server
        .mock("GET", "/api/v2/resources")
        .match_query(Matcher::UrlEncoded("includeHttps".into(), "1".into()))
        .match_header("x-plex-token", TOKEN)
        .with_status(200)
        .with_body(
            json!([
                {"name": "NAS", "provides": "server", "connections": [
                    {"uri": "https://192-168-1-10.abc.plex.direct:32400", "local": true, "relay": false},
                    {"uri": "https://1-2-3-4.abc.plex.direct:32400", "local": false, "relay": false},
                    {"uri": "https://relay.plex.direct", "local": false, "relay": true},
                ]},
                {"name": "Phone", "provides": "client,player", "connections": []},
            ])
            .to_string(),
        )
        .create_async()
        .await;
    let clock = Clock::new(1_793_784_600.0);
    let mut settings = plex(&server, json!({}));
    settings.enabled = false;
    let plugins = service(vec![settings], &clock);

    let servers = plugins
        .action("plex", "load_servers", json!({}), Map::new())
        .await
        .unwrap();
    let choices = servers.result.choices.unwrap();
    assert_eq!(choices.len(), 2);
    assert_eq!(choices[0].value, "https://192-168-1-10.abc.plex.direct:32400");
    assert_eq!(
        choices[0].label,
        "NAS (local) https://192-168-1-10.abc.plex.direct:32400"
    );

    let libraries = plugins
        .action("plex", "load_libraries", json!({}), Map::new())
        .await
        .unwrap();
    let labels: Vec<(String, String)> = libraries
        .result
        .choices
        .unwrap()
        .into_iter()
        .map(|c| (c.value, c.label))
        .collect();
    assert_eq!(
        labels,
        vec![
            ("1".to_string(), "TV Shows (/data/tv)".to_string()),
            ("2".to_string(), "Movies (/data/movies)".to_string()),
        ]
    );

    // An unsaved server typed in the form is used for this call
    let mut other = mockito::Server::new_async().await;
    let elsewhere = other
        .mock("GET", "/library/sections")
        .with_status(200)
        .with_body(json!({"MediaContainer": {"Directory": []}}).to_string())
        .expect(1)
        .create_async()
        .await;
    let mut form = Map::new();
    form.insert("server".to_string(), json!(other.url()));
    let none = plugins
        .action("plex", "load_libraries", json!({}), form)
        .await
        .unwrap();
    elsewhere.assert_async().await;
    assert_eq!(none.result.choices.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plex_health_and_test_event() {
    let _turn = SERIAL.lock().await;
    let mut server = mockito::Server::new_async().await;
    let _identity = server
        .mock("GET", "/identity")
        .with_status(200)
        .with_body("{}")
        .create_async()
        .await;
    let _sections = sections(&mut server).await;
    let clock = Clock::new(1_793_784_600.0);
    let plugins = service(vec![plex(&server, json!({"libraries": ["1"]}))], &clock);
    let items = plugins.health("plex").await.unwrap().unwrap();
    let got: Vec<(HealthStatus, &str)> = items
        .iter()
        .map(|i| (i.status, i.message.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (HealthStatus::Ok, "Server reachable"),
            (HealthStatus::Ok, "Token accepted"),
            (
                HealthStatus::Ok,
                "Downloads (/data on the server) reach TV Shows"
            ),
        ]
    );

    // No path rule: /downloads is in no library
    let plugins = service(
        vec![plex(&server, json!({"path_map": []}))],
        &clock,
    );
    let items = plugins.health("plex").await.unwrap().unwrap();
    assert_eq!(items[2].status, HealthStatus::Warn);
    assert!(items[2].message.contains("/downloads"), "{}", items[2].message);

    // A refused token fails, and the test event fails with it
    let mut refused = mockito::Server::new_async().await;
    let _identity = refused
        .mock("GET", "/identity")
        .with_status(200)
        .create_async()
        .await;
    let _sections = refused
        .mock("GET", "/library/sections")
        .with_status(401)
        .create_async()
        .await;
    let plugins = service(vec![plex(&refused, json!({}))], &clock);
    let items = plugins.health("plex").await.unwrap().unwrap();
    assert_eq!(items[1].status, HealthStatus::Fail);
    assert_eq!(items[1].message, "Token refused: HTTP 401");
    let run = plugins.test("plex").await.unwrap();
    assert!(run.error.unwrap().contains("Token refused"));
    assert!(run.logs.iter().any(|l| l.message == "Server reachable"));
    assert!(!format!("{:?}", run.logs).contains(TOKEN));

    // Nothing listens
    let plugins = service(
        vec![plugin(
            "plex",
            true,
            json!({"server": "http://127.0.0.1:9", "plex_tv_url": server.url()}),
            &[("token", TOKEN)],
        )],
        &clock,
    );
    let items = plugins.health("plex").await.unwrap().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].status, HealthStatus::Fail);
    assert!(items[0].message.starts_with("Server not reachable"), "{}", items[0].message);
}

async fn embyfin(name: &str, prefix: &str, auth: (&str, Matcher)) {
    let mut server = mockito::Server::new_async().await;
    let _folders = server
        .mock("GET", format!("{}/Library/VirtualFolders", prefix).as_str())
        .match_header(auth.0, auth.1.clone())
        .with_status(200)
        .with_body(
            json!([
                {"Name": "Shows", "ItemId": "a", "Locations": ["/media/tv"]},
                {"Name": "Films", "ItemId": "b", "Locations": ["/media/films"]},
            ])
            .to_string(),
        )
        .create_async()
        .await;
    let updated = server
        .mock("POST", format!("{}/Library/Media/Updated", prefix).as_str())
        .match_header(auth.0, auth.1.clone())
        .match_body(Matcher::Json(json!({"Updates": [
            {"Path": "/media/tv/Show/S01", "UpdateType": "Created"},
            {"Path": "/media/tv/Show/S02", "UpdateType": "Created"},
        ]})))
        .with_status(204)
        .expect(1)
        .create_async()
        .await;
    let _public = server
        .mock("GET", format!("{}/System/Info/Public", prefix).as_str())
        .with_status(200)
        .create_async()
        .await;
    let _info = server
        .mock("GET", format!("{}/System/Info", prefix).as_str())
        .match_header(auth.0, auth.1.clone())
        .with_status(200)
        .create_async()
        .await;
    let clock = Clock::new(1_793_784_600.0);
    let settings = plugin(
        name,
        true,
        json!({
            "server": server.url(),
            "libraries": ["Shows"],
            "path_map": [{"from": "/downloads", "to": "/media"}],
        }),
        &[("api_key", KEY)],
    );
    let plugins = service(vec![settings], &clock);
    plugins.start();
    events::emit(finished("/downloads/tv/Show/S01/e1.mkv"));
    events::emit(finished("/downloads/tv/Show/S02/e1.mkv"));
    events::emit(finished("/downloads/films/Film/film.mkv"));
    wait_for("the events", || runs_of(&plugins, name, "download.finished") == 3).await;
    clock.advance(21.0);
    plugins.tick();
    wait_matched("the refresh", &updated).await;

    let libraries = plugins
        .action(name, "load_libraries", json!({}), Map::new())
        .await
        .unwrap();
    let values: Vec<String> = libraries
        .result
        .choices
        .unwrap()
        .into_iter()
        .map(|c| c.value)
        .collect();
    assert_eq!(values, vec!["Shows", "Films"]);
    let items = plugins.health(name).await.unwrap().unwrap();
    assert!(
        items.iter().all(|i| i.status == HealthStatus::Ok),
        "{:?}",
        items
    );
    updated.assert_async().await;
    assert!(!serde_json::to_string(&plugins.status()).unwrap().contains(KEY));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jellyfin_sends_due_folders_in_one_request() {
    let _turn = SERIAL.lock().await;
    embyfin(
        "jellyfin",
        "",
        (
            "authorization",
            Matcher::Exact(format!(
                "MediaBrowser Client=\"putio-sync\", Device=\"putio-sync\", DeviceId=\"putio-sync\", Version=\"1\", Token=\"{}\"",
                KEY
            )),
        ),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn emby_sends_due_folders_in_one_request() {
    let _turn = SERIAL.lock().await;
    embyfin("emby", "/emby", ("x-emby-token", Matcher::Exact(KEY.to_string()))).await;
}
