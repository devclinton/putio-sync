//! Queries the doctor reads (feature 010).

use putio_sync_db::models::{DownloadStatus, QueueStatus};
use putio_sync_db::Database;

async fn db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join(".putiosync.db"))
        .await
        .unwrap();
    (dir, db)
}

#[tokio::test]
async fn quick_check_is_ok_on_a_fresh_db() {
    let (_dir, db) = db().await;
    assert_eq!(db.quick_check().await.unwrap(), vec!["ok"]);
}

#[tokio::test]
async fn failed_and_claimed_split_queue_rows_by_status() {
    let (_dir, db) = db().await;
    let q = db.queue();
    for id in 1..=5 {
        q.add(id, "f.mkv", 1, "", "", 0).await.unwrap();
    }
    q.update_status(2, QueueStatus::Downloading).await.unwrap();
    q.update_status(3, QueueStatus::Paused).await.unwrap();
    q.update_status(4, QueueStatus::Failed).await.unwrap();
    q.update_status(5, QueueStatus::Completed).await.unwrap();

    let failed: Vec<i64> = q
        .failed()
        .await
        .unwrap()
        .iter()
        .map(|i| i.file_id)
        .collect();
    assert_eq!(failed, vec![4]);
    let mut claimed: Vec<i64> = q
        .claimed()
        .await
        .unwrap()
        .iter()
        .map(|i| i.file_id)
        .collect();
    claimed.sort();
    assert_eq!(claimed, vec![1, 2, 3]);
}

#[tokio::test]
async fn failing_downloads_are_failed_or_retried_often() {
    let (_dir, db) = db().await;
    let d = db.downloads();
    d.insert_with_retry_count(1, "ok.mkv", 1, "/x/ok.mkv", DownloadStatus::Completed, 0)
        .await
        .unwrap();
    d.insert_with_retry_count(2, "bad.mkv", 1, "/x/bad.mkv", DownloadStatus::Failed, 0)
        .await
        .unwrap();
    d.insert_with_retry_count(
        3,
        "flaky.mkv",
        1,
        "/x/flaky.mkv",
        DownloadStatus::InProgress,
        3,
    )
    .await
    .unwrap();
    // Failed once, then finished: rows are per attempt, so the later success wins
    d.insert_with_retry_count(4, "late.mkv", 1, "/x/late.mkv", DownloadStatus::Failed, 1)
        .await
        .unwrap();
    d.insert_with_retry_count(
        4,
        "late.mkv",
        2,
        "/x/late.mkv",
        DownloadStatus::Completed,
        5,
    )
    .await
    .unwrap();

    let ids: Vec<i64> = d
        .failing(3)
        .await
        .unwrap()
        .iter()
        .map(|r| r.file_id)
        .collect();
    assert_eq!(ids, vec![2, 3]);
}

#[tokio::test]
async fn processed_torrent_paths_are_listed() {
    let (_dir, db) = db().await;
    let t = db.processed_torrents();
    t.record_processed("/watch/a.torrent", "c1", 10, None)
        .await
        .unwrap();
    assert_eq!(t.paths().await.unwrap(), vec!["/watch/a.torrent"]);
}
