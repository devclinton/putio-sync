//! CLI output hygiene: no startup banner, the cause chain in errors, a next step for auth
//! errors, help that says which command to run, and shell completions

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

fn cli() -> Command {
    let mut cmd = Command::cargo_bin("putio-sync").unwrap();
    cmd.env_remove("PUTIO_SYNC_CONFIG");
    cmd
}

#[test]
fn help_and_version_print_no_startup_banner() {
    for arg in ["--help", "--version"] {
        cli()
            .arg(arg)
            .assert()
            .success()
            .stderr(predicate::str::is_empty());
    }
}

#[test]
fn help_says_what_to_run() {
    cli()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("web UI"))
        .stdout(predicate::str::contains("putio-sync daemon"))
        .stdout(predicate::str::contains("putio-sync setup"))
        .stdout(predicate::str::contains("putio-sync doctor"));
    cli()
        .args(["daemon", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Docker image runs"));
    cli()
        .args(["server", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("most people want 'daemon'"));
}

#[test]
fn no_token_error_names_the_next_step_and_shows_no_banner() {
    let dir = assert_fs::TempDir::new().unwrap();
    let config = dir.child("config.toml");
    config
        .write_str("[auth]\nauth_method = \"Token\"\n")
        .unwrap();
    cli()
        .arg("--config")
        .arg(config.path())
        .args(["files", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("putio-sync login"))
        .stderr(predicate::str::contains("STARTUP DEBUG").not())
        .stderr(predicate::str::contains("Program completed").not());
}

#[test]
fn completions_print_a_script() {
    cli()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("putio-sync"))
        .stderr(predicate::str::is_empty());
}
