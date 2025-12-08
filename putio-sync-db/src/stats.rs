//! Statistics repository for managing download statistics

use super::error::{DbError, DbResult};
use super::models::DownloadStats;
use sqlx::SqlitePool;
use tracing::debug;

/// Repository for managing download statistics
pub struct StatsRepository<'a> {
    pool: &'a SqlitePool,
}

/// Statistics by file extension
#[derive(Debug, Clone)]
pub struct ExtensionStats {
    /// File extension (normalized to lowercase)
    pub extension: String,
    /// Number of files with this extension
    pub file_count: i64,
    /// Total bytes for this extension
    pub total_bytes: i64,
}

impl<'a> StatsRepository<'a> {
    /// Create a new stats repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Get the current statistics
    pub async fn get(&self) -> DbResult<DownloadStats> {
        let stats = sqlx::query_as::<_, DownloadStats>(
            r#"
            SELECT * FROM download_stats WHERE id = 1
            "#,
        )
        .fetch_one(self.pool)
        .await
        .map_err(|e| DbError::DatabaseError(format!("Failed to get stats: {}", e)))?;

        Ok(stats)
    }

    /// Update statistics after a download
    ///
    /// # Arguments
    ///
    /// * `bytes` - Number of bytes downloaded
    /// * `duration_seconds` - Duration of download in seconds
    pub async fn update_download(&self, bytes: i64, duration_seconds: i64) -> DbResult<()> {
        debug!(
            "Updating download stats: bytes={}, duration={}s",
            bytes, duration_seconds
        );

        sqlx::query(
            r#"
            UPDATE download_stats
            SET total_downloads = total_downloads + 1,
                total_bytes = total_bytes + ?,
                total_time_seconds = total_time_seconds + ?,
                last_updated = CURRENT_TIMESTAMP
            WHERE id = 1
            "#,
        )
        .bind(bytes)
        .bind(duration_seconds)
        .execute(self.pool)
        .await
        .map_err(|e| DbError::DatabaseError(format!("Failed to update download stats: {}", e)))?;

        Ok(())
    }

    /// Get statistics by file extension
    ///
    /// # Arguments
    ///
    /// * `since` - Optional start date (ISO 8601 format)
    /// * `until` - Optional end date (ISO 8601 format)
    pub async fn get_by_extension(
        &self,
        since: Option<&str>,
        until: Option<&str>,
    ) -> DbResult<Vec<ExtensionStats>> {
        let mut query = String::from(
            r#"
            SELECT
                COALESCE(extension, 'unknown') as extension,
                COUNT(*) as file_count,
                SUM(file_size) as total_bytes
            FROM download_history
            WHERE action = 'downloaded'
            "#,
        );

        let mut bindings: Vec<String> = Vec::new();

        if let Some(since_date) = since {
            query.push_str(" AND timestamp >= ?");
            bindings.push(since_date.to_string());
        }

        if let Some(until_date) = until {
            query.push_str(" AND timestamp <= ?");
            bindings.push(until_date.to_string());
        }

        query.push_str(
            r#"
            GROUP BY extension
            ORDER BY total_bytes DESC
            "#,
        );

        let mut q = sqlx::query_as::<_, (String, i64, i64)>(&query);
        for binding in &bindings {
            q = q.bind(binding);
        }

        let results = q
            .fetch_all(self.pool)
            .await
            .map_err(|e| DbError::DatabaseError(format!("Failed to get extension stats: {}", e)))?;

        let stats = results
            .into_iter()
            .map(|(extension, file_count, total_bytes)| ExtensionStats {
                extension,
                file_count,
                total_bytes,
            })
            .collect();

        Ok(stats)
    }

    /// Get total statistics for a date range
    ///
    /// # Arguments
    ///
    /// * `since` - Optional start date (ISO 8601 format)
    /// * `until` - Optional end date (ISO 8601 format)
    pub async fn get_for_range(
        &self,
        since: Option<&str>,
        until: Option<&str>,
    ) -> DbResult<(i64, i64, i64)> {
        let mut query = String::from(
            r#"
            SELECT
                COUNT(*) as download_count,
                COALESCE(SUM(file_size), 0) as total_bytes,
                COALESCE(SUM(duration_seconds), 0) as total_seconds
            FROM download_history
            WHERE action = 'downloaded'
            "#,
        );

        let mut bindings: Vec<String> = Vec::new();

        if let Some(since_date) = since {
            query.push_str(" AND timestamp >= ?");
            bindings.push(since_date.to_string());
        }

        if let Some(until_date) = until {
            query.push_str(" AND timestamp <= ?");
            bindings.push(until_date.to_string());
        }

        let mut q = sqlx::query_as::<_, (i64, i64, i64)>(&query);
        for binding in &bindings {
            q = q.bind(binding);
        }

        let result = q
            .fetch_one(self.pool)
            .await
            .map_err(|e| DbError::DatabaseError(format!("Failed to get range stats: {}", e)))?;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{history::HistoryRepository, models::HistoryAction, Database};
    use tempfile::TempDir;

    async fn setup_test_db() -> (TempDir, Database) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::new(&db_path).await.unwrap();
        (temp_dir, db)
    }

