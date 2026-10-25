//! Queue repository for managing download queue

use super::error::{DbError, DbResult};
use super::models::{QueueItem, QueueStatus};
use sqlx::SqlitePool;
use tracing::debug;

/// Repository for queue operations
pub struct QueueRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> QueueRepository<'a> {
    /// Create a new queue repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Add an item to the queue
    pub async fn add(
        &self,
        file_id: i64,
        filename: &str,
        file_size: i64,
        download_url: &str,
        relative_path: &str,
        priority: i64,
    ) -> DbResult<i64> {
        debug!(
            "Adding to queue: file_id={}, filename={}, size={}, relative_path={}, priority={}",
            file_id, filename, file_size, relative_path, priority
        );

        let result = sqlx::query(
            r#"
            INSERT INTO download_queue (file_id, filename, file_size, download_url, relative_path, priority, status)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(file_id) DO UPDATE SET
                priority = excluded.priority,
                relative_path = excluded.relative_path,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(file_id)
        .bind(filename)
        .bind(file_size)
        .bind(download_url)
        .bind(relative_path)
        .bind(priority)
        .bind(QueueStatus::Pending.to_string())
        .execute(self.pool)
        .await
        .map_err(|e| DbError::DatabaseError(format!("Failed to add to queue: {}", e)))?;

        Ok(result.last_insert_rowid())
    }

    /// Get all pending items ordered by priority (highest first)
    pub async fn get_pending(&self) -> DbResult<Vec<QueueItem>> {
        let items = sqlx::query_as::<_, QueueItem>(
            r#"
            SELECT * FROM download_queue
            WHERE status = 'pending'
            ORDER BY priority DESC, created_at ASC
            "#,
        )
        .fetch_all(self.pool)
        .await
        .map_err(|e| DbError::DatabaseError(format!("Failed to get pending queue items: {}", e)))?;

        Ok(items)
    }

    /// Atomically claim the next pending item by updating its status to downloading
    ///
    /// This prevents race conditions where multiple workers try to process the same item.
    /// Returns None if no pending items are available.
    pub async fn claim_next_pending(&self) -> DbResult<Option<QueueItem>> {
        // Start a transaction to ensure atomicity
        let mut tx =
            self.pool.begin().await.map_err(|e| {
                DbError::DatabaseError(format!("Failed to begin transaction: {}", e))
            })?;

        // Get the next pending item
        let item = sqlx::query_as::<_, QueueItem>(
            r#"
            SELECT * FROM download_queue
            WHERE status = 'pending'
            ORDER BY priority DESC, created_at ASC
            LIMIT 1
            "#,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| DbError::DatabaseError(format!("Failed to get next pending item: {}", e)))?;

        if let Some(ref queue_item) = item {
            // Update the status to downloading
            sqlx::query(
                r#"
                UPDATE download_queue
                SET status = 'downloading', updated_at = CURRENT_TIMESTAMP
                WHERE file_id = ?
                "#,
            )
            .bind(queue_item.file_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::DatabaseError(format!("Failed to update item status: {}", e)))?;
        }

        // Commit the transaction
        tx.commit()
            .await
            .map_err(|e| DbError::DatabaseError(format!("Failed to commit transaction: {}", e)))?;

        Ok(item)
    }

    /// Get all queue items
    pub async fn get_all(&self) -> DbResult<Vec<QueueItem>> {
        let items = sqlx::query_as::<_, QueueItem>(
            r#"
            SELECT * FROM download_queue
            ORDER BY priority DESC, created_at ASC
            "#,
        )
        .fetch_all(self.pool)
        .await
        .map_err(|e| DbError::DatabaseError(format!("Failed to get queue items: {}", e)))?;

        Ok(items)
    }

    /// Get a queue item by file_id
    pub async fn get_by_file_id(&self, file_id: i64) -> DbResult<Option<QueueItem>> {
        let item = sqlx::query_as::<_, QueueItem>(
            r#"
            SELECT * FROM download_queue
            WHERE file_id = ?
            "#,
        )
        .bind(file_id)
        .fetch_optional(self.pool)
        .await
        .map_err(|e| {
            DbError::DatabaseError(format!("Failed to get queue item by file_id: {}", e))
        })?;

        Ok(item)
    }

    /// Update queue item status
    pub async fn update_status(&self, file_id: i64, status: QueueStatus) -> DbResult<()> {
        sqlx::query(
            r#"
            UPDATE download_queue
            SET status = ?, updated_at = CURRENT_TIMESTAMP
            WHERE file_id = ?
            "#,
        )
        .bind(status.to_string())
        .bind(file_id)
        .execute(self.pool)
        .await
        .map_err(|e| DbError::DatabaseError(format!("Failed to update queue status: {}", e)))?;

        Ok(())
    }

    /// Update queue item priority
    pub async fn update_priority(&self, file_id: i64, priority: i64) -> DbResult<()> {
        sqlx::query(
            r#"
            UPDATE download_queue
            SET priority = ?, updated_at = CURRENT_TIMESTAMP
            WHERE file_id = ?
            "#,
        )
        .bind(priority)
        .bind(file_id)
        .execute(self.pool)
        .await
        .map_err(|e| DbError::DatabaseError(format!("Failed to update queue priority: {}", e)))?;

        Ok(())
    }

    /// Pause all items in the queue
    pub async fn pause_all(&self) -> DbResult<usize> {
        let result = sqlx::query(
            r#"
            UPDATE download_queue
            SET status = 'paused', updated_at = CURRENT_TIMESTAMP
            WHERE status IN ('pending', 'downloading')
            "#,
        )
        .execute(self.pool)
        .await
        .map_err(|e| DbError::DatabaseError(format!("Failed to pause queue: {}", e)))?;

        Ok(result.rows_affected() as usize)
    }

    /// Resume all paused items in the queue
    pub async fn resume_all(&self) -> DbResult<usize> {
        let result = sqlx::query(
            r#"
            UPDATE download_queue
            SET status = 'pending', updated_at = CURRENT_TIMESTAMP
            WHERE status = 'paused'
            "#,
        )
        .execute(self.pool)
        .await
        .map_err(|e| DbError::DatabaseError(format!("Failed to resume queue: {}", e)))?;

        Ok(result.rows_affected() as usize)
    }

    /// Remove item from queue
    pub async fn remove(&self, file_id: i64) -> DbResult<()> {
        sqlx::query(
            r#"
            DELETE FROM download_queue
            WHERE file_id = ?
            "#,
        )
        .bind(file_id)
        .execute(self.pool)
        .await
        .map_err(|e| DbError::DatabaseError(format!("Failed to remove from queue: {}", e)))?;

        Ok(())
    }

    /// Clear completed items from queue
    pub async fn clear_completed(&self) -> DbResult<usize> {
        let result = sqlx::query(
            r#"
            DELETE FROM download_queue
            WHERE status = 'completed'
            "#,
        )
        .execute(self.pool)
        .await
        .map_err(|e| DbError::DatabaseError(format!("Failed to clear completed items: {}", e)))?;

        Ok(result.rows_affected() as usize)
    }

    /// Get queue statistics
    pub async fn get_stats(&self) -> DbResult<QueueStats> {
        let stats = sqlx::query_as::<_, QueueStats>(
            r#"
            SELECT
                COUNT(*) as total,
                SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END) as pending,
                SUM(CASE WHEN status = 'downloading' THEN 1 ELSE 0 END) as downloading,
                SUM(CASE WHEN status = 'paused' THEN 1 ELSE 0 END) as paused,
                SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed,
                SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) as failed
            FROM download_queue
            "#,
        )
        .fetch_one(self.pool)
        .await
        .map_err(|e| DbError::DatabaseError(format!("Failed to get queue stats: {}", e)))?;

        Ok(stats)
    }
}

/// Queue statistics
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct QueueStats {
    /// Total number of items in queue
    pub total: i64,
    /// Number of pending items
    pub pending: i64,
    /// Number of downloading items
    pub downloading: i64,
    /// Number of paused items
    pub paused: i64,
    /// Number of completed items
    pub completed: i64,
    /// Number of failed items
    pub failed: i64,
}
