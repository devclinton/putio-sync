//! Queue repository for managing download queue

use super::error::{tracked, DbResult};
use super::models::{QueueItem, QueueStatus};
use putio_sync_telemetry::DbOp;
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
        .map_err(tracked(DbOp::Queue, "Failed to add to queue"))?;

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
        .map_err(tracked(DbOp::Queue, "Failed to get pending queue items"))?;

        Ok(items)
    }

    /// Atomically claim the next pending item by updating its status to downloading
    ///
    /// This prevents race conditions where multiple workers try to process the same item.
    /// Returns None if no pending items are available.
    pub async fn claim_next_pending(&self) -> DbResult<Option<QueueItem>> {
        // A single write statement takes the write lock up front. A deferred transaction
        // (SELECT then UPDATE) fails with SQLITE_BUSY when two workers upgrade at once,
        // because SQLite skips the busy handler for read-to-write upgrades.
        sqlx::query_as::<_, QueueItem>(
            r#"
            UPDATE download_queue
            SET status = 'downloading', updated_at = CURRENT_TIMESTAMP
            WHERE file_id = (
                SELECT file_id FROM download_queue
                WHERE status = 'pending'
                ORDER BY priority DESC, created_at ASC
                LIMIT 1
            )
            AND status = 'pending'
            RETURNING *
            "#,
        )
        .fetch_optional(self.pool)
        .await
        .map_err(tracked(DbOp::Queue, "Failed to claim queue item"))
    }

    /// Put rows left in `downloading` back to `pending`
    ///
    /// Only `pending` rows are claimed, so a row a crash left in `downloading` would
    /// otherwise never run again. Call once at startup, before any worker starts; it
    /// keeps priority and `created_at`, so queue order is unchanged. Returns the count.
    pub async fn requeue_interrupted(&self) -> DbResult<usize> {
        let result = sqlx::query(
            r#"
            UPDATE download_queue
            SET status = 'pending', updated_at = CURRENT_TIMESTAMP
            WHERE status = 'downloading'
            "#,
        )
        .execute(self.pool)
        .await
        .map_err(tracked(
            DbOp::Queue,
            "Failed to requeue interrupted downloads",
        ))?;

        Ok(result.rows_affected() as usize)
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
        .map_err(tracked(DbOp::Queue, "Failed to get queue items"))?;

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
        .map_err(tracked(DbOp::Queue, "Failed to get queue item by file_id"))?;

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
        .map_err(tracked(DbOp::Queue, "Failed to update queue status"))?;

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
        .map_err(tracked(DbOp::Queue, "Failed to update queue priority"))?;

        Ok(())
    }

    /// Give the rows in `file_ids` strictly decreasing priorities, so `get_all` and the
    /// workers list them in that order (first = highest priority). One transaction.
    pub async fn set_order(&self, file_ids: &[i64]) -> DbResult<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(tracked(DbOp::Queue, "Failed to reorder queue"))?;
        let top = file_ids.len() as i64;
        for (index, file_id) in file_ids.iter().enumerate() {
            sqlx::query(
                r#"
                UPDATE download_queue
                SET priority = ?, updated_at = CURRENT_TIMESTAMP
                WHERE file_id = ?
                "#,
            )
            .bind(top - index as i64)
            .bind(file_id)
            .execute(&mut *tx)
            .await
            .map_err(tracked(DbOp::Queue, "Failed to reorder queue"))?;
        }
        tx.commit()
            .await
            .map_err(tracked(DbOp::Queue, "Failed to reorder queue"))
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
        .map_err(tracked(DbOp::Queue, "Failed to pause queue"))?;

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
        .map_err(tracked(DbOp::Queue, "Failed to resume queue"))?;

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
        .map_err(tracked(DbOp::Queue, "Failed to remove from queue"))?;

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
        .map_err(tracked(DbOp::Queue, "Failed to clear completed items"))?;

        Ok(result.rows_affected() as usize)
    }

    /// Row counts for the states that have a queue gauge
    pub async fn counts_by_status(&self) -> DbResult<Vec<(String, i64)>> {
        sqlx::query_as::<_, (String, i64)>(
            r#"
            SELECT status, COUNT(*) FROM download_queue
            WHERE status IN ('pending', 'downloading', 'paused', 'failed')
            GROUP BY status
            "#,
        )
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Queue, "Failed to count queue items"))
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
        .map_err(tracked(DbOp::Queue, "Failed to get queue stats"))?;

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
