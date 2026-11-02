//! Queue columns for existing-file handling (feature 040)

use putio_sync_db::models::QueueStatus;
use putio_sync_db::Database;

#[tokio::test]
async fn fixed_path_survives_a_later_add_and_save_as_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join(".putiosync.db"))
        .await
        .unwrap();
    let queue = db.queue();

    queue
        .add_with_path_kept(
            1,
            "Movie.mkv",
            10,
            "",
            "Movies",
            0,
            QueueStatus::Pending,
            true,
        )
        .await
        .unwrap();
    queue
        .add(1, "Movie.mkv", 10, "", "Movies", 0, QueueStatus::Pending)
        .await
        .unwrap();
    queue
        .add(2, "Other.mkv", 10, "", "", 0, QueueStatus::Pending)
        .await
        .unwrap();

    let item = queue.get_by_file_id(1).await.unwrap().unwrap();
    assert!(item.fixed_path);
    assert_eq!(item.save_as, None);
    assert!(!queue.get_by_file_id(2).await.unwrap().unwrap().fixed_path);

    queue.set_save_as(2, Some("Other (1).mkv")).await.unwrap();
    let item = queue.get_by_file_id(2).await.unwrap().unwrap();
    assert_eq!(item.save_as.as_deref(), Some("Other (1).mkv"));
    queue.set_save_as(2, None).await.unwrap();
    assert_eq!(
        queue.get_by_file_id(2).await.unwrap().unwrap().save_as,
        None
    );
}
