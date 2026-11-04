//! The setup wizard's dry run (feature 013): totals, samples, files already present,
//! the time limit, and nothing written.

use putio_sync_client::PutioApiClient;
use putio_sync_core::config::DownloadConfig as DownloadSettings;
use putio_sync_core::rules::FolderRule;
use putio_sync_download::{dry_run, DownloadConfig, DRY_RUN_SAMPLES};
use std::time::Duration;

const ROOT: &str = r#"{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false}"#;

fn entry(id: i64, name: &str, size: i64, parent_id: i64) -> String {
    let folder = !name.contains('.');
    format!(
        r#"{{"id":{id},"name":"{name}","size":{size},"file_type":"{}","parent_id":{parent_id},"is_folder":{folder},"is_shared":false}}"#,
        if folder { "FOLDER" } else { "VIDEO" }
    )
}

async fn list(server: &mut mockito::ServerGuard, id: i64, entries: &[String]) -> mockito::Mock {
    server
        .mock("GET", format!("/files/list?parent_id={id}").as_str())
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"files":[{}],"parent":{ROOT},"status":"OK"}}"#,
            entries.join(",")
        ))
        .create_async()
        .await
}

/// Root: Movies/ (film.mkv 5000, setup.exe 10), Music/ (song.flac 300), top.mkv 700
async fn mock_account(server: &mut mockito::ServerGuard) {
    list(
        server,
        0,
        &[
            entry(10, "Movies", 0, 0),
            entry(30, "Music", 0, 0),
            entry(1, "top.mkv", 700, 0),
        ],
    )
    .await;
    list(
        server,
        10,
        &[
            entry(11, "film.mkv", 5000, 10),
            entry(12, "setup.exe", 10, 10),
        ],
    )
    .await;
    list(server, 30, &[entry(31, "song.flac", 300, 30)]).await;
}

fn config(target: &std::path::Path, settings: DownloadSettings) -> DownloadConfig {
    DownloadConfig::from(&DownloadSettings {
        target_directory: target.to_path_buf(),
        ..settings
    })
}

#[tokio::test]
async fn reports_what_would_be_downloaded() {
    let mut server = mockito::Server::new_async().await;
    mock_account(&mut server).await;
    let client = PutioApiClient::with_base_url("t".into(), server.url());
    let dir = tempfile::tempdir().unwrap();
    // Already there with the same size; a different size still downloads
    std::fs::create_dir_all(dir.path().join("Music")).unwrap();
    std::fs::write(dir.path().join("Music/song.flac"), vec![0u8; 300]).unwrap();
    std::fs::write(dir.path().join("top.mkv"), b"short").unwrap();

    let report = dry_run(
        &client,
        &config(dir.path(), DownloadSettings::default()),
        Duration::from_secs(10),
    )
    .await
    .unwrap();
    assert_eq!(report.files_seen, 4);
    assert_eq!(
        (report.to_download.count, report.to_download.bytes),
        (2, 5700)
    );
    assert_eq!(report.already_present.count, 1);
    assert_eq!((report.skipped.count, report.skipped.bytes), (1, 10));
    assert!(!report.partial);
    let paths: Vec<_> = report.samples.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(paths, ["Movies/film.mkv", "top.mkv"]);
    // Nothing written
    assert!(!dir.path().join("Movies").exists());
}

#[tokio::test]
async fn applies_the_account_preset_and_folder_rules() {
    let mut server = mockito::Server::new_async().await;
    mock_account(&mut server).await;
    let client = PutioApiClient::with_base_url("t".into(), server.url());
    let dir = tempfile::tempdir().unwrap();
    let settings = DownloadSettings {
        preset: Some("audio".to_string()),
        rules: vec![FolderRule {
            path: "Movies".to_string(),
            preset: Some("video".to_string()),
            ..FolderRule::default()
        }],
        ..DownloadSettings::default()
    };
    let report = dry_run(
        &client,
        &config(dir.path(), settings),
        Duration::from_secs(10),
    )
    .await
    .unwrap();
    let paths: Vec<_> = report.samples.iter().map(|s| s.path.as_str()).collect();
    assert_eq!(paths, ["Movies/film.mkv", "Music/song.flac"]);
    assert_eq!(report.skipped.count, 2); // top.mkv (audio only), setup.exe
}

#[tokio::test]
async fn keeps_the_largest_samples() {
    let mut server = mockito::Server::new_async().await;
    let files: Vec<String> = (1..=100)
        .map(|i| entry(i, &format!("f{i:03}.mkv"), i * 10, 0))
        .collect();
    list(&mut server, 0, &files).await;
    let client = PutioApiClient::with_base_url("t".into(), server.url());
    let dir = tempfile::tempdir().unwrap();
    let report = dry_run(
        &client,
        &config(dir.path(), DownloadSettings::default()),
        Duration::from_secs(10),
    )
    .await
    .unwrap();
    assert_eq!(report.samples.len(), DRY_RUN_SAMPLES);
    assert_eq!(report.samples[0].path, "f100.mkv");
    assert_eq!(report.samples[DRY_RUN_SAMPLES - 1].size, 810);
    assert_eq!(report.to_download.count, 100);
}

#[tokio::test]
async fn stops_at_the_time_limit() {
    let mut server = mockito::Server::new_async().await;
    mock_account(&mut server).await;
    let client = PutioApiClient::with_base_url("t".into(), server.url());
    let dir = tempfile::tempdir().unwrap();
    let report = dry_run(
        &client,
        &config(dir.path(), DownloadSettings::default()),
        Duration::ZERO,
    )
    .await
    .unwrap();
    assert!(report.partial);
    assert_eq!(report.files_seen, 0);
}
