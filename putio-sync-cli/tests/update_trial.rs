//! A new version that fails its first start puts the old binary back and restarts it
//! (055 US3, T036). Unix only: the "old binary" is a shell script.
#![cfg(unix)]

use assert_cmd::Command;
use putio_sync_update::{swap, trial, version::Version};
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn a_failed_first_start_rolls_back_and_restarts_the_old_binary() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let exe = dir.path().join("putio-sync");
    let ran = dir.path().join("ran");

    // The "old version": records that it was restarted, and with which arguments
    let old = swap::old_path(&exe);
    std::fs::write(
        &old,
        format!("#!/bin/sh\necho \"$@\" > '{}'\n", ran.display()),
    )
    .unwrap();
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o755)).unwrap();
    // "Update now" swapped in this build of putio-sync
    trial::begin(
        dir.path(),
        "0.0.1".parse().unwrap(),
        Version::current(),
        "full",
        &exe,
        None,
    )
    .await
    .unwrap();

    Command::cargo_bin("putio-sync")
        .unwrap()
        .args(["--config", config.to_str().unwrap(), "daemon"])
        .env("PUTIO_SYNC_TEST_FAIL_START", "1")
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();

    // The old binary is back under the name and ran in place of the failed start
    assert!(!old.exists());
    assert!(std::fs::read_to_string(&exe)
        .unwrap()
        .starts_with("#!/bin/sh"));
    let args = std::fs::read_to_string(&ran).unwrap();
    assert!(args.contains("daemon"), "{args}");
    // ...and the old version will show why
    assert!(
        matches!(
            trial::read(dir.path()),
            Some(trial::Marker::RolledBack { ref reason, .. }) if reason.contains("crashed at start")
        ),
        "{:?}",
        trial::read(dir.path())
    );
}

#[tokio::test]
async fn a_second_start_of_the_same_update_rolls_back_without_running() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let exe = dir.path().join("putio-sync");
    let ran = dir.path().join("ran");
    let old = swap::old_path(&exe);
    std::fs::write(&old, format!("#!/bin/sh\ntouch '{}'\n", ran.display())).unwrap();
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o755)).unwrap();
    trial::begin(
        dir.path(),
        "0.0.1".parse().unwrap(),
        Version::current(),
        "patch",
        &exe,
        None,
    )
    .await
    .unwrap();
    // The first start counted itself, then died without a panic (killed, power cut)
    assert_eq!(
        trial::on_start(dir.path(), Version::current()),
        trial::Start::Trial
    );

    Command::cargo_bin("putio-sync")
        .unwrap()
        .args(["--config", config.to_str().unwrap(), "daemon"])
        .timeout(std::time::Duration::from_secs(60))
        .assert()
        .success();
    assert!(ran.exists());
    assert!(matches!(
        trial::read(dir.path()),
        Some(trial::Marker::RolledBack { .. })
    ));
}
