//! The Plugins check (feature 028)

mod support;

use putio_sync_core::plugins::PluginSettings;
use putio_sync_doctor::{CheckId, CheckResult, Status};
use putio_sync_plugins::{PluginsOptions, PluginsService};
use std::path::Path;
use std::sync::Arc;
use support::*;

fn write(dir: &Path, name: &str, manifest: &str, script: &str) {
    let path = dir.join(name);
    std::fs::create_dir_all(&path).unwrap();
    std::fs::write(
        path.join("plugin.toml"),
        format!("name = \"{name}\"\n{manifest}"),
    )
    .unwrap();
    std::fs::write(path.join("main.lua"), script).unwrap();
}

fn enabled(name: &str) -> PluginSettings {
    PluginSettings {
        name: name.to_string(),
        enabled: true,
        ..PluginSettings::default()
    }
}

async fn check(env: &Env) -> CheckResult {
    env.doctor
        .run()
        .await
        .check(CheckId::Plugins)
        .cloned()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plugins_are_checked_for_their_state_and_last_run() {
    let mut env = Env::new().await;
    env.healthy().await;
    let plugins_dir = env.dir.path().join("plugins");
    write(
        &plugins_dir,
        "fails",
        "version = \"1\"\nevents = [\"*\"]\n",
        "return {on_event = function() error('webhook said no') end}",
    );
    write(
        &plugins_dir,
        "needy",
        concat!(
            "version = \"1\"\nevents = [\"*\"]\n",
            "[[config]]\nkey = \"topic\"\ntype = \"string\"\nrequired = true\n",
        ),
        "return {on_event = function() end}",
    );
    write(&plugins_dir, "broken", "", "");
    let mut config = putio_sync_core::config::Config::default();
    config.download.target_directory = env.downloads();
    config.plugins_dir = Some(plugins_dir.clone());

    // Nothing on is fine
    env.doctor.configure(&config);
    let found = check(&env).await;
    assert_eq!(found.status, Status::Ok, "{found:?}");
    assert_eq!(found.summary, "7 plugins found, none on.");

    // An enabled plugin that cannot load is a problem naming it
    config.plugins = vec![enabled("broken"), enabled("needy")];
    env.doctor.configure(&config);
    let found = check(&env).await;
    assert_eq!(found.status, Status::Problem, "{found:?}");
    assert_eq!(found.items.len(), 1);
    assert_eq!(found.items[0].key, "broken");
    assert!(found.items[0]
        .detail
        .as_deref()
        .unwrap()
        .contains("plugin.toml"));

    // A missing required value and settings for no plugin are warnings
    config.plugins = vec![enabled("needy"), enabled("ghost")];
    env.doctor.configure(&config);
    let found = check(&env).await;
    assert_eq!(found.status, Status::Warning, "{found:?}");
    assert_eq!(found.summary, "2 plugins need attention.");
    let keys: Vec<&str> = found.items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys, ["needy", "ghost"]);
    assert!(found.items[1]
        .detail
        .as_deref()
        .unwrap()
        .contains("no plugin"));

    // An enabled plugin whose last run failed is a warning with the error
    config.plugins = vec![enabled("fails")];
    let service = Arc::new(PluginsService::new(PluginsOptions {
        dir: Some(plugins_dir),
        data_dir: None,
        settings: config.plugins.clone(),
        clock: None,
    }));
    service.start();
    service.test("fails").await.unwrap();
    env.doctor = env.doctor.with_plugins(service);
    env.doctor.configure(&config);
    let found = check(&env).await;
    assert_eq!(found.status, Status::Warning, "{found:?}");
    assert_eq!(found.summary, "1 plugin needs attention.");
    let detail = found.items[0].detail.as_deref().unwrap();
    assert!(detail.contains("webhook said no"), "{detail}");
    assert!(detail.contains("plugin.test"), "{detail}");
}
