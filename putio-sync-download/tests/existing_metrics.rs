//! One sync hitting every existing-file outcome counts each once in
//! `putio_sync_existing_files_total` and records it on its history row (feature 040).
//! Its own test binary, so the global registry sees no other test.

use std::sync::Arc;

use mockito::Matcher;
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::ExistingFile;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadFile, DownloadManager};
use putio_sync_telemetry::global;

const SIZE: usize = 4096;

fn crc(data: &[u8]) -> String {
    format!(
        "{:08x}",
        crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(data)
    )
}

fn count(outcome: &str) -> f64 {
    global()
        .sample("putio_sync_existing_files_total", &[("outcome", outcome)])
        .unwrap_or(0.0)
}

#[tokio::test]
async fn every_outcome_is_counted_once_and_kept_on_its_history_row() {
    let mut server = mockito::Server::new_async().await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(PutioApiClient::with_base_url("tok".into(), server.url()));
    let manager = DownloadManager::new(
        client,
        db.clone(),
        DownloadConfig {
            target_directory: dir.path().to_path_buf(),
            max_retries: 0,
            retry_initial_delay_secs: 0,
            ..DownloadConfig::default()
        },
    );
    let outcomes = [
        "skipped",
        "resumed",
        "restarted",
        "overwritten",
        "saved_as",
        "refused",
    ];
    let before: Vec<f64> = outcomes.iter().map(|o| count(o)).collect();

    let mut mocks = Vec::new();
    for id in 1..=6 {
        mocks.push(
            server
                .mock("GET", format!("/files/{id}/download").as_str())
                .match_header("range", Matcher::Missing)
                .with_body(vec![9u8; SIZE])
                .create_async()
                .await,
        );
        mocks.push(
            server
                .mock("GET", format!("/files/{id}/download").as_str())
                .match_header("range", Matcher::Any)
                .with_status(206)
                .with_body(vec![9u8; SIZE / 2])
                .create_async()
                .await,
        );
    }
    let half_then_nines = [vec![7u8; SIZE / 2], vec![9u8; SIZE / 2]].concat();
    let path = |name: &str| dir.path().join(name);
    std::fs::write(path("skipped.bin"), vec![9u8; SIZE]).unwrap();
    std::fs::write(path("resumed.bin"), vec![7u8; SIZE / 2]).unwrap();
    std::fs::write(path("restarted.bin"), vec![7u8; SIZE / 2]).unwrap();
    std::fs::write(path("overwritten.bin"), b"old").unwrap();
    std::fs::write(path("saved_as.bin"), b"old").unwrap();
    std::fs::create_dir(path("refused.bin")).unwrap();

    let cases = [
        (1, "skipped.bin", ExistingFile::Resume, crc(&[9u8; SIZE])),
        (
            2,
            "resumed.bin",
            ExistingFile::Resume,
            crc(&half_then_nines),
        ),
        (3, "restarted.bin", ExistingFile::Resume, crc(&[9u8; SIZE])),
        (
            4,
            "overwritten.bin",
            ExistingFile::Overwrite,
            crc(&[9u8; SIZE]),
        ),
        (5, "saved_as.bin", ExistingFile::KeepBoth, crc(&[9u8; SIZE])),
        (6, "refused.bin", ExistingFile::Resume, crc(&[9u8; SIZE])),
    ];
    for (id, name, choice, crc32) in cases {
        let mut config = manager.get_config().await;
        config.on_existing = choice;
        manager.update_config(config).await;
        manager
            .download_file(
                DownloadFile {
                    file_id: id,
                    filename: name.to_string(),
                    relative_path: String::new(),
                    file_size: SIZE as u64,
                    download_url: format!("{}/files/{id}/download", server.url()),
                    crc32: Some(crc32),
                    save_as: None,
                    fixed_path: false,
                },
                None,
            )
            .await
            .unwrap();
    }

    for (outcome, before) in outcomes.iter().zip(before) {
        assert_eq!(count(outcome) - before, 1.0, "{outcome}");
    }
    let history = db.history().query(None, 50, 0, None).await.unwrap();
    for row in &history {
        let outcome = row.outcome.as_deref().unwrap();
        let expected = if outcome == "saved_as" {
            path("saved_as (1).bin")
        } else {
            path(&format!("{outcome}.bin"))
        };
        assert_eq!(row.local_path.as_deref(), expected.to_str(), "{outcome}");
    }
    let mut seen: Vec<_> = history.iter().filter_map(|r| r.outcome.clone()).collect();
    seen.sort();
    // A refused file is not downloaded, so it has no history row
    assert_eq!(
        seen,
        ["overwritten", "restarted", "resumed", "saved_as", "skipped"]
    );
}
