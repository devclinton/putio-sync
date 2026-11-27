//! The CLI's language choice, end to end (feature 060)

use assert_cmd::Command;
use predicates::prelude::*;
#[cfg(feature = "i18n")]
use std::collections::BTreeMap;

/// `putio-sync` with no language in its environment and no config file
fn cli(dir: &assert_fs::TempDir) -> Command {
    let mut cmd = Command::cargo_bin("putio-sync").unwrap();
    for name in ["PUTIO_SYNC_LANG", "LANGUAGE", "LC_ALL", "LC_MESSAGES"] {
        cmd.env_remove(name);
    }
    cmd.env("LANG", "C")
        .env("HOME", dir.path())
        .env("PUTIO_SYNC_CONFIG", dir.path().join("config.toml"));
    cmd
}

#[cfg(feature = "i18n")]
fn current(cmd: &mut Command) -> (String, String) {
    let out = cmd.args(["languages", "--json"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    (
        value["current"]["code"].as_str().unwrap().to_string(),
        value["current"]["source"].as_str().unwrap().to_string(),
    )
}

#[cfg(feature = "i18n")]
fn catalog(code: &str) -> BTreeMap<String, String> {
    let path = format!("{}/i18n/{code}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn languages_json_has_the_documented_shape() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = cli(&dir).args(["languages", "--json"]).output().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        value["current"],
        serde_json::json!({"code": "en", "source": "default"})
    );
    let languages = value["languages"].as_array().unwrap();
    assert_eq!(
        languages[0],
        serde_json::json!({"code": "en", "name": "English", "rtl": false})
    );
    #[cfg(feature = "i18n")]
    {
        assert_eq!(languages.len(), 27);
        let rtl: Vec<&str> = languages
            .iter()
            .filter(|l| l["rtl"] == true)
            .map(|l| l["code"].as_str().unwrap())
            .collect();
        assert_eq!(rtl, ["ar", "he"]);
    }
    #[cfg(not(feature = "i18n"))]
    assert_eq!(languages.len(), 1);
}

#[cfg(feature = "i18n")]
#[test]
fn the_language_comes_from_flag_then_env_then_config_then_system() {
    let dir = assert_fs::TempDir::new().unwrap();
    std::fs::write(dir.path().join("config.toml"), "[ui]\nlanguage = \"fr\"\n").unwrap();
    let pair = |code: &str, source: &str| (code.to_string(), source.to_string());

    let mut all = cli(&dir);
    all.env("PUTIO_SYNC_LANG", "ja").env("LANG", "it_IT.UTF-8");
    assert_eq!(current(all.args(["--lang", "de"])), pair("de", "flag"));

    let mut env = cli(&dir);
    env.env("PUTIO_SYNC_LANG", "ja").env("LANG", "it_IT.UTF-8");
    assert_eq!(current(&mut env), pair("ja", "env"));

    let mut config = cli(&dir);
    config.env("LANG", "it_IT.UTF-8");
    assert_eq!(current(&mut config), pair("fr", "config"));

    std::fs::remove_file(dir.path().join("config.toml")).unwrap();
    let mut system = cli(&dir);
    system.env("LANG", "it_IT.UTF-8");
    assert_eq!(current(&mut system), pair("it", "os"));

    let mut list = cli(&dir);
    list.env("LANGUAGE", "xx:pt_BR:de")
        .env("LANG", "it_IT.UTF-8");
    assert_eq!(current(&mut list), pair("pt", "os"));

    // C and POSIX ask for English, quietly
    cli(&dir)
        .env("LC_ALL", "POSIX")
        .args(["languages", "--json"])
        .assert()
        .success()
        .stderr("");

    // The flag works after the subcommand too
    let out = cli(&dir)
        .args(["languages", "--json", "--lang", "ko"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["current"]["code"], "ko");
    assert_eq!(value["current"]["source"], "flag");
}

#[cfg(feature = "i18n")]
#[test]
fn an_unknown_language_warns_once_and_changes_nothing_else() {
    let dir = assert_fs::TempDir::new().unwrap();
    let out = cli(&dir)
        .env("LANG", "de_DE.UTF-8")
        .args(["--lang", "xx", "languages", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.starts_with("putio-sync: "), "{stderr}");
    assert!(
        stderr.contains("\"xx\"") && stderr.contains("--lang en"),
        "{stderr}"
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["current"]["code"], "de");

    // Not with -q
    cli(&dir)
        .args(["-q", "--lang", "xx", "languages", "--json"])
        .assert()
        .success()
        .stderr("");
}

#[cfg(not(feature = "i18n"))]
#[test]
fn a_build_without_catalogs_says_it_has_english_only() {
    let dir = assert_fs::TempDir::new().unwrap();
    cli(&dir)
        .args(["--lang", "de", "languages"])
        .assert()
        .success()
        .stderr("putio-sync: this build has English only.\n")
        .stdout(predicate::str::contains("In use: en"));
    // A system locale is not a request: nothing is printed
    cli(&dir)
        .env("LANG", "de_DE.UTF-8")
        .args(["languages"])
        .assert()
        .success()
        .stderr("");
}

/// `--json` output and completion scripts are the same bytes in every language
#[cfg(feature = "i18n")]
#[test]
fn machine_output_is_the_same_in_every_language() {
    let dir = assert_fs::TempDir::new().unwrap();
    let runs: [&[&str]; 3] = [
        &["crash-report", "list", "--json"],
        &["completions", "bash"],
        &["plugins", "check", "--json", "."],
    ];
    for args in runs {
        if args[0] == "plugins" && !cfg!(feature = "plugins") {
            continue;
        }
        let run = |lang: &str| {
            let out = cli(&dir)
                .current_dir(dir.path())
                .env("PUTIO_SYNC_LANG", lang)
                .args(args)
                .output()
                .unwrap();
            (out.status.code(), out.stdout)
        };
        let english = run("en");
        assert!(!english.1.is_empty(), "{args:?}");
        for lang in ["de", "ja", "ar"] {
            assert_eq!(run(lang), english, "{args:?} in {lang}");
        }
    }
}

#[cfg(feature = "i18n")]
#[test]
fn help_and_argument_errors_are_translated() {
    let dir = assert_fs::TempDir::new().unwrap();
    let de = catalog("de");
    let (Some(usage), Some(about), Some(unexpected)) = (
        de.get("Usage:"),
        de.get("List the languages this build can show"),
        de.get("unexpected argument '{0}'"),
    ) else {
        panic!("de.json lacks the help strings");
    };

    let help = cli(&dir).args(["--lang", "de", "--help"]).output().unwrap();
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(
        text.contains(usage.as_str()) && text.contains(about.as_str()),
        "{text}"
    );
    // Names stay as they are typed
    for name in ["languages", "doctor", "--lang", "--config"] {
        assert!(text.contains(name), "{name} in {text}");
    }

    let sub = cli(&dir)
        .args(["languages", "--help", "--lang", "de"])
        .output()
        .unwrap();
    assert!(String::from_utf8(sub.stdout)
        .unwrap()
        .contains(usage.as_str()));

    cli(&dir)
        .args(["--lang", "de", "languages", "--bogus"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            unexpected.replace("{0}", "--bogus"),
        ));

    // English is clap's own text and exit code
    cli(&dir)
        .args(["languages", "--bogus"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unexpected argument '--bogus'"));
}

#[cfg(feature = "i18n")]
#[test]
fn the_language_list_is_translated() {
    let dir = assert_fs::TempDir::new().unwrap();
    let de = catalog("de");
    let line = de["In use: {0} (from {1})"]
        .replace("{0}", "de")
        .replace("{1}", "PUTIO_SYNC_LANG");
    cli(&dir)
        .env("PUTIO_SYNC_LANG", "de")
        .arg("languages")
        .assert()
        .success()
        .stdout(predicate::str::contains(line).and(predicate::str::contains("Deutsch")));
}
