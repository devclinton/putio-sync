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
