//! `putio-sync archive` (feature 041): the three commands from a config file, against a
//! mocked Prometheus, and the exit codes of the binary

use assert_cmd::Command;
use chrono::{DateTime, Utc};
use flate2::read::GzDecoder;
use mockito::{Matcher, Server};
use putio_sync_cli::commands::archive::{run, ArchiveCommon, ArchiveJob};
use putio_sync_core::config::{AuthConfig, Config};
use putio_sync_db::Database;
use std::io::Read;
use std::path::{Path, PathBuf};

const TOKEN: &str = "TOKEN-abcdef0123456789";
const URL_PASSWORD: &str = "urlpass-0123456789";

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .unwrap()
        .with_timezone(&Utc)
}

/// A config with a Put.io token and a download folder holding a database with a download
async fn setup(dir: &Path) -> PathBuf {
    let downloads = dir.join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let db = Database::new(downloads.join(".putiosync.db"))
        .await
        .unwrap();
    db.history()
        .insert_download_at(1, "a.mkv", 1000, Some(10), None, at("2026-11-01T01:00:00Z"))
        .await
        .unwrap();
    db.close().await.unwrap();
    let config = dir.join("config.toml");
    let mut settings = Config {
        auth: Some(AuthConfig::with_token(TOKEN.to_string())),
        ..Config::default()
    };
    settings.download.target_directory = downloads;
    settings.save(&config).unwrap();
    config
}

fn day_answer() -> String {
    let values: Vec<String> = (0..288)
        .map(|i| {
            format!(
                r#"[{},"1"]"#,
                at("2026-11-01T00:00:00Z").timestamp() + i * 300
            )
        })
        .collect();
    format!(
        r#"{{"status":"success","data":{{"resultType":"matrix","result":[
        {{"metric":{{"__name__":"putio_sync:up:min5m","instance":"nas:9742"}},"values":[{}]}}]}}}}"#,
        values.join(",")
    )
}

/// Every file under `dir`, gunzipped when it ends in `.gz`, as bytes
fn contents(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(contents(&path));
        } else {
            let mut bytes = std::fs::read(&path).unwrap();
            if path.extension().is_some_and(|e| e == "gz") {
                let mut plain = Vec::new();
                GzDecoder::new(bytes.as_slice())
                    .read_to_end(&mut plain)
                    .unwrap();
                bytes = plain;
            }
            out.push((path, bytes));
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

#[tokio::test]
async fn all_three_commands_run_and_write_no_token() {
    let dir = tempfile::tempdir().unwrap();
    let config = setup(dir.path()).await;
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/api/v1/query")
        .match_query(Matcher::UrlEncoded(
            "time".into(),
            "2026-11-02T00:00:00Z".into(),
        ))
        .with_body(day_answer())
        .create_async()
        .await;
    let archive = dir.path().join("archive");
    let common = ArchiveCommon {
        dir: Some(archive.clone()),
        require_manifest: false,
    };
    let now = at("2026-11-02T03:30:00Z");
    let mut out = Vec::new();

    run(
        ArchiveJob::Metrics {
            common: common.clone(),
            prometheus: Some(server.url()),
            instance: Some("nas:9742".to_string()),
            keep_days: None,
            since: Some("2026-11-01".parse().unwrap()),
        },
        Some(config.clone()),
        now,
        &mut out,
    )
    .await
    .unwrap();
    run(
        ArchiveJob::Db {
            common: common.clone(),
            keep_days: None,
        },
        Some(config.clone()),
        now,
        &mut out,
    )
    .await
    .unwrap();
    run(
        ArchiveJob::Summary {
            common,
            prometheus: Some(server.url()),
            instance: Some("nas:9742".to_string()),
            from: None,
            to: Some("2026-11-01".parse().unwrap()),
        },
        Some(config),
        now,
        &mut out,
    )
    .await
    .unwrap();

    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("saved 2026-11-01 points=288/288"), "{out}");
    assert!(out.contains("saved 2026-11-02 rows=1"), "{out}");
    assert!(out.contains("wrote summary/usage-daily.csv"), "{out}");
    assert!(!out.contains(TOKEN));
    let files = contents(&archive);
    assert!(files.len() >= 6, "{files:?}");
    for (path, bytes) in files {
        assert!(
            !contains(&bytes, TOKEN),
            "{} holds the token",
            path.display()
        );
    }
}

