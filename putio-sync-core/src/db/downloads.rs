//! Download repository for managing download records

use super::models::{Download, DownloadChunk, DownloadStatus};
use crate::error::{CoreError, CoreResult};
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
    ) -> CoreResult<i64> {
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
        .map_err(|e| CoreError::DatabaseError(format!("Failed to insert download: {}", e)))?;

        Ok(result.last_insert_rowid())
    }

    /// Find a download by file ID only
    pub async fn find_by_file_id(&self, file_id: i64) -> CoreResult<Option<Download>> {
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
        .map_err(|e| CoreError::DatabaseError(format!("Failed to find download: {}", e)))?;

        Ok(download)
    }

    /// Find a download by file ID, filename, and size
    pub async fn find_by_file(
        &self,
        file_id: i64,
        filename: &str,
        file_size: i64,
    ) -> CoreResult<Option<Download>> {
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
        .map_err(|e| CoreError::DatabaseError(format!("Failed to find download: {}", e)))?;

        Ok(download)
    }

    /// Get download by ID
    pub async fn get(&self, id: i64) -> CoreResult<Option<Download>> {
        let download = sqlx::query_as::<_, Download>(
            r#"
            SELECT * FROM downloads WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to get download: {}", e)))?;

        Ok(download)
    }

    /// Update download status
    pub async fn update_status(&self, id: i64, status: DownloadStatus) -> CoreResult<()> {
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
        .map_err(|e| CoreError::DatabaseError(format!("Failed to update download status: {}", e)))?;

        Ok(())
    }

    /// List all incomplete downloads (in_progress or failed)
    pub async fn list_incomplete(&self) -> CoreResult<Vec<Download>> {
        let downloads = sqlx::query_as::<_, Download>(
            r#"
            SELECT * FROM downloads
            WHERE status IN ('in_progress', 'failed')
            ORDER BY id ASC
            "#,
        )
        .fetch_all(self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to list incomplete downloads: {}", e)))?;

        Ok(downloads)
    }

    /// Delete a download record
    pub async fn delete(&self, id: i64) -> CoreResult<()> {
        debug!("Deleting download {}", id);

        sqlx::query(
            r#"
            DELETE FROM downloads WHERE id = ?
            "#,
        )
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to delete download: {}", e)))?;

        Ok(())
    }

    /// Insert a chunk for a download
    pub async fn insert_chunk(
        &self,
        download_id: i64,
        chunk_index: i64,
        start_byte: i64,
        end_byte: i64,
    ) -> CoreResult<i64> {
        debug!(
            "Inserting chunk {} for download {} (bytes {}-{})",
            chunk_index, download_id, start_byte, end_byte
        );

        let result = sqlx::query(
            r#"
            INSERT INTO download_chunks (download_id, chunk_index, start_byte, end_byte, status)
            VALUES (?, ?, ?, ?, 'pending')
            "#,
        )
        .bind(download_id)
        .bind(chunk_index)
        .bind(start_byte)
        .bind(end_byte)
        .execute(self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to insert chunk: {}", e)))?;

        Ok(result.last_insert_rowid())
    }

    /// Get all chunks for a download
    pub async fn get_chunks(&self, download_id: i64) -> CoreResult<Vec<DownloadChunk>> {
        let chunks = sqlx::query_as::<_, DownloadChunk>(
            r#"
            SELECT * FROM download_chunks
            WHERE download_id = ?
            ORDER BY chunk_index ASC
            "#,
        )
        .bind(download_id)
        .fetch_all(self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to get chunks: {}", e)))?;

        Ok(chunks)
    }

    /// Update chunk status by chunk ID
    pub async fn update_chunk_status(
        &self,
        chunk_id: i64,
        status: super::models::ChunkStatus,
    ) -> CoreResult<()> {
        debug!("Updating chunk {} status to {}", chunk_id, status);

        sqlx::query(
            r#"
            UPDATE download_chunks SET status = ? WHERE id = ?
            "#,
        )
        .bind(status.to_string())
        .bind(chunk_id)
        .execute(self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to update chunk status: {}", e)))?;

        Ok(())
    }

    /// Update chunk status by download ID and chunk number
    pub async fn update_chunk_status_by_number(
        &self,
        download_id: i64,
        chunk_number: i64,
        status: super::models::ChunkStatus,
    ) -> CoreResult<()> {
        debug!(
            "Updating chunk {} for download {} status to {}",
            chunk_number, download_id, status
        );

        sqlx::query(
            r#"
            UPDATE download_chunks SET status = ?
            WHERE download_id = ? AND chunk_index = ?
            "#,
        )
        .bind(status.to_string())
        .bind(download_id)
        .bind(chunk_number)
        .execute(self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to update chunk status: {}", e)))?;

        Ok(())
    }

    /// Delete all chunks for a download
    pub async fn delete_chunks(&self, download_id: i64) -> CoreResult<()> {
        debug!("Deleting chunks for download {}", download_id);

        sqlx::query(
            r#"
            DELETE FROM download_chunks WHERE download_id = ?
            "#,
        )
        .bind(download_id)
        .execute(self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to delete chunks: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
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
        repo.insert(12345, "test.mkv", file_size, "/tmp/test.mkv", DownloadStatus::Completed)
            .await
            .unwrap();

        let download = repo.find_by_file(12345, "test.mkv", file_size).await.unwrap();
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
            .insert(12345, "test.mkv", 1024, "/tmp/test.mkv", DownloadStatus::InProgress)
            .await
            .unwrap();

        repo.update_status(id, DownloadStatus::Completed).await.unwrap();

        let download = repo.get(id).await.unwrap().unwrap();
        assert_eq!(download.status, "completed");
    }

    #[tokio::test]
    async fn test_list_incomplete() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = DownloadRepository::new(db.pool());

        repo.insert(1, "file1.mkv", 1024, "/tmp/file1.mkv", DownloadStatus::Completed)
            .await
            .unwrap();
        repo.insert(2, "file2.mkv", 2048, "/tmp/file2.mkv", DownloadStatus::InProgress)
            .await
            .unwrap();
        repo.insert(3, "file3.mkv", 3072, "/tmp/file3.mkv", DownloadStatus::Failed)
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
            .insert(12345, "test.mkv", 1024, "/tmp/test.mkv", DownloadStatus::Completed)
            .await
            .unwrap();

        repo.delete(id).await.unwrap();

        let download = repo.get(id).await.unwrap();
        assert!(download.is_none());
    }

    #[tokio::test]
    async fn test_insert_chunk() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = DownloadRepository::new(db.pool());

        let download_id = repo
            .insert(12345, "test.mkv", 1024, "/tmp/test.mkv", DownloadStatus::InProgress)
            .await
            .unwrap();

        let chunk_id = repo.insert_chunk(download_id, 0, 0, 1023).await.unwrap();
        assert!(chunk_id > 0);
    }

    #[tokio::test]
    async fn test_get_chunks() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = DownloadRepository::new(db.pool());

        let download_id = repo
            .insert(12345, "test.mkv", 3072, "/tmp/test.mkv", DownloadStatus::InProgress)
            .await
            .unwrap();

        repo.insert_chunk(download_id, 0, 0, 1023).await.unwrap();
        repo.insert_chunk(download_id, 1, 1024, 2047).await.unwrap();
        repo.insert_chunk(download_id, 2, 2048, 3071).await.unwrap();

        let chunks = repo.get_chunks(download_id).await.unwrap();
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].chunk_index, 0);
        assert_eq!(chunks[1].chunk_index, 1);
        assert_eq!(chunks[2].chunk_index, 2);
    }

    #[tokio::test]
    async fn test_update_chunk_status() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = DownloadRepository::new(db.pool());

        let download_id = repo
            .insert(12345, "test.mkv", 1024, "/tmp/test.mkv", DownloadStatus::InProgress)
            .await
            .unwrap();

        let chunk_id = repo.insert_chunk(download_id, 0, 0, 1023).await.unwrap();

        repo.update_chunk_status(chunk_id, crate::db::models::ChunkStatus::Completed)
            .await
            .unwrap();

        let chunks = repo.get_chunks(download_id).await.unwrap();
        assert_eq!(chunks[0].status, "completed");
    }

    #[tokio::test]
    async fn test_delete_chunks() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = DownloadRepository::new(db.pool());

        let download_id = repo
            .insert(12345, "test.mkv", 2048, "/tmp/test.mkv", DownloadStatus::InProgress)
            .await
            .unwrap();

        repo.insert_chunk(download_id, 0, 0, 1023).await.unwrap();
        repo.insert_chunk(download_id, 1, 1024, 2047).await.unwrap();

        repo.delete_chunks(download_id).await.unwrap();

        let chunks = repo.get_chunks(download_id).await.unwrap();
        assert_eq!(chunks.len(), 0);
    }
}
