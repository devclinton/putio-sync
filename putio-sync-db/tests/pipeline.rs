//! Holds, daemon flags, Recently finished and the pipeline file lookup (feature 039)

use putio_sync_db::models::{DownloadSource, DownloadStatus, Origin, QueueStatus};
use putio_sync_db::pipeline::{FileState, FinishedOutcome, FinishedRow, NAS_DOWNLOADS_PAUSED};
use putio_sync_db::Database;

async fn db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join(".putiosync.db"))
        .await
        .unwrap();
    (dir, db)
}

fn source(transfer_id: Option<i64>) -> DownloadSource {
    DownloadSource {
        transfer_id,
        source_type: None,
        tracker_host: None,
        is_private: false,
        origin: Origin::Putio,
        watch_folder: None,
        putio_folder: None,
    }
}

fn finished(key: &str, outcome: FinishedOutcome, done_at: &str) -> FinishedRow {
    FinishedRow {
        key: key.into(),
        name: format!("{key} name"),
        size: 100,
        outcome,
        reason: None,
        added_at: None,
        putio_done_at: None,
        queued_at: None,
        done_at: done_at.into(),
    }
}

#[tokio::test]
async fn holds_add_list_remove() {
    let (_dir, db) = db().await;
    let holds = db.holds();
    assert!(!holds.is_held(7).await.unwrap());
    holds.add(7, "Show S01").await.unwrap();
    holds.add(7, "Show S01").await.unwrap();
    holds.add(8, "Film").await.unwrap();
    assert!(holds.is_held(7).await.unwrap());
    let list = holds.list().await.unwrap();
    assert_eq!(
        list.iter()
            .map(|h| (h.transfer_id, h.name.as_str()))
            .collect::<Vec<_>>(),
        vec![(7, "Show S01"), (8, "Film")]
    );
    assert!(holds.remove(7).await.unwrap());
    assert!(!holds.remove(7).await.unwrap());
    assert!(!holds.is_held(7).await.unwrap());
}

#[tokio::test]
async fn flags_default_off_and_persist() {
    let (_dir, db) = db().await;
    let flags = db.flags();
    assert!(!flags.get_bool(NAS_DOWNLOADS_PAUSED).await.unwrap());
    flags.set_bool(NAS_DOWNLOADS_PAUSED, true).await.unwrap();
    assert!(flags.get_bool(NAS_DOWNLOADS_PAUSED).await.unwrap());
    let raw: String = sqlx::query_scalar("SELECT value FROM daemon_flags WHERE key = ?")
        .bind(NAS_DOWNLOADS_PAUSED)
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(raw, "1");
    flags.set_bool(NAS_DOWNLOADS_PAUSED, false).await.unwrap();
    assert!(!flags.get_bool(NAS_DOWNLOADS_PAUSED).await.unwrap());
}

#[tokio::test]
async fn finished_upsert_list_newest_first_and_prune() {
    let (_dir, db) = db().await;
    let fin = db.finished();
    fin.upsert(&finished(
        "transfer:1",
        FinishedOutcome::Failed,
        "2026-01-01 10:00:00",
    ))
    .await
    .unwrap();
    fin.upsert(&finished(
        "transfer:2",
        FinishedOutcome::Done,
        "2026-01-01 11:00:00",
    ))
    .await
    .unwrap();
    // A later write for the same key replaces the row
    let mut again = finished("transfer:1", FinishedOutcome::Done, "2026-01-01 12:00:00");
    again.queued_at = Some("2026-01-01 11:30:00".into());
    fin.upsert(&again).await.unwrap();

    let list = fin.list().await.unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0], again);
    assert_eq!(list[1].key, "transfer:2");

    // Both rows are far older than 24 hours; one written now stays
    let now: String = sqlx::query_scalar("SELECT datetime('now')")
        .fetch_one(db.pool())
        .await
        .unwrap();
    fin.upsert(&finished("file:3", FinishedOutcome::Done, &now))
        .await
        .unwrap();
    assert_eq!(fin.prune_older_than(24).await.unwrap(), 2);
    let list = fin.list().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].key, "file:3");
}

