//! The example plugins in `examples/plugins/` load and give the answers their READMEs
//! promise (feature 028).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use putio_sync_core::plugins::PluginSettings;
use putio_sync_core::sync_hooks::{FilterAnswer, Hook, HookFile, SyncHooks};
use putio_sync_plugins::{PluginState, PluginsOptions, PluginsService};

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins")
}

fn file(name: &str, mb: u64, decision: &str) -> HookFile {
    HookFile {
        id: 1,
        name: name.to_string(),
        size: mb * 1024 * 1024,
        extension: name
            .rsplit_once('.')
            .map(|(_, e)| e.to_lowercase())
            .unwrap_or_default(),
        folder: "Downloads".to_string(),
        decision: decision.to_string(),
        reason: String::new(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_example_loads_and_answers() {
    let names: Vec<String> = std::fs::read_dir(examples())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("plugin.toml").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    for wanted in ["min-size-filter", "clean-names", "tv-folders"] {
        assert!(names.iter().any(|n| n == wanted), "{wanted} missing");
    }
    let plugins = PluginsService::new(PluginsOptions {
        dir: Some(examples()),
        settings: names
            .iter()
            .map(|n| PluginSettings {
                name: n.clone(),
                enabled: true,
                hook_timeout_ms: 2000,
                ..PluginSettings::default()
            })
            .collect(),
        ..PluginsOptions::default()
    });
    plugins.start();

    let until = Instant::now() + Duration::from_secs(10);
    for hook in Hook::ALL {
        while !plugins.provides(hook) {
            assert!(Instant::now() < until, "no plugin provides {hook}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    // filter
    let small = plugins.filter(&file("sample.mkv", 10, "download")).await;
    assert_eq!(
        small,
        Some((FilterAnswer::Skip, "min-size-filter".to_string()))
    );
    assert_eq!(
        plugins.filter(&file("film.mkv", 900, "download")).await,
        None
    );
    assert_eq!(
        plugins.filter(&file("notes.txt", 0, "download")).await,
        None
    );
    assert_eq!(
        plugins.filter(&file("sample.mkv", 10, "blocked")).await,
        None
    );

    // rename
    let cases = [
        (
            "Show.S01E02.1080p.WEB.x264-GRP.mkv",
            Some("Show S01E02.mkv"),
        ),
        (
            "The.Movie.2019.1080p.BluRay.x264.mp4",
            Some("The Movie (2019).mp4"),
        ),
        ("holiday.mkv", None),
        ("Show.S01E02.en.srt", None),
    ];
    for (name, want) in cases {
        let got = plugins.rename(&file(name, 500, "download")).await;
        assert_eq!(got.as_deref(), want, "{name}");
    }

    // folder
    let folder = plugins
        .folder(&file("Show.Name.S01E02.1080p.mkv", 500, "download"))
        .await;
    assert_eq!(folder.as_deref(), Some("TV/Show Name/Season 01"));
    assert_eq!(
        plugins.folder(&file("holiday.mkv", 500, "download")).await,
        None
    );

    for status in plugins
        .status()
        .into_iter()
        .filter(|s| names.contains(&s.name))
    {
        assert_eq!(
            status.state,
            PluginState::Enabled,
            "{}: {:?}",
            status.name,
            status.error
        );
        assert!(!status.hooks.is_empty(), "{}", status.name);
        assert!(
            status.runs.iter().all(|r| r.error.is_none()),
            "{}: {:?}",
            status.name,
            status.runs
        );
    }
}
