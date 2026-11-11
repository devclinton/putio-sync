//! The example plugins in `examples/plugins/` load and give the answers their READMEs
//! promise (feature 028), the WebAssembly ones too (029).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use putio_sync_core::plugins::PluginSettings;
use putio_sync_core::sync_hooks::{FilterAnswer, Hook, HookFile, SyncHooks};
use putio_sync_plugins::{PluginState, PluginsOptions, PluginsService};

fn examples() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins")
}

fn is_wasm(folder: &Path) -> bool {
    std::fs::read_to_string(folder.join("plugin.toml"))
        .is_ok_and(|t| t.lines().any(|l| l.replace(' ', "") == "runtime=\"wasm\""))
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
        // The WebAssembly ports answer the same; they have their own test
        .filter(|e| !is_wasm(&e.path()))
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

/// Build the Go example when `go` is installed; false (with the reason) when it is not
fn build_go(folder: &Path) -> bool {
    let built = Command::new("go")
        .arg("-C")
        .arg(folder)
        .args([
            "build",
            "-buildmode=c-shared",
            "-trimpath",
            "-ldflags=-s -w",
        ])
        .args(["-o", "plugin.wasm", "."])
        .env("GOOS", "wasip1")
        .env("GOARCH", "wasm")
        .status();
    match built {
        Ok(status) if status.success() => true,
        Ok(status) => panic!("go build of {} failed: {}", folder.display(), status),
        Err(e) => {
            eprintln!("skipping go-min-size-filter: go is not installed ({e})");
            false
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wasm_examples_load_and_answer() {
    let rust = examples().join("rust-clean-names");
    assert!(
        rust.join("plugin.wasm").is_file(),
        "rust-clean-names/plugin.wasm is checked in"
    );
    let mut names = vec!["rust-clean-names".to_string()];
    let go = build_go(&examples().join("go-min-size-filter"));
    if go {
        names.push("go-min-size-filter".to_string());
    }
    let plugins = PluginsService::new(PluginsOptions {
        dir: Some(examples()),
        settings: names
            .iter()
            .map(|n| PluginSettings {
                name: n.clone(),
                enabled: true,
                hook_timeout_ms: 2000,
                // Go's runtime asks for more than the default 32 MiB
                memory_mb: 64,
                ..PluginSettings::default()
            })
            .collect(),
        ..PluginsOptions::default()
    });
    plugins.start();

    let until = Instant::now() + Duration::from_secs(20);
    let hooks: &[Hook] = if go {
        &[Hook::Rename, Hook::Filter]
    } else {
        &[Hook::Rename]
    };
    for &hook in hooks {
        while !plugins.provides(hook) {
            let status = plugins.status();
            assert!(
                Instant::now() < until,
                "no plugin provides {hook}: {status:?}"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    for (name, want) in [
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
    ] {
        let got = plugins.rename(&file(name, 500, "download")).await;
        assert_eq!(got.as_deref(), want, "{name}");
    }
    if go {
        assert_eq!(
            plugins.filter(&file("sample.mkv", 10, "download")).await,
            Some((FilterAnswer::Skip, "go-min-size-filter".to_string()))
        );
        assert_eq!(plugins.filter(&file("film.mkv", 900, "stream")).await, None);
        assert_eq!(
            plugins.filter(&file("sample.mkv", 10, "blocked")).await,
            None
        );
    }

    // on_event: a test event runs, logs and uses the store
    for name in &names {
        let run = plugins.test(name).await.unwrap();
        assert_eq!(run.error, None, "{name}");
        assert!(!run.logs.is_empty(), "{name}: {:?}", run);
    }
    for status in plugins
        .status()
        .into_iter()
        .filter(|s| names.contains(&s.name))
    {
        assert_eq!(status.runtime, "wasm");
        assert_eq!(
            status.state,
            PluginState::Enabled,
            "{}: {:?}",
            status.name,
            status.error
        );
    }
}
