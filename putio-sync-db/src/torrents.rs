//! Processed torrents repository for tracking uploaded torrents

use super::models::ProcessedTorrent;
use super::error::{DbError, DbResult};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::path::Path;
use tracing::{debug, info};

/// Repository for managing processed torrents
pub struct ProcessedTorrentsRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> ProcessedTorrentsRepository<'a> {
    /// Create a new ProcessedTorrentsRepository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Check if a torrent file has been processed by checksum
    pub async fn is_processed(&self, checksum: &str) -> DbResult<bool> {
        let result = sqlx::query("SELECT COUNT(*) as count FROM processed_torrents WHERE file_checksum = ?")
            .bind(checksum)
            .fetch_one(self.pool)
            .await
            .map_err(|e| DbError::DatabaseError(format!("Failed to check processed torrent: {}", e)))?;

        let count: i64 = result.get("count");
        Ok(count > 0)
    }

    /// Record a processed torrent file
    pub async fn record_processed(
        &self,
        file_path: &str,
        checksum: &str,
        file_size: i64,
        magnet_link: Option<&str>,
    ) -> DbResult<()> {
        let query = r#"
            INSERT INTO processed_torrents (file_path, file_checksum, file_size, magnet_link)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(file_checksum) DO UPDATE SET 
                processed_at = CURRENT_TIMESTAMP
        "#;

        sqlx::query(query)
            .bind(file_path)
            .bind(checksum)
            .bind(file_size)
            .bind(magnet_link)
            .execute(self.pool)
            .await
            .map_err(|e| DbError::DatabaseError(format!("Failed to record processed torrent: {}", e)))?;

        debug!("Recorded processed torrent: {} (checksum: {})", file_path, checksum);
        Ok(())
    }

    /// Get all processed torrents
    pub async fn get_all(&self) -> DbResult<Vec<ProcessedTorrent>> {
        let torrents = sqlx::query_as::<_, ProcessedTorrent>(
            "SELECT id, file_path, file_checksum, file_size, magnet_link, processed_at FROM processed_torrents ORDER BY processed_at DESC"
        )
        .fetch_all(self.pool)
        .await
        .map_err(|e| DbError::DatabaseError(format!("Failed to fetch processed torrents: {}", e)))?;

        Ok(torrents)
    }

    /// Get processed torrents count
    pub async fn count(&self) -> DbResult<i64> {
        let result = sqlx::query("SELECT COUNT(*) as count FROM processed_torrents")
            .fetch_one(self.pool)
            .await
            .map_err(|e| DbError::DatabaseError(format!("Failed to count processed torrents: {}", e)))?;

        Ok(result.get("count"))
    }

    /// Clean up old processed torrent records (older than specified days)
    pub async fn cleanup_old(&self, days: u32) -> DbResult<u64> {
        let query = "DELETE FROM processed_torrents WHERE processed_at < datetime('now', '-' || ? || ' days')";
        
        let result = sqlx::query(query)
            .bind(days)
            .execute(self.pool)
            .await
            .map_err(|e| DbError::DatabaseError(format!("Failed to cleanup old processed torrents: {}", e)))?;

        let rows_deleted = result.rows_affected();
        info!("Cleaned up {} old processed torrent records", rows_deleted);
        
        Ok(rows_deleted)
    }
}

/// Calculate SHA-256 checksum of file contents
pub async fn calculate_file_checksum<P: AsRef<Path>>(path: P) -> DbResult<String> {
    let contents = tokio::fs::read(path.as_ref())
        .await
        .map_err(DbError::IoError)?;

    let hash = Sha256::digest(&contents);
    Ok(format!("{:x}", hash))
}

/// Calculate SHA-256 checksum of string content (for magnet links)
pub fn calculate_string_checksum(content: &str) -> String {
    let hash = Sha256::digest(content.as_bytes());
    format!("{:x}", hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_processed_torrents_repository() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = Database::new(&db_path).await.unwrap();
        let repo = db.processed_torrents();

        // Test recording and checking processed torrents
        let checksum = "abcd1234";
        assert!(!repo.is_processed(checksum).await.unwrap());

        repo.record_processed("test.torrent", checksum, 1024, None)
            .await
            .unwrap();

        assert!(repo.is_processed(checksum).await.unwrap());

        // Test getting all processed torrents
        let torrents = repo.get_all().await.unwrap();
        assert_eq!(torrents.len(), 1);
        assert_eq!(torrents[0].file_checksum, checksum);

        // Test count
        let count = repo.count().await.unwrap();
        assert_eq!(count, 1);

        db.close().await.unwrap();
    }

    #[tokio::test]
    async fn test_checksum_calculation() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.torrent");
        fs::write(&file_path, b"test torrent content").unwrap();

        let checksum = calculate_file_checksum(&file_path).await.unwrap();
        assert!(!checksum.is_empty());
        assert_eq!(checksum.len(), 64); // SHA-256 hex length

        // String checksum
        let str_checksum = calculate_string_checksum("magnet:?xt=urn:btih:test");
        assert!(!str_checksum.is_empty());
        assert_eq!(str_checksum.len(), 64);
    }
}