fn archive_command(config: &Path) -> Command {
    let mut cmd = Command::cargo_bin("putio-sync").unwrap();
    cmd.env_remove("PUTIO_SYNC_ARCHIVE_DIR")
        .env_remove("PUTIO_SYNC_ARCHIVE_PROMETHEUS_URL")
        .env_remove("PUTIO_SYNC_ARCHIVE_INSTANCE")
        .env_remove("PUTIO_SYNC_ARCHIVE_KEEP_DAYS")
        .env("PUTIO_SYNC_CONFIG", config)
        .arg("--config")
        .arg(config)
        .arg("archive");
    cmd
}

/// The single `archive <cmd>:` line of stderr
fn archive_line(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("archive ")).collect();
    assert_eq!(lines.len(), 1, "{text}");
    lines[0].to_string()
}

#[tokio::test]
#[cfg_attr(
    windows,
    ignore = "the CLI overflows its 1 MB main-thread stack on Windows"
)]
async fn metrics_without_a_prometheus_url_is_exit_1() {
    let dir = tempfile::tempdir().unwrap();
    let config = setup(dir.path()).await;
    let assert = archive_command(&config)
        .args(["metrics", "--dir"])
        .arg(dir.path().join("archive"))
        .assert()
        .code(1);
    let stderr = assert.get_output().stderr.clone();
    assert!(archive_line(&stderr).starts_with("archive metrics: --prometheus is required"));
}

#[tokio::test]
#[cfg_attr(
    windows,
    ignore = "the CLI overflows its 1 MB main-thread stack on Windows"
)]
async fn an_unreachable_prometheus_is_exit_2_and_the_line_shows_no_secret() {
    let dir = tempfile::tempdir().unwrap();
    let config = setup(dir.path()).await;
    let url = format!("http://user:{URL_PASSWORD}@127.0.0.1:1");
    let assert = archive_command(&config)
        .args([
            "metrics",
            "--instance",
            "nas:9742",
            "--prometheus",
            &url,
            "--dir",
        ])
        .arg(dir.path().join("archive"))
        .assert()
        .code(2);
    let output = assert.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = archive_line(&output.stderr);
    assert!(
        line.starts_with("archive metrics: cannot reach Prometheus (http://127.0.0.1:1)"),
        "{line}"
    );
    for secret in [URL_PASSWORD, TOKEN] {
        assert!(!stderr.contains(secret), "{stderr}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
    }
}

#[tokio::test]
#[cfg_attr(
    windows,
    ignore = "the CLI overflows its 1 MB main-thread stack on Windows"
)]
async fn a_missing_database_is_exit_3() {
    let dir = tempfile::tempdir().unwrap();
    let config = setup(dir.path()).await;
    std::fs::remove_file(dir.path().join("downloads/.putiosync.db")).unwrap();
    let assert = archive_command(&config)
        .args(["db", "--dir"])
        .arg(dir.path().join("archive"))
        .assert()
        .code(3);
    assert!(archive_line(&assert.get_output().stderr).starts_with("archive db: cannot open"));
}

#[tokio::test]
#[cfg_attr(
    windows,
    ignore = "the CLI overflows its 1 MB main-thread stack on Windows"
)]
async fn an_archive_folder_that_cannot_be_made_is_exit_4() {
    let dir = tempfile::tempdir().unwrap();
    let config = setup(dir.path()).await;
    let assert = archive_command(&config)
        .args(["db", "--dir"])
        .arg(dir.path().join("no/such/parent/archive"))
        .assert()
        .code(4);
    assert!(archive_line(&assert.get_output().stderr).starts_with("archive db: cannot create"));
}

#[tokio::test]
#[cfg_attr(
    windows,
    ignore = "the CLI overflows its 1 MB main-thread stack on Windows"
)]
async fn the_default_folder_is_beside_the_config_and_env_sets_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let config = setup(dir.path()).await;
    archive_command(&config).arg("db").assert().success();
    assert!(dir.path().join("archive/manifest.jsonl").is_file());

    let other = dir.path().join("elsewhere");
    archive_command(&config)
        .env("PUTIO_SYNC_ARCHIVE_DIR", &other)
        .arg("db")
        .assert()
        .success();
    assert!(other.join("manifest.jsonl").is_file());
}
