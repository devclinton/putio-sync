//! Daily database maintenance: prune old rows, refresh the planner statistics and keep the
//! write-ahead log small

use crate::error::{tracked, DbResult};
use crate::Database;
use putio_sync_telemetry::DbOp;
use tracing::{debug, info};

/// Days a processed watch-folder torrent is remembered for de-duplication
pub const PROCESSED_TORRENTS_KEEP_DAYS: u32 = 90;
/// Days the files a Put.io free-up run deleted are kept
pub const STORAGE_RUNS_KEEP_DAYS: i64 = 90;
/// Days download history is kept (it feeds the statistics pages, so this is long)
pub const HISTORY_KEEP_DAYS: i64 = 730;

/// Days a completed download-queue row is kept (a finished download leaves the queue at
/// once; only rows left by older versions or a failed removal reach this)
pub const QUEUE_COMPLETED_KEEP_DAYS: i64 = 7;

/// What one maintenance pass removed
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MaintenanceReport {
    /// Processed torrent records deleted
    pub processed_torrents: u64,
    /// Storage runs and the files they deleted
    pub storage_rows: u64,
    /// Download history rows deleted
    pub history: u64,
    /// Completed download-queue rows deleted
    pub queue_completed: u64,
}

impl MaintenanceReport {
    /// Total rows removed
    pub fn total(&self) -> u64 {
        self.processed_torrents + self.storage_rows + self.history + self.queue_completed
    }
}

impl Database {
    /// Delete rows older than their retention, then run `PRAGMA optimize`, an incremental
    /// vacuum (only when the file uses `auto_vacuum = INCREMENTAL`) and a truncating WAL
    /// checkpoint. Meant to run once a day.
    pub async fn run_maintenance(&self) -> DbResult<MaintenanceReport> {
        let processed_torrents = self
            .processed_torrents()
            .cleanup_old(PROCESSED_TORRENTS_KEEP_DAYS)
            .await?;
        let storage_rows = self.storage().prune(STORAGE_RUNS_KEEP_DAYS).await?;
        let history = self.history().prune(HISTORY_KEEP_DAYS).await?;
        let queue_completed = self
            .queue()
            .prune_completed(QUEUE_COMPLETED_KEEP_DAYS)
            .await?;
        let report = MaintenanceReport {
            processed_torrents,
            storage_rows,
            history,
            queue_completed,
        };

        let auto_vacuum: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
            .fetch_one(&self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to read auto_vacuum"))?;
        // 2 is INCREMENTAL; on a file without it the pragma does nothing, so skip it
        if auto_vacuum == 2 && report.total() > 0 {
            sqlx::query("PRAGMA incremental_vacuum")
                .execute(&self.pool)
                .await
                .map_err(tracked(DbOp::Other, "Failed to run the incremental vacuum"))?;
        }
        sqlx::query("PRAGMA optimize")
            .execute(&self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to optimize the database"))?;
        self.checkpoint().await?;

        if report.total() > 0 {
            info!(
                processed_torrents = report.processed_torrents,
                storage_rows = report.storage_rows,
                history = report.history,
                queue_completed = report.queue_completed,
                "Pruned old database rows"
            );
        } else {
            debug!("Database maintenance found nothing to prune");
        }
        Ok(report)
    }

    /// Fold the write-ahead log into the database file and truncate it
    pub async fn checkpoint(&self) -> DbResult<()> {
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to checkpoint the WAL"))?;
        Ok(())
    }

    /// Checkpoint the WAL and close every connection, for a daemon that is stopping. Other
    /// holders of this database see it closed, so call it only after the tasks using it
    /// have ended. A failed checkpoint is logged, not returned: the pool closes anyway.
    pub async fn shutdown(&self) {
        if let Err(e) = self.checkpoint().await {
            tracing::warn!(error = %e, "WAL checkpoint at shutdown failed");
        }
        self.pool.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn db() -> (TempDir, Database) {
        let dir = TempDir::new().unwrap();
        let db = Database::new(dir.path().join("m.db")).await.unwrap();
        (dir, db)
    }

    #[tokio::test]
    async fn maintenance_prunes_old_rows_and_keeps_recent_ones() {
        let (_dir, db) = db().await;
        for (path, age) in [("old", 200), ("new", 1)] {
            sqlx::query(
                "INSERT INTO processed_torrents (file_path, file_checksum, file_size, processed_at) \
                 VALUES (?, ?, 1, datetime('now', ?))",
            )
            .bind(path)
            .bind(path)
            .bind(format!("-{age} days"))
            .execute(db.pool())
            .await
            .unwrap();
        }
        for (name, age) in [("old", 1000), ("new", 1)] {
            sqlx::query(
                "INSERT INTO download_history (file_id, filename, file_size, action, timestamp) \
                 VALUES (1, ?, 1, 'downloaded', datetime('now', ?))",
            )
            .bind(name)
            .bind(format!("-{age} days"))
            .execute(db.pool())
            .await
            .unwrap();
        }
        let old_run = sqlx::query(
            "INSERT INTO storage_runs (finished_at, trigger, files, bytes) \
             VALUES (datetime('now', '-200 days'), 'schedule', 1, 1)",
        )
        .execute(db.pool())
        .await
        .unwrap()
        .last_insert_rowid();
        sqlx::query(
            "INSERT INTO storage_run_files (run_id, file_id, path, size, local_path) \
             VALUES (?, 1, 'p', 1, 'l')",
        )
        .bind(old_run)
        .execute(db.pool())
        .await
        .unwrap();

        let report = db.run_maintenance().await.unwrap();
        assert_eq!(report.processed_torrents, 1);
        assert_eq!(report.history, 1);
        assert_eq!(report.storage_rows, 2);
        assert_eq!(db.processed_torrents().count().await.unwrap(), 1);
        let history: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM download_history")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(history, 1);

        // A second pass has nothing left to remove
        assert_eq!(db.run_maintenance().await.unwrap().total(), 0);
    }

    #[tokio::test]
    async fn maintenance_prunes_only_old_completed_queue_rows() {
        let (_dir, db) = db().await;
        for (file_id, status, age) in [
            (1, "completed", 30),
            (2, "completed", 1),
            (3, "failed", 30),
            (4, "pending", 30),
        ] {
            sqlx::query(
                "INSERT INTO download_queue (file_id, filename, file_size, download_url, status, updated_at) \
                 VALUES (?, 'f', 1, 'u', ?, datetime('now', ?))",
            )
            .bind(file_id)
            .bind(status)
            .bind(format!("-{age} days"))
            .execute(db.pool())
            .await
            .unwrap();
        }
        let report = db.run_maintenance().await.unwrap();
        assert_eq!(report.queue_completed, 1);
        let left: Vec<i64> =
            sqlx::query_scalar("SELECT file_id FROM download_queue ORDER BY file_id")
                .fetch_all(db.pool())
                .await
                .unwrap();
        assert_eq!(left, vec![2, 3, 4]);
    }

    #[tokio::test]
    async fn database_path_with_query_characters_opens() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("what?#.db");
        let db = Database::new(&path).await.unwrap();
        assert!(path.exists());
        db.shutdown().await;
    }
}
