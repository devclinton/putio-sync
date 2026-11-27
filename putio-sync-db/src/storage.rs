//! Put.io storage (feature 033): each free-up run that deleted files, with the files it
//! deleted, for the Put.io page's "Recent runs" list.

use super::error::{tracked, DbResult};
use putio_sync_telemetry::DbOp;
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

/// A file a run deleted, as it is recorded
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRunFile {
    pub file_id: i64,
    pub path: String,
    pub size: i64,
    pub created_at: Option<String>,
    pub local_path: String,
}

/// A recorded file of a run
#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromRow)]
pub struct StorageRunFile {
    pub file_id: i64,
    pub path: String,
    pub size: i64,
    pub created_at: Option<String>,
    pub local_path: String,
}

/// A recorded run with the files it deleted
#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromRow)]
pub struct StorageRun {
    pub id: i64,
    /// UTC, `YYYY-MM-DD HH:MM:SS`
    pub finished_at: String,
    /// `manual` or `schedule`
    pub trigger: String,
    pub files: i64,
    pub bytes: i64,
    pub failed: i64,
    pub free_after: Option<i64>,
    #[sqlx(skip)]
    pub deleted: Vec<StorageRunFile>,
}

/// How full a disk was at a time (feature 064)
#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRow)]
pub struct UsageSample {
    /// Unix seconds
    pub taken_at: i64,
    pub used: i64,
    pub total: i64,
}

/// Samples older than this are deleted when a new one is added
const SAMPLE_KEEP_SECS: i64 = 30 * 86_400;

/// Repository for `storage_runs`, `storage_run_files` and `usage_samples`
pub struct StorageRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> StorageRepository<'a> {
    /// Create a new storage runs repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Record a run and its deleted files in one transaction; the run's id
    pub async fn record_run(
        &self,
        trigger: &str,
        files: &[NewRunFile],
        failed: usize,
        free_after: Option<i64>,
    ) -> DbResult<i64> {
        const CONTEXT: &str = "Failed to record a storage run";
        let bytes: i64 = files.iter().map(|f| f.size).sum();
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(tracked(DbOp::Other, CONTEXT))?;
        let id = sqlx::query(
            "INSERT INTO storage_runs (trigger, files, bytes, failed, free_after) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(trigger)
        .bind(files.len() as i64)
        .bind(bytes)
        .bind(failed as i64)
        .bind(free_after)
        .execute(&mut *tx)
        .await
        .map_err(tracked(DbOp::Other, CONTEXT))?
        .last_insert_rowid();
        for file in files {
            sqlx::query(
                "INSERT INTO storage_run_files (run_id, file_id, path, size, created_at, local_path) VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(file.file_id)
            .bind(&file.path)
            .bind(file.size)
            .bind(&file.created_at)
            .bind(&file.local_path)
            .execute(&mut *tx)
            .await
            .map_err(tracked(DbOp::Other, CONTEXT))?;
        }
        tx.commit().await.map_err(tracked(DbOp::Other, CONTEXT))?;
        Ok(id)
    }

    /// Delete runs finished more than `days` days ago with the files they deleted; the
    /// rows deleted
    pub async fn prune(&self, days: i64) -> DbResult<u64> {
        const CONTEXT: &str = "Failed to prune storage runs";
        let cutoff = format!("-{days} days");
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(tracked(DbOp::Other, CONTEXT))?;
        let files = sqlx::query(
            "DELETE FROM storage_run_files WHERE run_id IN \
             (SELECT id FROM storage_runs WHERE finished_at < datetime('now', ?))",
        )
        .bind(&cutoff)
        .execute(&mut *tx)
        .await
        .map_err(tracked(DbOp::Other, CONTEXT))?;
        let runs = sqlx::query("DELETE FROM storage_runs WHERE finished_at < datetime('now', ?)")
            .bind(&cutoff)
            .execute(&mut *tx)
            .await
            .map_err(tracked(DbOp::Other, CONTEXT))?;
        tx.commit().await.map_err(tracked(DbOp::Other, CONTEXT))?;
        Ok(files.rows_affected() + runs.rows_affected())
    }

    /// Record that `kind` (`local` or `putio`) had `used` of `total` bytes in use at
    /// `at` (Unix seconds), unless it has a sample less than `min_gap_secs` old. Samples
    /// older than 30 days go when one is added. True when it was recorded.
    pub async fn add_sample(
        &self,
        kind: &str,
        at: i64,
        used: i64,
        total: i64,
        min_gap_secs: i64,
    ) -> DbResult<bool> {
        const CONTEXT: &str = "Failed to record a usage sample";
        let added = sqlx::query(
            "INSERT INTO usage_samples (kind, taken_at, used, total) SELECT ?, ?, ?, ? \
             WHERE NOT EXISTS (SELECT 1 FROM usage_samples WHERE kind = ? AND taken_at > ?)",
        )
        .bind(kind)
        .bind(at)
        .bind(used)
        .bind(total)
        .bind(kind)
        .bind(at - min_gap_secs)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, CONTEXT))?
        .rows_affected()
            > 0;
        if added {
            sqlx::query("DELETE FROM usage_samples WHERE taken_at < ?")
                .bind(at - SAMPLE_KEEP_SECS)
                .execute(self.pool)
                .await
                .map_err(tracked(DbOp::Other, CONTEXT))?;
        }
        Ok(added)
    }

    /// The samples of `kind` taken at or after `since` (Unix seconds), oldest first
    pub async fn samples(&self, kind: &str, since: i64) -> DbResult<Vec<UsageSample>> {
        sqlx::query_as::<_, UsageSample>(
            "SELECT taken_at, used, total FROM usage_samples WHERE kind = ? AND taken_at >= ? \
             ORDER BY taken_at",
        )
        .bind(kind)
        .bind(since)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to read usage samples"))
    }

    /// The latest `limit` runs, newest first, each with its files in deletion order
    pub async fn recent_runs(&self, limit: i64) -> DbResult<Vec<StorageRun>> {
        const CONTEXT: &str = "Failed to read storage runs";
        let mut runs = sqlx::query_as::<_, StorageRun>(
            "SELECT id, CAST(finished_at AS TEXT) AS finished_at, trigger, files, bytes, failed, free_after
             FROM storage_runs ORDER BY id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, CONTEXT))?;
        for run in &mut runs {
            run.deleted = sqlx::query_as::<_, StorageRunFile>(
                "SELECT file_id, path, size, created_at, local_path FROM storage_run_files
                 WHERE run_id = ? ORDER BY rowid",
            )
            .bind(run.id)
            .fetch_all(self.pool)
            .await
            .map_err(tracked(DbOp::Other, CONTEXT))?;
        }
        Ok(runs)
    }
}

