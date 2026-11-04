//! A finished download puts `download.finished` on the event bus, a failed one
//! `download.failed`, and a skipped one nothing (feature 027).

use std::sync::Arc;
use std::time::Duration;

use putio_sync_client::PutioApiClient;
use putio_sync_core::events::{self, Event, Payload};
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadFile, DownloadManager};
use tokio::sync::broadcast;

const SIZE: usize = 2048;

fn file(id: i64, name: &str, dir: &str, url: String) -> DownloadFile {
    DownloadFile {
        file_id: id,
        filename: name.to_string(),
        relative_path: dir.to_string(),
        file_size: SIZE as u64,
        download_url: url,
        crc32: None,
        save_as: None,
        fixed_path: false,
    }
}

/// The next download event for `file_id`, or `None` within a short wait.
async fn next_for(rx: &mut broadcast::Receiver<Arc<Event>>, file_id: i64) -> Option<Arc<Event>> {
    let wait = async {
        loop {
            let event = rx.recv().await.ok()?;
            let id = match &event.payload {
                Payload::DownloadFinished(d) => d.file_id,
                Payload::DownloadFailed(d) => d.file_id,
                _ => continue,
            };
            if id == file_id {
                return Some(event);
            }
        }
    };
    tokio::time::timeout(Duration::from_millis(500), wait)
        .await
        .ok()
        .flatten()
}

#[tokio::test]
async fn downloads_emit_finished_and_failed_but_skips_do_not() {
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
        db,
        DownloadConfig {
            target_directory: dir.path().to_path_buf(),
            max_retries: 0,
            retry_initial_delay_secs: 0,
            ..DownloadConfig::default()
        },
    );
    server
        .mock("GET", "/files/7001/download")
        .with_body(vec![1u8; SIZE])
        .create_async()
        .await;
    server
        .mock("GET", "/files/7002/download")
        .with_status(500)
        .create_async()
        .await;
    let mut rx = events::subscribe();

    manager
        .download_file(
            file(
                7001,
                "ep.mkv",
                "Shows/S1",
                format!("{}/files/7001/download", server.url()),
            ),
            None,
        )
        .await
        .unwrap();
    let event = next_for(&mut rx, 7001).await.expect("download.finished");
    let Payload::DownloadFinished(d) = &event.payload else {
        panic!("{:?}", event)
    };
    assert_eq!(d.name, "ep.mkv");
    assert_eq!(d.relative_path, "Shows/S1/ep.mkv");
    assert_eq!(
        d.local_path,
        dir.path().join("Shows/S1/ep.mkv").display().to_string()
    );
    assert_eq!(d.size_bytes, SIZE as u64);
    assert!(!d.resumed);

    manager
        .download_file(
            file(
                7002,
                "bad.mkv",
                "",
                format!("{}/files/7002/download", server.url()),
            ),
            None,
        )
        .await
        .unwrap();
    let event = next_for(&mut rx, 7002).await.expect("download.failed");
    let Payload::DownloadFailed(d) = &event.payload else {
        panic!("{:?}", event)
    };
    assert_eq!(d.relative_path, "bad.mkv");
    assert_eq!(d.attempts, 1);
    assert!(!d.error.is_empty());

    // Already there and the same size: skipped, no event.
    manager
        .download_file(
            file(
                7001,
                "ep.mkv",
                "Shows/S1",
                format!("{}/files/7001/download", server.url()),
            ),
            None,
        )
        .await
        .unwrap();
    assert!(next_for(&mut rx, 7001).await.is_none());
}