    #[tokio::test]
    async fn test_get_stats() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = StatsRepository::new(db.pool());

        let stats = repo.get().await.unwrap();
        assert_eq!(stats.id, 1);
        assert_eq!(stats.total_downloads, 0);
        assert_eq!(stats.total_bytes, 0);
        assert_eq!(stats.total_time_seconds, 0);
    }

    #[tokio::test]
    async fn test_update_download() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = StatsRepository::new(db.pool());

        repo.update_download(1024 * 1024 * 100, 120).await.unwrap();

        let stats = repo.get().await.unwrap();
        assert_eq!(stats.total_downloads, 1);
        assert_eq!(stats.total_bytes, 1024 * 1024 * 100);
        assert_eq!(stats.total_time_seconds, 120);
    }

    #[tokio::test]
    async fn test_update_multiple_downloads() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = StatsRepository::new(db.pool());

        repo.update_download(1024, 10).await.unwrap();
        repo.update_download(2048, 20).await.unwrap();
        repo.update_download(4096, 30).await.unwrap();

        let stats = repo.get().await.unwrap();
        assert_eq!(stats.total_downloads, 3);
        assert_eq!(stats.total_bytes, 1024 + 2048 + 4096);
        assert_eq!(stats.total_time_seconds, 10 + 20 + 30);
    }

    #[tokio::test]
    async fn test_get_by_extension() {
        let (_temp_dir, db) = setup_test_db().await;
        let history_repo = HistoryRepository::new(db.pool());
        let stats_repo = StatsRepository::new(db.pool());

        // Insert some history entries with different extensions
        history_repo
            .insert(
                1,
                "file1.mkv",
                1024 * 1024 * 100,
                HistoryAction::Downloaded,
                Some(10),
                Some(100),
            )
            .await
            .unwrap();
        history_repo
            .insert(
                2,
                "file2.mkv",
                1024 * 1024 * 200,
                HistoryAction::Downloaded,
                Some(20),
                Some(100),
            )
            .await
            .unwrap();
        history_repo
            .insert(
                3,
                "file3.mp4",
                1024 * 1024 * 50,
                HistoryAction::Downloaded,
                Some(5),
                Some(100),
            )
            .await
            .unwrap();
        history_repo
            .insert(
                4,
                "file4.pdf",
                1024 * 10,
                HistoryAction::Downloaded,
                Some(1),
                Some(100),
            )
            .await
            .unwrap();

        let stats = stats_repo.get_by_extension(None, None).await.unwrap();

        // Should have 3 extensions
        assert_eq!(stats.len(), 3);

        // MKV should be first (largest total size)
        assert_eq!(stats[0].extension, "mkv");
        assert_eq!(stats[0].file_count, 2);
        assert_eq!(stats[0].total_bytes, 1024 * 1024 * 300);

        // MP4 should be second
        assert_eq!(stats[1].extension, "mp4");
        assert_eq!(stats[1].file_count, 1);
        assert_eq!(stats[1].total_bytes, 1024 * 1024 * 50);

        // PDF should be third
        assert_eq!(stats[2].extension, "pdf");
        assert_eq!(stats[2].file_count, 1);
        assert_eq!(stats[2].total_bytes, 1024 * 10);
    }

    #[tokio::test]
    async fn test_get_for_range() {
        let (_temp_dir, db) = setup_test_db().await;
        let history_repo = HistoryRepository::new(db.pool());
        let stats_repo = StatsRepository::new(db.pool());

        history_repo
            .insert(
                1,
                "file1.mkv",
                1024,
                HistoryAction::Downloaded,
                Some(10),
                Some(100),
            )
            .await
            .unwrap();
        history_repo
            .insert(
                2,
                "file2.mkv",
                2048,
                HistoryAction::Downloaded,
                Some(20),
                Some(100),
            )
            .await
            .unwrap();
        history_repo
            .insert(3, "file3.txt", 512, HistoryAction::Deleted, None, None)
            .await
            .unwrap();

        let (count, bytes, seconds) = stats_repo.get_for_range(None, None).await.unwrap();
        assert_eq!(count, 2); // Only downloaded files
        assert_eq!(bytes, 1024 + 2048);
        assert_eq!(seconds, 10 + 20);
    }
}
