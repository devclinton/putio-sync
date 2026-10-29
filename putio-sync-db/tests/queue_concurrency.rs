//! Concurrent queue claims must never fail with "database is locked" (live NAS logs, 145x).

use putio_sync_db::models::QueueStatus;
use putio_sync_db::Database;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Barrier;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn claim_next_pending_is_safe_under_concurrency() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );

    for id in 1..=10 {
        db.queue()
            .add(
                id,
                &format!("file{id}.mkv"),
                1,
                "",
                "",
                0,
                QueueStatus::Pending,
            )
            .await
            .unwrap();
    }

    let workers = 8;
    let barrier = Arc::new(Barrier::new(workers));
    let mut handles = Vec::new();
    for _ in 0..workers {
        let db = db.clone();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            let mut claimed = Vec::new();
            loop {
                match db.queue().claim_next_pending().await {
                    Ok(Some(item)) => claimed.push(item.file_id),
                    Ok(None) => break,
                    Err(e) => return Err(e.to_string()),
                }
            }
            Ok(claimed)
        }));
    }

    let mut all = Vec::new();
    for handle in handles {
        let claimed = handle.await.unwrap().expect("claim must not error");
        all.extend(claimed);
    }

    let unique: HashSet<_> = all.iter().copied().collect();
    assert_eq!(all.len(), 10, "every item claimed exactly once: {all:?}");
    assert_eq!(unique.len(), 10);
}
