//! Why files were not downloaded (feature 009): the latest folder rule decision per
//! Put.io file

use super::error::{tracked, DbResult};
use putio_sync_telemetry::{DbOp, RuleDecision};
use serde::Serialize;
use sqlx::{Row, SqlitePool};

/// One recorded decision
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuleDecisionRecord {
    /// Put.io file id
    pub file_id: i64,
    /// File name
    pub name: String,
    /// Folder path relative to the Put.io root
    pub path: String,
    /// `blocked`, `skipped` or `deleted`
    pub decision: &'static str,
    /// The reason in words
    pub reason: String,
    /// The rule path that decided, `None` for the global settings
    pub rule: Option<String>,
    /// When, `YYYY-MM-DDTHH:MM:SSZ`
    pub decided_at: String,
}

/// Repository for `rule_decisions`
pub struct RuleDecisionsRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> RuleDecisionsRepository<'a> {
    /// Create a new rule decisions repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Record (or replace) the decision for a file
    pub async fn record(
        &self,
        file_id: i64,
        name: &str,
        folder_path: &str,
        decision: RuleDecision,
        reason: &str,
        rule_path: Option<&str>,
    ) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT INTO rule_decisions (file_id, filename, folder_path, decision, reason, rule_path)
            VALUES (?, ?, ?, ?, ?, ?)
            ON CONFLICT(file_id) DO UPDATE SET
                filename = excluded.filename,
                folder_path = excluded.folder_path,
                decision = excluded.decision,
                reason = excluded.reason,
                rule_path = excluded.rule_path,
                decided_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(file_id)
        .bind(name)
        .bind(folder_path)
        .bind(decision.as_str())
        .bind(reason)
        .bind(rule_path)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to record rule decision"))?;
        Ok(())
    }

    /// Forget a file's decision (it was queued after all)
    pub async fn remove(&self, file_id: i64) -> DbResult<()> {
        sqlx::query("DELETE FROM rule_decisions WHERE file_id = ?")
            .bind(file_id)
            .execute(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to remove rule decision"))?;
        Ok(())
    }

    /// The newest `limit` decisions
    pub async fn recent(&self, limit: i64) -> DbResult<Vec<RuleDecisionRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT file_id, filename, folder_path, decision, reason, rule_path,
                   strftime('%Y-%m-%dT%H:%M:%SZ', decided_at) AS decided_at
            FROM rule_decisions ORDER BY decided_at DESC, file_id DESC LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to list rule decisions"))?;
        Ok(rows
            .iter()
            .map(|row| {
                let decision: String = row.get("decision");
                RuleDecisionRecord {
                    file_id: row.get("file_id"),
                    name: row.get("filename"),
                    path: row.get("folder_path"),
                    decision: RuleDecision::ALL
                        .iter()
                        .find(|d| d.as_str() == decision)
                        .map_or("blocked", RuleDecision::as_str),
                    reason: row.get("reason"),
                    rule: row.get("rule_path"),
                    decided_at: row
                        .get::<Option<String>, _>("decided_at")
                        .unwrap_or_default(),
                }
            })
            .collect())
    }

    /// Remove every decision; returns how many there were
    pub async fn clear(&self) -> DbResult<u64> {
        let result = sqlx::query("DELETE FROM rule_decisions")
            .execute(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to clear rule decisions"))?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    async fn record_replace_remove_and_clear() {
        let (_dir, db) = setup_test_db().await;
        let repo = db.rule_decisions();
        repo.record(
            102,
            "setup.exe",
            "Movies",
            RuleDecision::Blocked,
            "extension exe is blocked",
            None,
        )
        .await
        .unwrap();
        repo.record(
            201,
            "a.nfo",
            "Shows",
            RuleDecision::Skipped,
            "folder is skipped",
            Some("Shows"),
        )
        .await
        .unwrap();
        let all = repo.recent(10).await.unwrap();
        assert_eq!(all.len(), 2);
        let exe = all.iter().find(|r| r.file_id == 102).unwrap();
        assert_eq!(exe.decision, "blocked");
        assert_eq!(exe.rule, None);
        assert_eq!(exe.path, "Movies");
        assert!(exe.decided_at.ends_with('Z'), "{}", exe.decided_at);

        repo.record(
            102,
            "setup.exe",
            "Movies",
            RuleDecision::Deleted,
            "extension exe is blocked",
            Some("Movies"),
        )
        .await
        .unwrap();
        let exe = repo.recent(10).await.unwrap();
        assert_eq!(exe.len(), 2);
        let exe = exe.iter().find(|r| r.file_id == 102).unwrap();
        assert_eq!(exe.decision, "deleted");
        assert_eq!(exe.rule.as_deref(), Some("Movies"));

        assert_eq!(repo.recent(1).await.unwrap().len(), 1);
        repo.remove(201).await.unwrap();
        assert_eq!(repo.recent(10).await.unwrap().len(), 1);
        assert_eq!(repo.clear().await.unwrap(), 1);
        assert!(repo.recent(10).await.unwrap().is_empty());
    }
}
