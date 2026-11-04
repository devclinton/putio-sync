//! Timed pause and slow-down on the download manager (feature 032): stored so a restart
//! keeps them, the queue holds while paused, and the gauges follow.
//! Its own test binary, so the global registry sees no other test.

use std::sync::Arc;

use putio_sync_client::PutioApiClient;
use putio_sync_core::bandwidth::{Limit, OverrideMode, ScheduleEntry, Source};
use putio_sync_db::models::QueueStatus;
use putio_sync_db::pipeline::BANDWIDTH_OVERRIDE;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadError, DownloadManager};
use putio_sync_telemetry::global;

async fn manager(db: Arc<Database>, config: DownloadConfig) -> DownloadManager {
    let client = Arc::new(PutioApiClient::with_base_url(
        "tok".into(),
        "http://127.0.0.1:9".into(),
    ));
    DownloadManager::new(client, db, config)
}

fn gauge(name: &str) -> Option<f64> {
    global().sample(name, &[])
}

#[tokio::test]
async fn overrides_persist_hold_the_queue_and_set_the_gauges() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        bandwidth_limit_bytes_per_sec: Some(5_000_000),
        slow_limit_bytes_per_sec: 1_000_000,
        ..DownloadConfig::default()
    };
    let first = manager(db.clone(), config.clone()).await;
    assert_eq!(first.bandwidth().current().limit, Limit::Rate(5_000_000));

    // Bad lengths are refused and nothing is stored
    for minutes in [0, 1441] {
        let err = first
            .set_bandwidth_override(OverrideMode::Pause, minutes)
            .await
            .unwrap_err();
        assert!(matches!(err, DownloadError::InvalidInput(_)), "{err:?}");
    }
    assert_eq!(db.flags().get_text(BANDWIDTH_OVERRIDE).await.unwrap(), None);

    // Pause: stored, nothing is claimed, the gauges read 0 and 1
    let status = first
        .set_bandwidth_override(OverrideMode::Pause, 60)
        .await
        .unwrap();
    assert_eq!(
        (status.limit, status.source),
        (Limit::Paused, Source::Pause)
    );
    assert!(db
        .flags()
        .get_text(BANDWIDTH_OVERRIDE)
        .await
        .unwrap()
        .is_some_and(|t| t.contains("\"pause\"")));
    db.queue()
        .add(
            1,
            "a.mkv",
            10,
            "http://x/a",
            "a.mkv",
            0,
            QueueStatus::Pending,
        )
        .await
        .unwrap();
    assert!(!first.process_queue().await.unwrap());
    assert_eq!(
        db.queue().get_by_file_id(1).await.unwrap().unwrap().status,
        "pending"
    );
    let limit = "putio_sync_download_bandwidth_limit_bytes_per_second";
    assert_eq!(gauge(limit), Some(0.0));
    assert_eq!(gauge("putio_sync_download_bandwidth_override"), Some(1.0));

    // A restart brings it back with the same end
    let second = manager(db.clone(), config.clone()).await;
    let restored = second.restore_bandwidth_override().await.unwrap().unwrap();
    assert_eq!(Some(restored), status.override_);
    assert!(second.bandwidth().is_paused());

    // Slow replaces it; Resume schedule ends it and deletes the row
    let slow = second
        .set_bandwidth_override(OverrideMode::Slow, 30)
        .await
        .unwrap();
    assert_eq!(
        (slow.limit, slow.source),
        (Limit::Rate(1_000_000), Source::Slow)
    );
    assert_eq!(gauge(limit), Some(1_000_000.0));
    let cleared = second.clear_bandwidth_override().await.unwrap();
    assert_eq!(cleared.limit, Limit::Rate(5_000_000));
    assert!(cleared.override_.is_none());
    assert_eq!(db.flags().get_text(BANDWIDTH_OVERRIDE).await.unwrap(), None);
    assert_eq!(gauge("putio_sync_download_bandwidth_override"), Some(0.0));

    // An expired or unreadable stored override is dropped at start-up
    db.flags()
        .set_text(
            BANDWIDTH_OVERRIDE,
            r#"{"mode":"pause","until":"2020-01-01T00:00:00Z"}"#,
        )
        .await
        .unwrap();
    let third = manager(db.clone(), config.clone()).await;
    assert_eq!(third.restore_bandwidth_override().await.unwrap(), None);
    assert_eq!(db.flags().get_text(BANDWIDTH_OVERRIDE).await.unwrap(), None);
    db.flags()
        .set_text(BANDWIDTH_OVERRIDE, "not json")
        .await
        .unwrap();
    assert_eq!(third.restore_bandwidth_override().await.unwrap(), None);
    assert!(!third.bandwidth().is_paused());

    // A saved schedule reaches the running limiter
    third
        .update_config(DownloadConfig {
            bandwidth_schedule: vec![ScheduleEntry {
                days: "daily".into(),
                start: "00:00".into(),
                end: "24:00".into(),
                limit: Limit::Rate(2_000),
            }],
            ..config
        })
        .await;
    let now = third.bandwidth().current();
    assert_eq!((now.limit, now.entry), (Limit::Rate(2_000), Some(1)));
    assert_eq!(gauge(limit), Some(2_000.0));
}
