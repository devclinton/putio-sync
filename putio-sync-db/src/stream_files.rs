//! Files kept on Put.io by stream mode (feature 022) and the arrived steps each still
//! needs. A row is added the first time a sync pass decides a file is streamed.

use super::error::{tracked, DbResult};
use putio_sync_telemetry::DbOp;
use sqlx::{Row, SqlitePool};
use std::collections::HashSet;

/// One streamed file
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamFile {
    pub file_id: i64,
    pub filename: String,
    pub folder_path: String,
    pub size: i64,
    /// Steps still to run, in order
    pub pending: Vec<String>,
    pub attempts: i64,
    pub last_error: Option<String>,
}

impl StreamFile {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Self {
        Self {
            file_id: row.get("file_id"),
            filename: row.get("filename"),
            folder_path: row.get("folder_path"),
            size: row.get("size"),
            pending: split(row.get("pending")),
            attempts: row.get("attempts"),
            last_error: row.get("last_error"),
        }
    }
}

fn split(pending: String) -> Vec<String> {
    pending.split_whitespace().map(str::to_string).collect()
}

/// Repository for `stream_files`
pub struct StreamFilesRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> StreamFilesRepository<'a> {
    /// Create a new stream files repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Ids of every recorded file
    pub async fn ids(&self) -> DbResult<HashSet<i64>> {
        let rows = sqlx::query("SELECT file_id FROM stream_files")
            .fetch_all(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to list stream files"))?;
        Ok(rows.iter().map(|r| r.get("file_id")).collect())
    }

    /// Record a newly streamed file with the steps it needs. A file already recorded is
    /// left as it is.
    pub async fn add(
        &self,
        file_id: i64,
        filename: &str,
        folder_path: &str,
        size: i64,
        steps: &[&str],
    ) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT INTO stream_files (file_id, filename, folder_path, size, pending)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(file_id) DO NOTHING
            "#,
        )
        .bind(file_id)
        .bind(filename)
        .bind(folder_path)
        .bind(size)
        .bind(steps.join(" "))
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to record stream file"))?;
        Ok(())
    }

    /// Files with steps still to run, oldest first
    pub async fn pending(&self, limit: i64) -> DbResult<Vec<StreamFile>> {
        let rows = sqlx::query(
            r#"
            SELECT file_id, filename, folder_path, size, pending, attempts, last_error
            FROM stream_files WHERE pending != ''
            ORDER BY first_seen, file_id LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to list pending stream files"))?;
        Ok(rows.iter().map(StreamFile::from_row).collect())
    }

    /// Every recorded file, oldest first, at most `limit` (feature 023's scan)
    pub async fn all(&self, limit: i64) -> DbResult<Vec<StreamFile>> {
        let rows = sqlx::query(
            r#"
            SELECT file_id, filename, folder_path, size, pending, attempts, last_error
            FROM stream_files ORDER BY first_seen, file_id LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to list stream files"))?;
        Ok(rows.iter().map(StreamFile::from_row).collect())
    }

    /// One recorded file
    pub async fn get(&self, file_id: i64) -> DbResult<Option<StreamFile>> {
        let row = sqlx::query(
            r#"
            SELECT file_id, filename, folder_path, size, pending, attempts, last_error
            FROM stream_files WHERE file_id = ?
            "#,
        )
        .bind(file_id)
        .fetch_optional(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to get stream file"))?;
        Ok(row.as_ref().map(StreamFile::from_row))
    }

    /// A step finished: drop it from the file's pending steps
    pub async fn finish_step(&self, file_id: i64, step: &str) -> DbResult<()> {
        let Some(file) = self.get(file_id).await? else {
            return Ok(());
        };
        let rest: Vec<&str> = file
            .pending
            .iter()
            .map(String::as_str)
            .filter(|s| *s != step)
            .collect();
        sqlx::query("UPDATE stream_files SET pending = ?, last_error = NULL WHERE file_id = ?")
            .bind(rest.join(" "))
            .bind(file_id)
            .execute(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to update stream file"))?;
        Ok(())
    }

    /// A step failed: keep it for the next pass and note why
    pub async fn retry_step(&self, file_id: i64, error: &str) -> DbResult<()> {
        sqlx::query(
            "UPDATE stream_files SET attempts = attempts + 1, last_error = ? WHERE file_id = ?",
        )
        .bind(error)
        .bind(file_id)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to update stream file"))?;
        Ok(())
    }

    /// Forget files no longer streamed (deleted on Put.io, moved out or downloaded now)
    pub async fn remove(&self, file_ids: &[i64]) -> DbResult<u64> {
        let mut removed = 0;
        for chunk in file_ids.chunks(500) {
            let marks = vec!["?"; chunk.len()].join(",");
            let sql = format!("DELETE FROM stream_files WHERE file_id IN ({marks})");
            let mut query = sqlx::query(&sql);
            for id in chunk {
                query = query.bind(id);
            }
            removed += query
                .execute(self.pool)
                .await
                .map_err(tracked(DbOp::Other, "Failed to remove stream files"))?
                .rows_affected();
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use crate::Database;
    use tempfile::TempDir;

    async fn setup_test_db() -> (TempDir, Database) {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("test.db"))
            .await
            .unwrap();
        (temp_dir, db)
    }

    #[tokio::test]
    async fn add_once_then_run_steps_in_order() {
        let (_dir, db) = setup_test_db().await;
        let repo = db.stream_files();
        repo.add(7, "ep1.mkv", "Shows", 5000, &["mp4", "tv"])
            .await
            .unwrap();
        // A second sight changes nothing
        repo.add(7, "renamed.mkv", "Other", 1, &[]).await.unwrap();
        let file = repo.get(7).await.unwrap().unwrap();
        assert_eq!(file.filename, "ep1.mkv");
        assert_eq!(file.pending, ["mp4", "tv"]);
        assert_eq!(
            repo.ids().await.unwrap().into_iter().collect::<Vec<_>>(),
            [7]
        );

        repo.retry_step(7, "Put.io said 500").await.unwrap();
        let file = repo.get(7).await.unwrap().unwrap();
        assert_eq!(file.attempts, 1);
        assert_eq!(file.last_error.as_deref(), Some("Put.io said 500"));
        assert_eq!(file.pending, ["mp4", "tv"]);

        repo.finish_step(7, "mp4").await.unwrap();
        let file = repo.get(7).await.unwrap().unwrap();
        assert_eq!(file.pending, ["tv"]);
        assert_eq!(file.last_error, None);
        repo.finish_step(7, "tv").await.unwrap();
        assert!(repo.pending(10).await.unwrap().is_empty());
        // Still known, so it is not recorded again
        assert!(repo.ids().await.unwrap().contains(&7));
    }

    #[tokio::test]
    async fn pending_is_bounded_and_files_can_be_removed() {
        let (_dir, db) = setup_test_db().await;
        let repo = db.stream_files();
        for id in 1..=5 {
            repo.add(id, "a.mkv", "", 1, &["mp4"]).await.unwrap();
        }
        repo.add(6, "b.srt", "", 1, &[]).await.unwrap();
        let ids: Vec<i64> = repo
            .pending(3)
            .await
            .unwrap()
            .iter()
            .map(|f| f.file_id)
            .collect();
        assert_eq!(ids, [1, 2, 3]);
        assert_eq!(repo.all(10).await.unwrap().len(), 6);
        assert_eq!(repo.remove(&[1, 6, 99]).await.unwrap(), 2);
        assert_eq!(repo.ids().await.unwrap().len(), 4);
        assert_eq!(repo.remove(&[]).await.unwrap(), 0);
    }
}
