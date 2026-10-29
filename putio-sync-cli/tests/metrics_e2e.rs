//! The shared CLI helper serves metrics before any database exists, then picks up queue
//! counts once one is set (US1, T024, FR-004, FR-007).

use putio_sync_cli::telemetry;
use putio_sync_core::config::Config;
use putio_sync_db::Database;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

async fn scrape(addr: SocketAddr) -> String {
    let response = reqwest::get(format!("http://{addr}/metrics"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    response.text().await.unwrap()
}

fn sample(body: &str, series: &str) -> Option<f64> {
    body.lines()
        .find_map(|l| l.strip_prefix(series)?.trim().parse().ok())
}

#[tokio::test]
async fn serves_without_db_then_reports_queue_and_disk() {
    std::env::set_var("PUTIO_SYNC_METRICS_BIND", "127.0.0.1:0");
    let dir = tempfile::tempdir().unwrap();
    let queue_db = Arc::new(OnceLock::new());

    // Seed the database on real time: sqlx's pool timeouts misfire under a paused clock.
    let db = Database::new(dir.path().join(".putiosync.db"))
        .await
        .unwrap();
    for id in 1..=2 {
        db.queue()
            .add(
                id,
                "f.mkv",
                1,
                "",
                "",
                0,
                putio_sync_db::models::QueueStatus::Pending,
            )
            .await
            .unwrap();
    }
    tokio::time::pause();

    let addr = telemetry::start(
        &Config::default(),
        queue_db.clone(),
        2,
        dir.path().to_path_buf(),
    )
    .await
    .unwrap()
    .expect("listener started");
    tokio::task::yield_now().await;

    let body = scrape(addr).await;
    assert_eq!(
        sample(&body, "putio_sync_queue_items{state=\"pending\"}"),
        Some(0.0)
    );

    queue_db.set(Arc::new(db)).ok().unwrap();

    let mut body = String::new();
    for _ in 0..10 {
        tokio::time::advance(Duration::from_secs(16)).await;
        body = scrape(addr).await;
        if sample(&body, "putio_sync_queue_items{state=\"pending\"}") == Some(2.0) {
            break;
        }
    }
    assert_eq!(
        sample(&body, "putio_sync_queue_items{state=\"pending\"}"),
        Some(2.0),
        "{body}"
    );
    assert!(sample(&body, "putio_sync_disk_free_bytes").unwrap() > 0.0);
    assert_eq!(sample(&body, "putio_sync_download_workers"), Some(2.0));
}
