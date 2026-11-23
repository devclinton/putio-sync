//! Anonymous usage stats (feature 058, part 2): what the payload holds, and when anything is sent

use chrono::{DateTime, Duration as Span, Utc};
use putio_sync_core::config::{ArrKind, ArrServer, AuthConfig, Config};
use putio_sync_core::hooks::{HookConfig, HookStage};
use putio_sync_core::net_guard::Reach;
use putio_sync_core::plugins::PluginSettings;
use putio_sync_core::usage_stats::{self as stats, Inputs, Schedule, SendError};
use std::time::Duration;

fn hook(name: &str, stage: Option<HookStage>) -> HookConfig {
    toml::from_str::<HookConfig>(&format!(
        "name = \"{name}\"\nurl = \"https://hooks.bobs-nas.lan/secret-path\"\nevents = [\"*\"]\n{}",
        stage.map_or(String::new(), |s| format!("stage = \"{}\"\n", s.as_str()))
    ))
    .unwrap()
}

fn plugin(name: &str, enabled: bool) -> PluginSettings {
    PluginSettings {
        name: name.into(),
        enabled,
        approved_hosts: vec!["nas.bobs-house.lan".into()],
        approved_paths: vec!["r:/home/bob/private".into()],
        ..PluginSettings::default()
    }
}

/// A config full of values that must never leave the machine
#[allow(clippy::field_reassign_with_default)]
fn sensitive() -> Config {
    let mut c = Config::default();
    c.auth = Some(AuthConfig::with_token("TOKEN-SECRET-123".into()));
    c.api.auth_token = Some("API-TOKEN-SECRET".into());
    c.download.target_directory = "/home/bob/Downloads/secret-movies".into();
    c.sync.local_path = "/mnt/bobs-nas/sync".into();
    c.watch.enabled = true;
    c.watch.paths = vec!["/mnt/bobs-nas/watch-folder".into()];
    c.dlna.enabled = true;
    c.dlna.name = "Bob's Living Room".into();
    c.tailscale.allowed_users = vec!["bob@example.com".into()];
    c.tailscale.identity_sign_in = true;
    c.arr.enabled = true;
    c.arr.servers = vec![ArrServer {
        kind: ArrKind::Sonarr,
        name: Some("Bobs Sonarr".into()),
        url: "http://sonarr.bobs-house.lan:8989".into(),
        api_key: Some("ARR-KEY-SECRET".into()),
        category: None,
    }];
    c.hooks = vec![
        hook("bobs-private-hook", None),
        hook("bobs-stage-hook", Some(HookStage::PostProcessing)),
    ];
    c.plugins = vec![
        plugin("bobs-private-plugin", true),
        plugin("ntfy", true),
        plugin("plex", false),
    ];
    c.usage_stats.enabled = true;
    c.usage_stats.endpoint = "https://stats.example/ingest".into();
    c
}

fn inputs() -> Inputs {
    Inputs {
        build: "snap".into(),
        downloads_7d: 30,
        library_bytes: 2_000_000_000_000,
        crash_reports: true,
    }
}

#[test]
fn the_payload_has_flags_and_buckets_and_nothing_personal() {
    let id = "11111111-2222-4333-8444-555555555555";
    let p = stats::payload(&sensitive(), &inputs(), id);
    let json = serde_json::to_string_pretty(&p).unwrap();

    for secret in [
        "bob",
        "Bob",
        "secret",
        "SECRET",
        "TOKEN",
        "/home",
        "/mnt",
        ".lan",
        "example.com",
        "stats.example",
        "8989",
        "hooks.",
        "Living Room",
    ] {
        assert!(!json.contains(secret), "{secret:?} in {json}");
    }

    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["schema"], 1);
    assert_eq!(v["install_id"], id);
    assert_eq!(v["build"], "snap");
    assert!(
        ["linux", "macos", "windows", "freebsd", "netbsd", "openbsd", "other"]
            .contains(&v["os"].as_str().unwrap())
    );
    assert!(v["arch"].is_string());
    let f = &v["features"];
    assert_eq!(f["arr"], true);
    assert_eq!(f["watch_folders"], true);
    assert_eq!(f["dlna"], true);
    assert_eq!(f["tailscale"], true);
    assert_eq!(f["metrics"], sensitive().metrics.enabled);
    assert_eq!(f["subtitles"], false);
    assert_eq!(f["crash_reports"], true);
    assert_eq!(f["event_hooks"], true);
    assert_eq!(f["pipeline_stages"]["post_processing"], true);
    assert_eq!(f["pipeline_stages"]["pre_download"], false);
    // Only the bundled plugin that is on, by name
    assert_eq!(f["plugins"], serde_json::json!(["ntfy"]));
    assert_eq!(v["sizes"]["files_per_day"], "1-10");
    assert_eq!(v["sizes"]["library"], "1-10TB");
    // No keys beyond these
    let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "arch",
            "build",
            "features",
            "install_id",
            "os",
            "schema",
            "sizes",
            "version"
        ]
    );
}

