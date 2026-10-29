//! The daemon's scheduled runs (T031)

mod support;

use putio_sync_core::config::Config;
use putio_sync_doctor::{run_scheduled, STARTUP_DELAY};
use std::sync::Arc;
use std::time::Duration;
use support::*;

/// Let the startup delay pass on paused time, then go back to real time for the HTTP calls
async fn skip_startup_delay() {
    tokio::time::pause();
    tokio::time::advance(STARTUP_DELAY + Duration::from_secs(1)).await;
    tokio::time::resume();
}

#[tokio::test]
async fn a_scheduled_run_records_the_check_gauges() {
    let mut env = Env::new().await;
    env.healthy().await;
    env.gone(77).await;
    env.db
        .queue()
        .add(
            77,
            "gone.mkv",
            10,
            "u",
            "Movies",
            0,
            putio_sync_db::models::QueueStatus::Pending,
        )
        .await
        .unwrap();
    let doctor = Arc::new(env.doctor.with_metrics());
    let task = tokio::spawn(run_scheduled(doctor.clone()));
    tokio::task::yield_now().await;
    assert!(doctor.latest().is_none(), "ran before the startup delay");

    skip_startup_delay().await;
    for _ in 0..100 {
        if doctor.latest().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    task.abort();
    assert!(doctor.latest().is_some(), "no scheduled run");
    let status = putio_sync_telemetry::global().sample(
        "putio_sync_doctor_check_status",
        &[("check", "dead_queue_rows")],
    );
    assert_eq!(status, Some(1.0));
}

#[tokio::test]
async fn no_scheduled_run_while_disabled() {
    let env = Env::new().await;
    let mut config = Config::default();
    config.download.target_directory = env.downloads();
    config.doctor.enabled = false;
    env.doctor.configure(&config);
    let doctor = Arc::new(env.doctor);
    let task = tokio::spawn(run_scheduled(doctor.clone()));
    skip_startup_delay().await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    task.abort();
    assert!(doctor.latest().is_none());
}