#[tokio::test]
async fn pipeline_files_reports_queued_done_and_skipped_files() {
    let (_dir, db) = db().await;
    // Transfer 10: one queued, one done (no queue row), one skipped (neither)
    db.queue()
        .add(1, "e01.mkv", 100, "", "Show S01", 0, QueueStatus::Pending)
        .await
        .unwrap();
    db.queue()
        .update_status(1, QueueStatus::Downloading)
        .await
        .unwrap();
    for id in 1..=3 {
        db.sources().record(id, &source(Some(10))).await.unwrap();
    }
    db.downloads()
        .insert(
            2,
            "e02.mkv",
            200,
            "/data/Show S01/e02.mkv",
            DownloadStatus::Completed,
        )
        .await
        .unwrap();
    // A queue row with no source row (a folder upload, no transfer)
    db.queue()
        .add(4, "pic.jpg", 5, "", "Photos", 0, QueueStatus::Paused)
        .await
        .unwrap();

    let mut files = db.pipeline_files(&[10], 24).await.unwrap();
    files.sort_by_key(|f| f.file_id);
    let summary: Vec<_> = files
        .iter()
        .map(|f| (f.file_id, f.transfer_id, f.state.clone(), f.size))
        .collect();
    assert_eq!(
        summary,
        vec![
            (1, Some(10), FileState::Queued("downloading".into()), 100),
            (2, Some(10), FileState::Done, 200),
            (3, Some(10), FileState::Skipped, 0),
            (4, None, FileState::Queued("paused".into()), 5),
        ]
    );
    assert_eq!(files[0].relative_path.as_deref(), Some("Show S01"));
    assert_eq!(files[0].origin.as_deref(), Some("putio"));
    assert!(files[0].recorded_at.is_some());
    assert!(files[1].downloaded_at.is_some());
    assert_eq!(files[3].origin, None);
}

#[tokio::test]
async fn pipeline_files_reads_only_active_listed_or_recent_files() {
    let (_dir, db) = db().await;
    // Transfer 20 is not in the list: its done file counts only while recent
    db.sources().record(5, &source(Some(20))).await.unwrap();
    db.downloads()
        .insert(5, "old.mkv", 1, "/data/old.mkv", DownloadStatus::Completed)
        .await
        .unwrap();
    // Transfer 21 is not in the list and its file was skipped: never read
    db.sources().record(6, &source(Some(21))).await.unwrap();

    let ids = |files: Vec<putio_sync_db::pipeline::PipelineFile>| {
        files.iter().map(|f| f.file_id).collect::<Vec<_>>()
    };
    assert_eq!(ids(db.pipeline_files(&[], 24).await.unwrap()), vec![5]);

    sqlx::query("UPDATE downloads SET downloaded_at = datetime('now', '-25 hours')")
        .execute(db.pool())
        .await
        .unwrap();
    assert!(db.pipeline_files(&[], 24).await.unwrap().is_empty());
    assert_eq!(ids(db.pipeline_files(&[20], 24).await.unwrap()), vec![5]);
}

#[tokio::test]
async fn transfer_origins_prefer_the_arr_category() {
    use putio_sync_db::arr::NewArrItem;
    use putio_sync_db::models::TransferOrigin;
    let (_dir, db) = db().await;
    let watch = TransferOrigin {
        origin: Origin::Watch,
        watch_folder: Some("/watch".into()),
    };
    db.sources()
        .record_transfer_origin(1, &watch)
        .await
        .unwrap();
    db.sources()
        .record_transfer_origin(2, &watch)
        .await
        .unwrap();
    db.arr()
        .insert(&NewArrItem {
            hash: "abc",
            name: "Film",
            category: "radarr",
            transfer_id: Some(2),
            putio_status: "IN_QUEUE",
            total_size: 0,
        })
        .await
        .unwrap();

    let origins = db.transfer_origins(&[1, 2, 3]).await.unwrap();
    assert_eq!(origins.get(&1).map(String::as_str), Some("watch"));
    assert_eq!(origins.get(&2).map(String::as_str), Some("arr:radarr"));
    assert_eq!(origins.get(&3), None);
    assert!(db.transfer_origins(&[]).await.unwrap().is_empty());
}
