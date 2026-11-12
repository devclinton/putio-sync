//! Update checker state (feature 055): one row with the last releases check, so a restart
//! shows the known update without asking GitHub again.

use super::error::{tracked, DbResult};
use chrono::{DateTime, Utc};
use putio_sync_telemetry::DbOp;
use sqlx::SqlitePool;

/// The `update_state` row; every field is empty before the first check
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateState {
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    /// Why the last attempt failed; cleared by a successful one
    pub last_error: Option<String>,
    /// ETag of the last releases answer, sent as `If-None-Match`
    pub etag: Option<String>,
    /// The newer releases from the last answer, as the update crate stores them
    pub releases_json: Option<String>,
    /// The version whose notice the user hid
    pub dismissed_version: Option<String>,
    /// The version the last rollback left
    pub last_rolled_back: Option<String>,
    /// Why it was rolled back; cleared when the user dismisses the message
    pub rollback_reason: Option<String>,
}

type Row = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn parse_time(text: Option<String>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&text?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

/// Repository for `update_state`
pub struct UpdatesRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> UpdatesRepository<'a> {
    /// Create a new update state repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// The stored state, or the empty state before the first check
    pub async fn get(&self) -> DbResult<UpdateState> {
        let row: Option<Row> = sqlx::query_as(
            "SELECT last_attempt_at, last_success_at, last_error, etag, releases_json, \
             dismissed_version, last_rolled_back, rollback_reason FROM update_state WHERE id = 1",
        )
        .fetch_optional(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to read update state"))?;
        let Some((attempt, success, error, etag, releases, dismissed, rolled_back, reason)) = row
        else {
            return Ok(UpdateState::default());
        };
        Ok(UpdateState {
            last_attempt_at: parse_time(attempt),
            last_success_at: parse_time(success),
            last_error: error,
            etag,
            releases_json: releases,
            dismissed_version: dismissed,
            last_rolled_back: rolled_back,
            rollback_reason: reason,
        })
    }

    /// Record a check made at `attempt`. On success (`error` is `None`) the success time
    /// is set and the error cleared; `etag` and `releases_json` replace the stored ones
    /// when given, so a "not modified" answer keeps them. On failure only the attempt
    /// time and the error change.
    pub async fn record_check(
        &self,
        attempt: DateTime<Utc>,
        error: Option<&str>,
        etag: Option<&str>,
        releases_json: Option<&str>,
    ) -> DbResult<()> {
        let attempt = attempt.to_rfc3339();
        let success = error.is_none().then(|| attempt.clone());
        sqlx::query(
            "INSERT INTO update_state (id, last_attempt_at, last_success_at, last_error, etag, \
             releases_json) VALUES (1, ?1, ?2, ?3, ?4, ?5) \
             ON CONFLICT(id) DO UPDATE SET \
             last_attempt_at = ?1, \
             last_success_at = COALESCE(?2, last_success_at), \
             last_error = ?3, \
             etag = COALESCE(?4, etag), \
             releases_json = COALESCE(?5, releases_json)",
        )
        .bind(&attempt)
        .bind(success)
        .bind(error)
        .bind(etag)
        .bind(releases_json)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to record update check"))?;
        Ok(())
    }

    /// Hide the notice for `version`
    pub async fn dismiss(&self, version: &str) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO update_state (id, dismissed_version) VALUES (1, ?1) \
             ON CONFLICT(id) DO UPDATE SET dismissed_version = ?1",
        )
        .bind(version)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to dismiss update"))?;
        Ok(())
    }

    /// Record that `version` was rolled back, and why
    pub async fn record_rollback(&self, version: &str, reason: &str) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO update_state (id, last_rolled_back, rollback_reason) VALUES (1, ?1, ?2) \
             ON CONFLICT(id) DO UPDATE SET last_rolled_back = ?1, rollback_reason = ?2",
        )
        .bind(version)
        .bind(reason)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to record update rollback"))?;
        Ok(())
    }

    /// Hide the rollback message. The version is kept, so it isn't installed again
    /// automatically.
    pub async fn clear_rollback_message(&self) -> DbResult<()> {
        sqlx::query("UPDATE update_state SET rollback_reason = NULL WHERE id = 1")
            .execute(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to clear update rollback"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::Database;
    use chrono::{TimeZone, Utc};
    use tempfile::TempDir;

    #[tokio::test]
    async fn empty_before_the_first_check() {
        let dir = TempDir::new().unwrap();
        let db = Database::new(dir.path().join("test.db")).await.unwrap();
        assert_eq!(db.updates().get().await.unwrap(), Default::default());
    }

    #[tokio::test]
    async fn records_checks_and_keeps_the_etag_on_not_modified() {
        let dir = TempDir::new().unwrap();
        let db = Database::new(dir.path().join("test.db")).await.unwrap();
        let repo = db.updates();
        let t1 = Utc.with_ymd_and_hms(2026, 11, 10, 9, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 11, 11, 9, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2026, 11, 12, 9, 0, 0).unwrap();

        repo.record_check(t1, None, Some("\"e1\""), Some("[1]"))
            .await
            .unwrap();
        let s = repo.get().await.unwrap();
        assert_eq!(s.last_attempt_at, Some(t1));
        assert_eq!(s.last_success_at, Some(t1));
        assert_eq!(s.etag.as_deref(), Some("\"e1\""));
        assert_eq!(s.releases_json.as_deref(), Some("[1]"));

        // a failure keeps what the last success stored
        repo.record_check(t2, Some("timed out"), None, None)
            .await
            .unwrap();
        let s = repo.get().await.unwrap();
        assert_eq!(s.last_attempt_at, Some(t2));
        assert_eq!(s.last_success_at, Some(t1));
        assert_eq!(s.last_error.as_deref(), Some("timed out"));
        assert_eq!(s.releases_json.as_deref(), Some("[1]"));

        // not modified: success, error cleared, etag and releases kept
        repo.record_check(t3, None, None, None).await.unwrap();
        let s = repo.get().await.unwrap();
        assert_eq!(s.last_success_at, Some(t3));
        assert_eq!(s.last_error, None);
        assert_eq!(s.etag.as_deref(), Some("\"e1\""));
        assert_eq!(s.releases_json.as_deref(), Some("[1]"));
    }

    #[tokio::test]
    async fn dismiss_and_rollback() {
        let dir = TempDir::new().unwrap();
        let db = Database::new(dir.path().join("test.db")).await.unwrap();
        let repo = db.updates();
        repo.dismiss("0.4.0").await.unwrap();
        repo.record_rollback("0.4.1", "exited with code 1")
            .await
            .unwrap();
        let s = repo.get().await.unwrap();
        assert_eq!(s.dismissed_version.as_deref(), Some("0.4.0"));
        assert_eq!(s.last_rolled_back.as_deref(), Some("0.4.1"));
        assert_eq!(s.rollback_reason.as_deref(), Some("exited with code 1"));

        repo.clear_rollback_message().await.unwrap();
        let s = repo.get().await.unwrap();
        assert_eq!(s.last_rolled_back.as_deref(), Some("0.4.1"));
        assert_eq!(s.rollback_reason, None);
        assert_eq!(s.dismissed_version.as_deref(), Some("0.4.0"));
    }

    #[tokio::test]
    async fn survives_reopening() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let t = Utc.with_ymd_and_hms(2026, 11, 10, 9, 0, 0).unwrap();
        let db = Database::new(&path).await.unwrap();
        db.updates()
            .record_check(t, None, Some("e"), Some("[]"))
            .await
            .unwrap();
        db.updates().dismiss("0.4.0").await.unwrap();
        db.close().await.unwrap();

        let db = Database::new(&path).await.unwrap();
        let s = db.updates().get().await.unwrap();
        assert_eq!(s.last_success_at, Some(t));
        assert_eq!(s.etag.as_deref(), Some("e"));
        assert_eq!(s.dismissed_version.as_deref(), Some("0.4.0"));
    }
}
