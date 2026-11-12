//! `putio-sync plugins check` (feature 059): the report and exit code for good and bad
//! plugin folders

use assert_cmd::Command;
use assert_fs::prelude::*;
use predicates::prelude::*;

const MANIFEST: &str = "name = \"hello\"\nversion = \"1.0.0\"\nevents = [\"*\"]\n";

fn check() -> Command {
    let mut cmd = Command::cargo_bin("putio-sync").unwrap();
    cmd.arg("plugins").arg("check");
    cmd
}

fn folder(manifest: &str, script: &str) -> assert_fs::TempDir {
    let dir = assert_fs::TempDir::new().unwrap();
    dir.child("plugin.toml").write_str(manifest).unwrap();
    dir.child("main.lua").write_str(script).unwrap();
    dir
}

#[test]
fn a_good_folder_is_ok() {
    let dir = folder(MANIFEST, "return {on_event = function() end}");
    check()
        .arg(dir.path())
        .assert()
        .code(0)
        .stdout("hello 1.0.0: ok\n");
}

#[test]
fn a_bad_link_and_a_syntax_error_are_reported() {
    let dir = folder(
        &format!("{}docs = \"javascript:alert(1)\"\n", MANIFEST),
        "return {",
    );
    check()
        .arg(dir.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains("docs: must be an https address"))
        .stdout(predicate::str::contains("1 problem\n"));

    let dir = folder(MANIFEST, "return {\n on_event = function( end\n}");
    check()
        .arg(dir.path())
        .assert()
        .code(1)
        .stdout(predicate::str::starts_with("main.lua:2: "))
        .stdout(predicate::str::ends_with("hello 1.0.0: 1 problem\n"));
}

#[test]
fn a_missing_entry_and_a_missing_folder() {
    let dir = folder(MANIFEST, "");
    std::fs::remove_file(dir.path().join("main.lua")).unwrap();
    check()
        .arg(dir.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains("main.lua: not found"));
    check().arg(dir.path().join("nope")).assert().code(2);
}

#[test]
fn json_report() {
    let dir = folder(MANIFEST, "return {");
    let out = check().arg("--json").arg(dir.path()).assert().code(1);
    let report: serde_json::Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(report["name"], "hello");
    assert_eq!(report["version"], "1.0.0");
    assert_eq!(report["problems"][0]["file"], "main.lua");
}
