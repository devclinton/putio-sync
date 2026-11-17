//! The Event hooks check (feature 027)

mod support;

use putio_sync_core::hooks::HookConfig;
use putio_sync_doctor::{CheckId, Status};
use putio_sync_hooks::HooksService;
use std::sync::Arc;
use support::*;

fn command_hook(name: &str, program: &str) -> HookConfig {
    // A literal string, so a Windows path's backslashes aren't escapes
    toml::from_str(&format!(
        "name = \"{name}\"\nevents = [\"*\"]\ncommand = ['{program}']\ntimeout_secs = 5"
    ))
    .unwrap()
}

#[tokio::test]
async fn hooks_are_checked_for_their_program_and_last_run() {
    let mut env = Env::new().await;
    env.healthy().await;
    let mut config = putio_sync_core::config::Config::default();
    config.download.target_directory = env.downloads();

    let check = env
        .doctor
        .run()
        .await
        .check(CheckId::Hooks)
        .cloned()
        .unwrap();
    assert_eq!(check.status, Status::Ok, "{check:?}");
    assert_eq!(check.summary, "No hooks are set up.");

    // A program that is not there is a problem naming the hook
    config.hooks = vec![command_hook("gone", "/nonexistent/hook.sh")];
    env.doctor.configure(&config);
    let check = env
        .doctor
        .run()
        .await
        .check(CheckId::Hooks)
        .cloned()
        .unwrap();
    assert_eq!(check.status, Status::Problem, "{check:?}");
    assert_eq!(check.items[0].key, "gone");
    assert!(check.items[0]
        .detail
        .as_deref()
        .unwrap()
        .contains("not found"));

    // A hook whose last run failed is a warning
    let script = env.dir.path().join("fail.sh");
    std::fs::write(&script, "#!/bin/sh\necho nope >&2\nexit 3\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    config.hooks = vec![command_hook("fails", script.to_str().unwrap())];
    let service = Arc::new(HooksService::new(config.hooks.clone()));
    service.test("fails").await.unwrap();
    env.doctor = env.doctor.with_hooks(service);
    env.doctor.configure(&config);
    let check = env
        .doctor
        .run()
        .await
        .check(CheckId::Hooks)
        .cloned()
        .unwrap();
    assert_eq!(check.status, Status::Warning, "{check:?}");
    assert_eq!(check.items[0].key, "fails");
    assert!(
        check.items[0]
            .detail
            .as_deref()
            .unwrap()
            .contains("hook.test"),
        "{check:?}"
    );

    // Turned off, it is not reported
    config.hooks[0].enabled = false;
    env.doctor.configure(&config);
    let check = env
        .doctor
        .run()
        .await
        .check(CheckId::Hooks)
        .cloned()
        .unwrap();
    assert_eq!(check.status, Status::Ok, "{check:?}");
}
