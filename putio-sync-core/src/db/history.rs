//! History repository for managing download and delete history

use super::models::{DownloadHistory, HistoryAction};
use crate::error::{CoreError, CoreResult};
use sqlx::SqlitePool;
use tracing::debug;

/// Repository for managing history records
pub struct HistoryRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> HistoryRepository<'a> {
    /// Create a new history repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Insert a new history record
    ///
    /// # Arguments
    ///
    /// * `file_id` - Put.io file ID
    /// * `filename` - Original filename
    /// * `file_size` - File size in bytes
    /// * `action` - Action performed (downloaded or deleted)
    /// * `duration_seconds` - Optional duration in seconds (for downloads)
    /// * `average_speed` - Optional average speed in bytes/sec (for downloads)
    pub async fn insert(
        &self,
        file_id: i64,
        filename: &str,
        file_size: i64,
        action: HistoryAction,
        duration_seconds: Option<i64>,
        average_speed: Option<i64>,
    ) -> CoreResult<i64> {
        // Extract file extension (normalized to lowercase)
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase());

        debug!(
            "Inserting history: file_id={}, filename={}, action={}",
            file_id, filename, action
        );

        let result = sqlx::query(
            r#"
            INSERT INTO download_history (file_id, filename, file_size, extension, action, duration_seconds, average_speed)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(file_id)
        .bind(filename)
        .bind(file_size)
        .bind(extension)
        .bind(action.to_string())
        .bind(duration_seconds)
        .bind(average_speed)
        .execute(self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("Failed to insert history: {}", e)))?;

        Ok(result.last_insert_rowid())
    }

    /// Query history with optional filters
    ///
    /// # Arguments
    ///
    /// * `action` - Optional action filter (downloaded or deleted)
    /// * `limit` - Maximum number of results (default: 50)
    /// * `offset` - Number of results to skip (for pagination)
    pub async fn query(
        &self,
        action: Option<HistoryAction>,
        limit: i64,
        offset: i64,
    ) -> CoreResult<Vec<DownloadHistory>> {
        let history = match action {
            Some(action) => {
                sqlx::query_as::<_, DownloadHistory>(
                    r#"
                    SELECT * FROM download_history
                    WHERE action = ?
                    ORDER BY timestamp DESC
                    LIMIT ? OFFSET ?
                    "#,
                )
                .bind(action.to_string())
                .bind(limit)
                .bind(offset)
                .fetch_all(self.pool)
                .await
            }
            None => {
                sqlx::query_as::<_, DownloadHistory>(
                    r#"
                    SELECT * FROM download_history
                    ORDER BY timestamp DESC
                    LIMIT ? OFFSET ?
                    "#,
                )
                .bind(limit)
                .bind(offset)
                .fetch_all(self.pool)
                .await
            }
        }
        .map_err(|e| CoreError::DatabaseError(format!("Failed to query history: {}", e)))?;

        Ok(history)
    }

    /// Query history with date range filter
    ///
    /// # Arguments
    ///
    /// * `since` - Optional start date (ISO 8601 format)
    /// * `until` - Optional end date (ISO 8601 format)
    /// * `action` - Optional action filter
    /// * `limit` - Maximum number of results
    /// * `offset` - Number of results to skip
    pub async fn query_by_date(
        &self,
        since: Option<&str>,
        until: Option<&str>,
        action: Option<HistoryAction>,
        limit: i64,
        offset: i64,
    ) -> CoreResult<Vec<DownloadHistory>> {
        let mut query = String::from("SELECT * FROM download_history WHERE 1=1");
        let mut bindings: Vec<String> = Vec::new();

        if let Some(since_date) = since {
            query.push_str(" AND timestamp >= ?");
            bindings.push(since_date.to_string());
        }

        if let Some(until_date) = until {
            query.push_str(" AND timestamp <= ?");
            bindings.push(until_date.to_string());
        }

        if let Some(action_filter) = &action {
            query.push_str(" AND action = ?");
            bindings.push(action_filter.to_string());
        }

        query.push_str(" ORDER BY timestamp DESC LIMIT ? OFFSET ?");

        let mut q = sqlx::query_as::<_, DownloadHistory>(&query);
        for binding in &bindings {
            q = q.bind(binding);
        }
        q = q.bind(limit).bind(offset);

        let history = q
            .fetch_all(self.pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to query history by date: {}", e)))?;

        Ok(history)
    }

    /// Count total history entries
    ///
    /// # Arguments
    ///
    /// * `action` - Optional action filter
    pub async fn count(&self, action: Option<HistoryAction>) -> CoreResult<i64> {
        let count: (i64,) = match action {
            Some(action) => {
                sqlx::query_as(
                    r#"
                    SELECT COUNT(*) FROM download_history WHERE action = ?
                    "#,
                )
                .bind(action.to_string())
                .fetch_one(self.pool)
                .await
            }
            None => {
                sqlx::query_as(
                    r#"
                    SELECT COUNT(*) FROM download_history
                    "#,
                )
                .fetch_one(self.pool)
                .await
            }
        }
        .map_err(|e| CoreError::DatabaseError(format!("Failed to count history: {}", e)))?;

        Ok(count.0)
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
    async fn test_insert_history() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());

        let id = repo
            .insert(
                12345,
                "test.mkv",
                1024 * 1024 * 100,
                HistoryAction::Downloaded,
                Some(120),
                Some(850000),
            )
            .await
            .unwrap();

        assert!(id > 0);
    }

    #[tokio::test]
    async fn test_query_history() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());

        repo.insert(1, "file1.mkv", 1024, HistoryAction::Downloaded, Some(10), Some(100))
            .await
            .unwrap();
        repo.insert(2, "file2.mp4", 2048, HistoryAction::Downloaded, Some(20), Some(100))
            .await
            .unwrap();
        repo.insert(3, "file3.txt", 512, HistoryAction::Deleted, None, None)
            .await
            .unwrap();

        let all = repo.query(None, 50, 0).await.unwrap();
        assert_eq!(all.len(), 3);

        let downloaded = repo.query(Some(HistoryAction::Downloaded), 50, 0).await.unwrap();
        assert_eq!(downloaded.len(), 2);

        let deleted = repo.query(Some(HistoryAction::Deleted), 50, 0).await.unwrap();
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].file_id, 3);
    }

    #[tokio::test]
    async fn test_query_pagination() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());

        for i in 1..=10 {
            repo.insert(i, &format!("file{}.mkv", i), 1024, HistoryAction::Downloaded, None, None)
                .await
                .unwrap();
        }

        let page1 = repo.query(None, 5, 0).await.unwrap();
        assert_eq!(page1.len(), 5);

        let page2 = repo.query(None, 5, 5).await.unwrap();
        assert_eq!(page2.len(), 5);

        // Ensure pages are different
        assert_ne!(page1[0].file_id, page2[0].file_id);
    }

    #[tokio::test]
    async fn test_count() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());

        repo.insert(1, "file1.mkv", 1024, HistoryAction::Downloaded, None, None)
            .await
            .unwrap();
        repo.insert(2, "file2.mkv", 2048, HistoryAction::Downloaded, None, None)
            .await
            .unwrap();
        repo.insert(3, "file3.txt", 512, HistoryAction::Deleted, None, None)
            .await
            .unwrap();

        let total = repo.count(None).await.unwrap();
        assert_eq!(total, 3);

        let downloaded = repo.count(Some(HistoryAction::Downloaded)).await.unwrap();
        assert_eq!(downloaded, 2);

        let deleted = repo.count(Some(HistoryAction::Deleted)).await.unwrap();
        assert_eq!(deleted, 1);
    }

    #[tokio::test]
    async fn test_extension_extraction() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());

        repo.insert(1, "file.MKV", 1024, HistoryAction::Downloaded, None, None)
            .await
            .unwrap();

        let history = repo.query(None, 1, 0).await.unwrap();
        assert_eq!(history[0].extension, Some("mkv".to_string()));
    }
}
