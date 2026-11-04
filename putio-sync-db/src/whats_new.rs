//! What's new (feature 017): the ids of the changelog entries this install has shown, so
//! the web UI lists only the entries an upgrade added. An empty table means a first run.

use super::error::{tracked, DbResult};
use putio_sync_telemetry::DbOp;
use sqlx::SqlitePool;
use std::collections::HashSet;

/// Repository for `whats_new_seen`
pub struct WhatsNewRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> WhatsNewRepository<'a> {
    /// Create a new What's new repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Every id recorded as seen
    pub async fn seen_ids(&self) -> DbResult<HashSet<String>> {
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM whats_new_seen")
            .fetch_all(self.pool)
            .await
            .map_err(tracked(
                DbOp::Other,
                "Failed to read seen changelog entries",
            ))?;
        Ok(ids.into_iter().collect())
    }

    /// Record `ids` as seen; ids already recorded keep their first time
    pub async fn mark_seen(&self, ids: &[String]) -> DbResult<()> {
        let mut tx = self.pool.begin().await.map_err(tracked(
            DbOp::Other,
            "Failed to record seen changelog entries",
        ))?;
        for id in ids {
            sqlx::query("INSERT OR IGNORE INTO whats_new_seen (id) VALUES (?)")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(tracked(
                    DbOp::Other,
                    "Failed to record seen changelog entries",
                ))?;
        }
        tx.commit().await.map_err(tracked(
            DbOp::Other,
            "Failed to record seen changelog entries",
        ))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::Database;
    use tempfile::TempDir;

    #[tokio::test]
    async fn records_ids_once() {
        let dir = TempDir::new().unwrap();
        let db = Database::new(dir.path().join("test.db")).await.unwrap();
        let repo = db.whats_new();
        assert!(repo.seen_ids().await.unwrap().is_empty());

        let ids = vec!["a".to_string(), "b".to_string()];
        repo.mark_seen(&ids).await.unwrap();
        repo.mark_seen(&["b".to_string(), "c".to_string()])
            .await
            .unwrap();
        let seen = repo.seen_ids().await.unwrap();
        assert_eq!(seen.len(), 3);
        assert!(seen.contains("a") && seen.contains("c"));

        repo.mark_seen(&[]).await.unwrap();
        assert_eq!(repo.seen_ids().await.unwrap().len(), 3);
    }
}