#[cfg(test)]
mod tests {
    use super::NewRunFile;
    use crate::Database;
    use tempfile::TempDir;

    fn file(id: i64, size: i64) -> NewRunFile {
        NewRunFile {
            file_id: id,
            path: format!("/Movies/{id}.mkv"),
            size,
            created_at: Some("2026-01-01T00:00:00".to_string()),
            local_path: format!("/data/{id}.mkv"),
        }
    }

    #[tokio::test]
    async fn records_runs_newest_first_with_files() {
        let dir = TempDir::new().unwrap();
        let db = Database::new(dir.path().join("test.db")).await.unwrap();
        let repo = db.storage();
        assert!(repo.recent_runs(20).await.unwrap().is_empty());

        let first = repo
            .record_run("manual", &[file(1, 10), file(2, 5)], 0, Some(100))
            .await
            .unwrap();
        let second = repo
            .record_run("schedule", &[file(3, 7)], 1, None)
            .await
            .unwrap();

        let runs = repo.recent_runs(20).await.unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].id, second);
        assert_eq!(runs[0].trigger, "schedule");
        assert_eq!(runs[0].failed, 1);
        assert_eq!(runs[0].free_after, None);
        assert_eq!(runs[1].id, first);
        assert_eq!(runs[1].files, 2);
        assert_eq!(runs[1].bytes, 15);
        assert_eq!(runs[1].free_after, Some(100));
        let ids: Vec<i64> = runs[1].deleted.iter().map(|f| f.file_id).collect();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(runs[1].deleted[0].local_path, "/data/1.mkv");
        assert!(!runs[1].finished_at.is_empty());

        assert_eq!(repo.recent_runs(1).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn samples_are_spaced_listed_oldest_first_and_pruned() {
        let dir = TempDir::new().unwrap();
        let db = Database::new(dir.path().join("test.db")).await.unwrap();
        let repo = db.storage();
        const HOUR: i64 = 3600;
        let t0 = 1_800_000_000;
        assert!(repo.add_sample("putio", t0, 10, 100, HOUR).await.unwrap());
        // Too soon for the same kind, fine for another
        assert!(!repo
            .add_sample("putio", t0 + HOUR - 1, 11, 100, HOUR)
            .await
            .unwrap());
        assert!(repo
            .add_sample("local", t0 + 60, 5, 50, HOUR)
            .await
            .unwrap());
        assert!(repo
            .add_sample("putio", t0 + HOUR, 12, 100, HOUR)
            .await
            .unwrap());

        let putio = repo.samples("putio", 0).await.unwrap();
        assert_eq!(
            putio
                .iter()
                .map(|s| (s.taken_at, s.used, s.total))
                .collect::<Vec<_>>(),
            vec![(t0, 10, 100), (t0 + HOUR, 12, 100)]
        );
        assert_eq!(repo.samples("putio", t0 + 1).await.unwrap().len(), 1);
        assert_eq!(repo.samples("local", 0).await.unwrap().len(), 1);

        // 31 days on, a new sample removes every older one
        let later = t0 + 31 * 86_400;
        assert!(repo.add_sample("local", later, 6, 50, HOUR).await.unwrap());
        assert!(repo.samples("putio", 0).await.unwrap().is_empty());
        assert_eq!(repo.samples("local", 0).await.unwrap().len(), 1);
    }
}
