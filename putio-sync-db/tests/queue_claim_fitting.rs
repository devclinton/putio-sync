//! A claim that takes a size limit skips a big head row for a smaller one that fits.

use putio_sync_db::models::QueueStatus;
use putio_sync_db::Database;

async fn open() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join(".putiosync.db"))
        .await
        .unwrap();
    (dir, db)
}

async fn add(db: &Database, id: i64, size: i64) {
    db.queue()
        .add(
            id,
            &format!("f{id}.mkv"),
            size,
            "",
            "",
            0,
            QueueStatus::Pending,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn big_head_row_is_skipped_and_left_pending() {
    let (_dir, db) = open().await;
    add(&db, 1, 900).await;
    add(&db, 2, 50).await;
    add(&db, 3, 40).await;
    let before = db.queue().get_by_file_id(1).await.unwrap().unwrap();

    let got = db.queue().claim_next_pending_fitting(100).await.unwrap();
    assert_eq!(
        got.unwrap().file_id,
        2,
        "first row that fits, in queue order"
    );
    let got = db.queue().claim_next_pending_fitting(100).await.unwrap();
    assert_eq!(got.unwrap().file_id, 3);
    assert!(db
        .queue()
        .claim_next_pending_fitting(100)
        .await
        .unwrap()
        .is_none());

    let after = db.queue().get_by_file_id(1).await.unwrap().unwrap();
    assert_eq!(after.status, "pending");
    assert_eq!(after.updated_at, before.updated_at, "no claim churn");

    // With room for it, the head row is claimed as before
    assert_eq!(
        db.queue()
            .claim_next_pending_fitting(900)
            .await
            .unwrap()
            .unwrap()
            .file_id,
        1
    );
}

#[tokio::test]
async fn unknown_size_always_fits() {
    let (_dir, db) = open().await;
    add(&db, 1, 0).await;
    add(&db, 2, -1).await;
    assert_eq!(
        db.queue()
            .claim_next_pending_fitting(0)
            .await
            .unwrap()
            .unwrap()
            .file_id,
        1
    );
    assert_eq!(
        db.queue()
            .claim_next_pending_fitting(0)
            .await
            .unwrap()
            .unwrap()
            .file_id,
        2
    );
}