fn at(h: i64) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-03-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
        + Span::hours(h)
}

fn config_for(endpoint: &str, enabled: bool) -> Config {
    let mut c = sensitive();
    c.usage_stats.enabled = enabled;
    c.usage_stats.endpoint = endpoint.into();
    c
}

#[tokio::test]
async fn off_or_without_an_endpoint_sends_nothing() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(stats::FILE_NAME);

    let off = config_for(&server.url(), false);
    assert_eq!(
        stats::send_now(&file, &off, &inputs(), at(0), Reach::Local).await,
        Err(SendError::NotActive)
    );
    let no_endpoint = config_for("", true);
    assert_eq!(
        stats::send_now(&file, &no_endpoint, &inputs(), at(0), Reach::Local).await,
        Err(SendError::NotActive)
    );
    // The schedule never asks for a send either, however long it waits
    let mut s = Schedule::new(Duration::ZERO);
    for cfg in [&off, &no_endpoint] {
        for h in [0, 24, 1000] {
            assert!(!s.due(&cfg.usage_stats, None, at(h)));
        }
    }
    mock.assert_async().await;
    assert!(!file.exists(), "nothing is written while off");
}

#[tokio::test]
async fn a_send_posts_the_payload_once_and_records_the_time() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/ingest")
        .match_header("content-type", "application/json")
        .match_header(
            "user-agent",
            concat!("putio-sync/", env!("CARGO_PKG_VERSION")),
        )
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "schema": 1,
            "build": "snap",
        })))
        .with_status(204)
        .expect(1)
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(stats::FILE_NAME);
    let config = config_for(&format!("{}/ingest", server.url()), true);

    stats::send_now(&file, &config, &inputs(), at(0), Reach::Local)
        .await
        .unwrap();
    mock.assert_async().await;
    let state = stats::load(&file);
    assert_eq!(state.sent_at(), Some(at(0)));
    assert_eq!(state.attempted_at(), Some(at(0)));
}

#[tokio::test]
async fn a_local_endpoint_is_refused_unless_local_is_allowed() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(stats::FILE_NAME);
    let config = config_for(&server.url(), true);
    let err = stats::send_now(&file, &config, &inputs(), at(0), Reach::Public)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, SendError::Failed(e) if e.contains("not allowed")),
        "{err:?}"
    );
    mock.assert_async().await;
    // The failed attempt counts: the next try is tomorrow
    let state = stats::load(&file);
    assert_eq!(state.attempted_at(), Some(at(0)));
    assert_eq!(state.sent_at(), None);
}

#[tokio::test]
async fn a_failing_receiver_is_tried_once_a_day_at_most() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/")
        .with_status(500)
        .expect(3)
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(stats::FILE_NAME);
    let config = config_for(&format!("{}/", server.url()), true);

    // Drive the schedule with an injected clock, one tick an hour, for three and a bit days
    let mut schedule = Schedule::new(Duration::from_secs(1800));
    let mut sends = Vec::new();
    for hour in 0..=75 {
        let now = at(hour) + Span::minutes(1);
        let last = stats::load(&file).attempted_at();
        if schedule.due(&config.usage_stats, last, now) {
            let r = stats::send_now(&file, &config, &inputs(), now, Reach::Local).await;
            assert!(matches!(r, Err(SendError::Failed(_))));
            sends.push(hour);
            schedule = Schedule::new(Duration::from_secs(1800));
        }
    }
    // First after the delay, then a day (plus the delay) apart
    assert_eq!(sends, vec![1, 26, 51]);
    mock.assert_async().await;
}

#[test]
fn an_unwritable_state_file_stops_a_send_before_it_starts() {
    let dir = tempfile::tempdir().unwrap();
    // A directory where the file should be
    let file = dir.path().join(stats::FILE_NAME);
    std::fs::create_dir(&file).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let config = config_for("https://stats.example/", true);
    let r = rt.block_on(stats::send_now(
        &file,
        &config,
        &inputs(),
        at(0),
        Reach::Public,
    ));
    assert!(matches!(r, Err(SendError::NotSaved(_))), "{r:?}");
}

#[test]
fn the_config_round_trips_and_defaults_stay_out_of_a_saved_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    Config::default().save(&path).unwrap();
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("usage_stats"));
    let mut c = Config::default();
    c.usage_stats.enabled = true;
    c.usage_stats.endpoint = "https://stats.example/ingest".into();
    c.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("[usage_stats]"), "{text}");
    let back: Config = toml::from_str(&text).unwrap();
    assert_eq!(back.usage_stats, c.usage_stats);
    c.usage_stats.endpoint = "ftp://x".into();
    assert!(c.validate().is_err());
}
