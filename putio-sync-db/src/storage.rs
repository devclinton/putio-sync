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

/// Repository for `storage_runs` and `storage_run_files`
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
}
