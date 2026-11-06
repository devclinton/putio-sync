//! The switch gauge follows the stored flag, so a change made by the CLI shows too (feature
//! 039, T041). Its own test binary: the gauge is process-wide, and every pipeline tick sets it.

use putio_sync_client::PutioApiClient;
use putio_sync_core::stall::StallTracker;
use putio_sync_daemon::daemon::pipeline_tick;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn pipeline_tick_sets_the_nas_switch_gauge() {
    let mut server = mockito::Server::new_async().await;
    let _list = server
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK","transfers":[]}"#)
        .create_async()
        .await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(PutioApiClient::with_base_url("t".to_string(), server.url()));
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        ..DownloadConfig::default()
    };
    let manager = DownloadManager::new(client, db.clone(), config);
    let tracker = StallTracker::new(Duration::from_secs(3600));
    let gauge = || putio_sync_telemetry::global().sample("putio_sync_nas_downloads_paused", &[]);

    db.flags()
        .set_bool(putio_sync_db::pipeline::NAS_DOWNLOADS_PAUSED, true)
        .await
        .unwrap();
    pipeline_tick(&manager, &tracker, 24).await.unwrap();
    assert_eq!(gauge(), Some(1.0));

    db.flags()
        .set_bool(putio_sync_db::pipeline::NAS_DOWNLOADS_PAUSED, false)
        .await
        .unwrap();
    pipeline_tick(&manager, &tracker, 24).await.unwrap();
    assert_eq!(gauge(), Some(0.0));
}
