//! Download repository for managing download records

use super::error::{tracked, DbResult};
use super::models::{Download, DownloadStatus};
use putio_sync_telemetry::DbOp;
use sqlx::SqlitePool;
use tracing::debug;

/// Repository for managing download records
pub struct DownloadRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> DownloadRepository<'a> {
    /// Create a new download repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Insert a new download record
    ///
    /// # Arguments
    ///
    /// * `file_id` - Put.io file ID
    /// * `filename` - Original filename
    /// * `file_size` - File size in bytes
    /// * `local_path` - Local filesystem path
    /// * `status` - Download status
    pub async fn insert(
        &self,
        file_id: i64,
        filename: &str,
        file_size: i64,
        local_path: &str,
        status: DownloadStatus,
    ) -> DbResult<i64> {
        debug!(
            "Inserting download: file_id={}, filename={}, size={}",
            file_id, filename, file_size
        );

        let result = sqlx::query(
            r#"
            INSERT INTO downloads (file_id, filename, file_size, local_path, status)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(file_id)
        .bind(filename)
        .bind(file_size)
        .bind(local_path)
        .bind(status.to_string())
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to insert download"))?;

        Ok(result.last_insert_rowid())
    }

    /// Insert a new download record with retry count
    pub async fn insert_with_retry_count(
        &self,
        file_id: i64,
        filename: &str,
        file_size: i64,
        local_path: &str,
        status: DownloadStatus,
        retry_count: i64,
    ) -> DbResult<i64> {
        debug!(
            "Inserting download: file_id={}, filename={}, size={}, retries={}",
            file_id, filename, file_size, retry_count
        );

        let result = sqlx::query(
            r#"
            INSERT INTO downloads (file_id, filename, file_size, local_path, status, retry_count)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(file_id)
        .bind(filename)
        .bind(file_size)
        .bind(local_path)
        .bind(status.to_string())
        .bind(retry_count)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to insert download"))?;

        Ok(result.last_insert_rowid())
    }

    /// Point a file's downloads at a new path (feature 038: a staged item moved into place)
    pub async fn set_local_path(&self, file_id: i64, local_path: &str) -> DbResult<()> {
        sqlx::query("UPDATE downloads SET local_path = ? WHERE file_id = ?")
            .bind(local_path)
            .bind(file_id)
            .execute(self.pool)
            .await
            .map_err(tracked(DbOp::History, "Failed to move a download's path"))?;
        Ok(())
    }

    /// Find a download by file ID only
    pub async fn find_by_file_id(&self, file_id: i64) -> DbResult<Option<Download>> {
        let download = sqlx::query_as::<_, Download>(
            r#"
            SELECT * FROM downloads
            WHERE file_id = ?
            ORDER BY id DESC
            LIMIT 1
            "#,
        )
        .bind(file_id)
        .fetch_optional(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to find download"))?;

        Ok(download)
    }

    /// The download whose finished file is at `local_path` (feature 025: Local files page
    /// to Put.io id)
    pub async fn find_by_local_path(&self, local_path: &str) -> DbResult<Option<Download>> {
        sqlx::query_as::<_, Download>(
            "SELECT * FROM downloads WHERE local_path = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(local_path)
        .fetch_optional(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to find download by path"))
    }

    /// Find a download by file ID, filename, and size
    pub async fn find_by_file(
        &self,
        file_id: i64,
        filename: &str,
        file_size: i64,
    ) -> DbResult<Option<Download>> {
        let download = sqlx::query_as::<_, Download>(
            r#"
            SELECT * FROM downloads
            WHERE file_id = ? AND filename = ? AND file_size = ?
            "#,
        )
        .bind(file_id)
        .bind(filename)
        .bind(file_size)
        .fetch_optional(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to find download"))?;

        Ok(download)
    }

    /// Get download by ID
    pub async fn get(&self, id: i64) -> DbResult<Option<Download>> {
        let download = sqlx::query_as::<_, Download>(
            r#"
            SELECT * FROM downloads WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to get download"))?;

        Ok(download)
    }

    /// Update download status
    pub async fn update_status(&self, id: i64, status: DownloadStatus) -> DbResult<()> {
        debug!("Updating download {} status to {}", id, status);

        sqlx::query(
            r#"
            UPDATE downloads SET status = ? WHERE id = ?
            "#,
        )
        .bind(status.to_string())
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to update download status"))?;

        Ok(())
    }

    /// List all incomplete downloads (in_progress or failed)
    pub async fn list_incomplete(&self) -> DbResult<Vec<Download>> {
        let downloads = sqlx::query_as::<_, Download>(
            r#"
            SELECT * FROM downloads
            WHERE status IN ('in_progress', 'failed')
            ORDER BY id ASC
            "#,
        )
        .fetch_all(self.pool)
        .await
        .map_err(tracked(
            DbOp::History,
            "Failed to list incomplete downloads",
        ))?;

        Ok(downloads)
    }

    /// Downloads that failed, or are unfinished after at least `min_retries` retries,
    /// leaving out files that later completed (doctor, feature 010)
    pub async fn failing(&self, min_retries: i64) -> DbResult<Vec<Download>> {
        sqlx::query_as::<_, Download>(
            r#"
            SELECT * FROM downloads
            WHERE (status = 'failed' OR (status != 'completed' AND retry_count >= ?))
              AND file_id NOT IN (SELECT file_id FROM downloads WHERE status = 'completed')
            ORDER BY id ASC
            "#,
        )
        .bind(min_retries)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to list failing downloads"))
    }

    /// `(file_id, file_size, local_path)` of every completed download, the storage
    /// planner's record of what is safely on disk (feature 033)
    pub async fn completed(&self) -> DbResult<Vec<(i64, i64, String)>> {
        sqlx::query_as::<_, (i64, i64, String)>(
            "SELECT file_id, file_size, local_path FROM downloads WHERE status = 'completed' ORDER BY id",
        )
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to list completed downloads"))
    }

    /// Record how a download was checked (feature 034). `verified_at` is set to now
    /// unless the method is `none` (verification off).
    pub async fn set_verified(&self, id: i64, crc32: Option<&str>, verify: &str) -> DbResult<()> {
        sqlx::query(
            "UPDATE downloads SET crc32 = ?, verify = ?, \
             verified_at = CASE WHEN ? = 'none' THEN verified_at ELSE CURRENT_TIMESTAMP END \
             WHERE id = ?",
        )
        .bind(crc32)
        .bind(verify)
        .bind(verify)
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to set download verify"))?;
        Ok(())
    }

    /// A verify run found the file matching (feature 034)
    pub async fn touch_verified(&self, file_id: i64, method: &str) -> DbResult<()> {
        sqlx::query(
            "UPDATE downloads SET verify = ?, verified_at = CURRENT_TIMESTAMP \
             WHERE file_id = ? AND status = 'completed'",
        )
        .bind(method)
        .bind(file_id)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to set download verified_at"))?;
        Ok(())
    }

    /// Every completed download, oldest first, for a verify run (feature 034)
    pub async fn completed_rows(&self) -> DbResult<Vec<Download>> {
        sqlx::query_as::<_, Download>(
            "SELECT * FROM downloads WHERE status = 'completed' ORDER BY id",
        )
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to list completed downloads"))
    }

    /// Delete a download record
    pub async fn delete(&self, id: i64) -> DbResult<()> {
        debug!("Deleting download {}", id);

        sqlx::query(
            r#"
            DELETE FROM downloads WHERE id = ?
            "#,
        )
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to delete download"))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use tempfile::TempDir;

    async fn setup_test_db() -> (TempDir, Database) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::new(&db_path).await.unwrap();
        (temp_dir, db)
    }

    #[tokio::test]
    async fn completed_lists_only_completed_rows() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = DownloadRepository::new(db.pool());
        repo.insert(1, "a.mkv", 10, "/d/a.mkv", DownloadStatus::Completed)
            .await
            .unwrap();
        repo.insert(2, "b.mkv", 20, "/d/b.mkv", DownloadStatus::Failed)
            .await
            .unwrap();
        assert_eq!(
            repo.completed().await.unwrap(),
            vec![(1, 10, "/d/a.mkv".to_string())]
        );
    }

    #[tokio::test]
    async fn test_insert_download() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = DownloadRepository::new(db.pool());

        let id = repo
            .insert(
                12345,
                "test.mkv",
                1024 * 1024 * 100,
                "/tmp/test.mkv",
                DownloadStatus::InProgress,
            )
            .await
            .unwrap();

        assert!(id > 0);
    }

    #[tokio::test]
    async fn test_find_by_local_path() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = DownloadRepository::new(db.pool());
        repo.insert(1, "a.mkv", 10, "/d/a.mkv", DownloadStatus::Completed)
            .await
            .unwrap();
        repo.insert(2, "a.mkv", 10, "/d/a.mkv", DownloadStatus::Completed)
            .await
            .unwrap();
        let found = repo.find_by_local_path("/d/a.mkv").await.unwrap().unwrap();
        assert_eq!(found.file_id, 2, "the newest row wins");
        assert!(repo
            .find_by_local_path("/d/none.mkv")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn test_find_by_file() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = DownloadRepository::new(db.pool());

        let file_size = 1024 * 1024 * 100;
        repo.insert(
            12345,
            "test.mkv",
            file_size,
            "/tmp/test.mkv",
            DownloadStatus::Completed,
        )
        .await
        .unwrap();

        let download = repo
            .find_by_file(12345, "test.mkv", file_size)
            .await
            .unwrap();
        assert!(download.is_some());

        let download = download.unwrap();
        assert_eq!(download.file_id, 12345);
        assert_eq!(download.filename, "test.mkv");
        assert_eq!(download.file_size, file_size);
        assert_eq!(download.status, "completed");
    }

    #[tokio::test]
    async fn test_update_status() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = DownloadRepository::new(db.pool());

        let id = repo
            .insert(
                12345,
                "test.mkv",
                1024,
                "/tmp/test.mkv",
                DownloadStatus::InProgress,
            )
            .await
            .unwrap();

        repo.update_status(id, DownloadStatus::Completed)
            .await
            .unwrap();

        let download = repo.get(id).await.unwrap().unwrap();
        assert_eq!(download.status, "completed");
    }

    #[tokio::test]
    async fn test_list_incomplete() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = DownloadRepository::new(db.pool());

        repo.insert(
            1,
            "file1.mkv",
            1024,
            "/tmp/file1.mkv",
            DownloadStatus::Completed,
        )
        .await
        .unwrap();
        repo.insert(
            2,
            "file2.mkv",
            2048,
            "/tmp/file2.mkv",
            DownloadStatus::InProgress,
        )
        .await
        .unwrap();
        repo.insert(
            3,
            "file3.mkv",
            3072,
            "/tmp/file3.mkv",
            DownloadStatus::Failed,
        )
        .await
        .unwrap();

        let incomplete = repo.list_incomplete().await.unwrap();
        assert_eq!(incomplete.len(), 2);
        assert_eq!(incomplete[0].file_id, 2);
        assert_eq!(incomplete[1].file_id, 3);
    }

    #[tokio::test]
    async fn test_delete() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = DownloadRepository::new(db.pool());

        let id = repo
            .insert(
                12345,
                "test.mkv",
                1024,
                "/tmp/test.mkv",
                DownloadStatus::Completed,
            )
            .await
            .unwrap();

        repo.delete(id).await.unwrap();

        let download = repo.get(id).await.unwrap();
        assert!(download.is_none());
    }
}
