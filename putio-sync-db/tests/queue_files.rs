//! Pausing, resuming and retrying chosen queue rows (feature 039, R10)

use putio_sync_db::models::QueueStatus;
use putio_sync_db::Database;

async fn db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join(".putiosync.db"))
        .await
        .unwrap();
    (dir, db)
}

/// Rows 1..=5 as pending, downloading, paused, failed and completed
async fn one_of_each(db: &Database) {
    let q = db.queue();
    for id in 1..=5 {
        q.add(id, "f.mkv", 1, "", "", 10 - id, QueueStatus::Pending)
            .await
            .unwrap();
    }
    q.update_status(2, QueueStatus::Downloading).await.unwrap();
    q.update_status(3, QueueStatus::Paused).await.unwrap();
    q.update_status(4, QueueStatus::Failed).await.unwrap();
    q.update_status(5, QueueStatus::Completed).await.unwrap();
}

async fn status(db: &Database, id: i64) -> String {
    db.queue().get_by_file_id(id).await.unwrap().unwrap().status
}

#[tokio::test]
async fn add_takes_the_initial_status_and_keeps_an_existing_rows_status() {
    let (_dir, db) = db().await;
    let q = db.queue();
    q.add(1, "a.mkv", 1, "", "", 0, QueueStatus::Paused)
        .await
        .unwrap();
    assert_eq!(status(&db, 1).await, "paused");
    q.add(1, "a.mkv", 1, "", "", 3, QueueStatus::Pending)
        .await
        .unwrap();
    assert_eq!(status(&db, 1).await, "paused");
}

#[tokio::test]
async fn pause_files_pauses_only_pending_or_downloading_rows() {
    let (_dir, db) = db().await;
    one_of_each(&db).await;
    let mut changed = db.queue().pause_files(&[1, 2, 3, 4, 5, 99]).await.unwrap();
    changed.sort();
    assert_eq!(changed, vec![1, 2]);
    for (id, want) in [
        (1, "paused"),
        (2, "paused"),
        (3, "paused"),
        (4, "failed"),
        (5, "completed"),
    ] {
        assert_eq!(status(&db, id).await, want, "row {id}");
    }
}

#[tokio::test]
async fn resume_files_puts_paused_rows_back_in_queue_order() {
    let (_dir, db) = db().await;
    one_of_each(&db).await;
    db.queue()
        .update_status(1, QueueStatus::Paused)
        .await
        .unwrap();
    let before = db.queue().get_all().await.unwrap();

    let mut changed = db.queue().resume_files(&[1, 2, 3, 4]).await.unwrap();
    changed.sort();
    assert_eq!(changed, vec![1, 3]);
    assert_eq!(status(&db, 1).await, "pending");
    assert_eq!(status(&db, 2).await, "downloading");
    assert_eq!(status(&db, 3).await, "pending");
    assert_eq!(status(&db, 4).await, "failed");

    let after = db.queue().get_all().await.unwrap();
    let order = |rows: &[putio_sync_db::models::QueueItem]| {
        rows.iter()
            .map(|r| (r.id, r.file_id, r.priority))
            .collect::<Vec<_>>()
    };
    assert_eq!(order(&before), order(&after));
}

#[tokio::test]
async fn retry_files_puts_failed_rows_back_in_queue_order() {
    let (_dir, db) = db().await;
    one_of_each(&db).await;
    let before = db.queue().get_by_file_id(4).await.unwrap().unwrap();
    assert_eq!(db.queue().retry_files(&[1, 3, 4]).await.unwrap(), vec![4]);
    let after = db.queue().get_by_file_id(4).await.unwrap().unwrap();
    assert_eq!(after.status, "pending");
    assert_eq!((after.id, after.priority), (before.id, before.priority));
    assert_eq!(status(&db, 3).await, "paused");
}

/// The Pause local downloads switch puts running rows back to `pending`, not `paused`
#[tokio::test]
async fn unclaim_files_puts_only_downloading_rows_back_to_pending() {
    let (_dir, db) = db().await;
    one_of_each(&db).await;
    assert_eq!(
        db.queue().unclaim_files(&[1, 2, 3, 4]).await.unwrap(),
        vec![2]
    );
    assert_eq!(status(&db, 2).await, "pending");
    assert_eq!(status(&db, 3).await, "paused");
    assert_eq!(status(&db, 4).await, "failed");
}

#[tokio::test]
async fn no_ids_changes_nothing() {
    let (_dir, db) = db().await;
    one_of_each(&db).await;
    assert!(db.queue().pause_files(&[]).await.unwrap().is_empty());
    assert!(db.queue().resume_files(&[]).await.unwrap().is_empty());
    assert!(db.queue().retry_files(&[]).await.unwrap().is_empty());
    assert!(db.queue().unclaim_files(&[]).await.unwrap().is_empty());
}
