//! The bundled notifiers against a local server: what they send, grouping, and secrets
//! kept out of the runs.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mockito::Matcher;
use putio_sync_core::events::{self, EventType, Payload};
use putio_sync_core::plugins::PluginSettings;
use putio_sync_plugins::discover::{discover, Source};
use putio_sync_plugins::{PluginState, PluginsOptions, PluginsService, RunResult};
use serde_json::json;

/// The bus is global, so these tests take turns
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const TOKEN: &str = "tk_ntfy_secret_value";

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
    let plugins = PluginsService::new(PluginsOptions {
        clock: clock.options(),
        ..PluginsOptions::default()
    });
    let checked = plugins.check(&settings).unwrap();
    plugins.configure(checked);
    plugins
}

fn plugin(name: &str, config: serde_json::Value, secrets: &[(&str, &str)]) -> PluginSettings {
    PluginSettings {
        name: name.to_string(),
        enabled: true,
        config: serde_json::from_value(config).unwrap(),
        secrets: secrets
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        ..PluginSettings::default()
    }
}

fn finished(name: &str) -> Payload {
    let mut p = events::samples()
        .into_iter()
        .find(|p| p.event_type() == EventType::DownloadFinished)
        .unwrap();
    if let Payload::DownloadFinished(d) = &mut p {
        d.name = name.to_string();
        d.size_bytes = 1_400_000_000;
    }
    p
}

async fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < until, "timed out waiting for {}", what);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
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

#[test]
fn every_bundled_plugin_loads() {
    let found = discover(None);
    let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["discord", "emby", "jellyfin", "ntfy", "plex", "pushover", "telegram"]
    );
    for f in &found {
        assert_eq!(f.source, Source::Bundled);
        let m = f
            .loadable()
            .unwrap_or_else(|| panic!("{}: {:?}", f.name, f.error));
        let media = ["emby", "jellyfin", "plex"].contains(&f.name.as_str());
        let shared = if media { "media.new" } else { "notifier.new" };
        assert!(f.script.contains(shared), "{}", f.name);
        assert!(m.permissions.http.any());
        assert_eq!(m.permissions.store, media, "{}", f.name);
    }
    let disabled = PluginsService::new(PluginsOptions::default()).status();
    assert!(disabled.iter().all(|p| p.state == PluginState::Disabled));
    let ntfy = disabled.iter().find(|p| p.name == "ntfy").unwrap();
    assert_eq!(ntfy.hosts_needed, vec!["ntfy.sh"]);
    let plex = disabled.iter().find(|p| p.name == "plex").unwrap();
    assert_eq!(plex.hosts_needed, vec!["plex.tv"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ntfy_sends_the_first_event_and_a_summary_after_the_window() {
    let _turn = SERIAL.lock().await;
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("POST", "/")
        .match_header("authorization", format!("Bearer {}", TOKEN).as_str())
        .match_body(Matcher::PartialJson(json!({
            "topic": "nas", "title": "Downloaded", "message": "File 0 (1.4 GB)", "priority": 4
        })))
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    let summary = server
        .mock("POST", "/")
        .match_body(Matcher::PartialJson(json!({
            "title": "9 more downloads finished",
            "message": "File 1, File 2, File 3, File 4, File 5 and 4 more"
        })))
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    let clock = Clock::new(1_793_784_600.0);
    let plugins = service(
        vec![plugin(
            "ntfy",
            json!({"server": server.url(), "topic": "nas", "priority": 4}),
            &[("token", TOKEN)],
        )],
        &clock,
    );
    plugins.start();

    for i in 0..10 {
        events::emit(finished(&format!("File {}", i)));
        clock.advance(10.0);
    }
    wait_for("ten runs", || {
        runs_of(&plugins, "ntfy", "download.finished") == 10
    })
    .await;
    first.assert_async().await;

    // A tick inside the window sends nothing; one after it sends the summary.
    plugins.tick();
    clock.advance(300.0);
    plugins.tick();
    let mut done = false;
    for _ in 0..500 {
        if summary.matched_async().await {
            done = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(done, "no summary sent");
    first.assert_async().await;

    let status = serde_json::to_string(&plugins.status()).unwrap();
    assert!(!status.contains(TOKEN));
    let ntfy = plugins
        .status()
        .into_iter()
        .find(|p| p.name == "ntfy")
        .unwrap();
    assert!(ntfy.has_tick);
    assert!(ntfy.runs.iter().all(|r| r.result == RunResult::Success));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_send_is_a_failed_run_without_the_secret() {
    let _turn = SERIAL.lock().await;
    let mut server = mockito::Server::new_async().await;
    let url = format!("{}/api/webhooks/1/{}", server.url(), TOKEN);
    server
        .mock("POST", Matcher::Any)
        .with_status(401)
        .create_async()
        .await;
    let clock = Clock::new(0.0);
    let plugins = service(
        vec![plugin(
            "discord",
            json!({}),
            &[("webhook_url", url.as_str())],
        )],
        &clock,
    );
    let run = plugins.test("discord").await.unwrap();
    assert_eq!(run.result, RunResult::Failure);
    assert_eq!(run.error.as_deref(), Some("127.0.0.1: HTTP 401"));
    assert!(!serde_json::to_string(&run).unwrap().contains(TOKEN));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discord_telegram_and_pushover_send_their_bodies() {
    let _turn = SERIAL.lock().await;
    let mut server = mockito::Server::new_async().await;
    let discord = server
        .mock("POST", "/api/webhooks/1/abcd")
        .match_body(Matcher::Json(json!({
            "username": "nas", "content": "**putio-sync test**\nTest event from putio-sync"
        })))
        .with_status(204)
        .create_async()
        .await;
    let telegram = server
        .mock("POST", "/bot123:ABCDEF/sendMessage")
        .match_body(Matcher::Json(json!({
            "chat_id": "-100", "text": "putio-sync test\nTest event from putio-sync"
        })))
        .with_status(200)
        .create_async()
        .await;
    let pushover = server
        .mock("POST", "/1/messages.json")
        .match_header("content-type", "application/x-www-form-urlencoded")
        .match_body(Matcher::AllOf(vec![
            Matcher::UrlEncoded("token".into(), "apptoken1".into()),
            Matcher::UrlEncoded("user".into(), "userkey1".into()),
            Matcher::UrlEncoded("title".into(), "putio-sync test".into()),
            Matcher::UrlEncoded("message".into(), "Test event from putio-sync".into()),
            Matcher::UrlEncoded("priority".into(), "0".into()),
        ]))
        .with_status(200)
        .create_async()
        .await;
    let clock = Clock::new(0.0);
    let base = server.url();
    let plugins = service(
        vec![
            plugin(
                "discord",
                json!({"username": "nas"}),
                &[("webhook_url", &format!("{}/api/webhooks/1/abcd", base))],
            ),
            plugin(
                "telegram",
                json!({"api_url": base, "chat_id": "-100"}),
                &[("bot_token", "123:ABCDEF")],
            ),
            plugin(
                "pushover",
                json!({"api_url": base}),
                &[("app_token", "apptoken1"), ("user_key", "userkey1")],
            ),
        ],
        &clock,
    );
    for name in ["discord", "telegram", "pushover"] {
        let run = plugins.test(name).await.unwrap();
        assert_eq!(run.result, RunResult::Success, "{}: {:?}", name, run.error);
    }
    discord.assert_async().await;
    telegram.assert_async().await;
    pushover.assert_async().await;
}
