//! DB error counting and queue counts (T015).

use putio_sync_db::models::QueueStatus;
use putio_sync_db::Database;
use putio_sync_telemetry::global;

fn queue_errors() -> f64 {
    global()
        .sample("putio_sync_db_errors_total", &[("operation", "queue")])
        .unwrap_or(0.0)
}

#[tokio::test]
async fn closed_pool_counts_one_queue_error() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join(".putiosync.db"))
        .await
        .unwrap();
    db.pool().close().await;

    let before = queue_errors();
    assert!(db.queue().add(1, "a.mkv", 1, "", "", 0).await.is_err());
    assert_eq!(queue_errors() - before, 1.0);
}

#[tokio::test]
async fn counts_by_status_groups_gauged_states() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join(".putiosync.db"))
        .await
        .unwrap();
    for id in 1..=4 {
        db.queue().add(id, "f.mkv", 1, "", "", 0).await.unwrap();
    }
    db.queue()
        .update_status(3, QueueStatus::Failed)
        .await
        .unwrap();
    db.queue()
        .update_status(4, QueueStatus::Completed)
        .await
        .unwrap();

    let mut counts = db.queue().counts_by_status().await.unwrap();
    counts.sort();
    assert_eq!(
        counts,
        vec![("failed".to_string(), 1), ("pending".to_string(), 2)]
    );
}
