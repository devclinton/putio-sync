//! Release pre-checks and reports (feature 042): every check, and every rejection and
//! operator flag with the traits that tell fakes apart. Never a magnet link, URL, API
//! key or .torrent bytes.

use super::error::{tracked, DbError, DbResult};
use putio_sync_telemetry::DbOp;
use sqlx::{Row, SqlitePool};

/// The longest operator note kept
pub const MAX_NOTE_CHARS: usize = 500;

/// `release_checks.state`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    /// The file list was read and the rule passed
    Passed,
    /// The file list was read and the rule failed
    Rejected,
    /// Waiting for the check on Put.io (a magnet fallback)
    Pending,
}

impl CheckState {
    /// The stored value
    pub fn as_str(self) -> &'static str {
        match self {
            CheckState::Passed => "passed",
            CheckState::Rejected => "rejected",
            CheckState::Pending => "pending",
        }
    }
}

/// A check to record. `source`, `stage` and `lookup` take the values the table's CHECKs
/// allow (data-model.md).
#[derive(Debug, Clone, Default)]
pub struct NewCheck<'a> {
    /// Info hash, `None` when the .torrent couldn't be read
    pub hash: Option<&'a str>,
    /// Release name
    pub name: &'a str,
    /// `torrent`, `url`, `magnet`, `watch_torrent` or `watch_magnet`
    pub source: &'a str,
    /// The *arr category
    pub category: Option<&'a str>,
    /// The watch path
    pub watch_folder: Option<&'a str>,
    /// `before` or `putio`
    pub stage: &'a str,
    /// Outcome, `None` meaning pending
    pub state: Option<CheckState>,
    /// Magnets only: the best cache result
    pub lookup: Option<&'a str>,
    /// Host of the cache that answered `found`
    pub lookup_cache: Option<&'a str>,
    /// The Put.io transfer, when one was created
    pub transfer_id: Option<i64>,
    /// Bytes in the file list
    pub total_size: i64,
    /// Why it was rejected
    pub reason: Option<&'a str>,
}

/// A check still waiting for Put.io
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCheck {
    /// Row id
    pub id: i64,
    /// Info hash
    pub hash: Option<String>,
    /// Release name
    pub name: String,
    /// As recorded
    pub source: String,
    /// The *arr category
    pub category: Option<String>,
    /// The watch path
    pub watch_folder: Option<String>,
    /// The Put.io transfer
    pub transfer_id: Option<i64>,
}

/// `release_reports.kind`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportKind {
    /// The check rejected it
    Rejected,
    /// The operator flagged it fake
    FlaggedFake,
    /// The operator flagged it not fake
    FlaggedNotFake,
}

impl ReportKind {
    /// The stored value
    pub fn as_str(self) -> &'static str {
        match self {
            ReportKind::Rejected => "rejected",
            ReportKind::FlaggedFake => "flagged_fake",
            ReportKind::FlaggedNotFake => "flagged_not_fake",
        }
    }
}

/// A report to record (data-model.md `release_reports`)
#[derive(Debug, Clone)]
pub struct NewReport<'a> {
    /// What happened
    pub kind: ReportKind,
    /// The check it came from
    pub check_id: Option<i64>,
    /// The *arr item
    pub arr_hash: Option<&'a str>,
    /// The Put.io file or folder flagged
    pub file_id: Option<i64>,
    /// The Put.io transfer
    pub transfer_id: Option<i64>,
    /// As `release_checks.source`, plus `putio` and `manual`
    pub source: Option<&'a str>,
    /// The *arr category
    pub category: Option<&'a str>,
    /// The watch path
    pub watch_folder: Option<&'a str>,
    /// `before`, `putio`, or `None` for a flag
    pub stage: Option<&'a str>,
    /// Release name
    pub name: &'a str,
    /// Info hash
    pub hash: Option<&'a str>,
    /// Bytes in the file list
    pub total_size: Option<i64>,
    /// Files in the list, padding left out
    pub file_count: Option<i64>,
    /// JSON `[{"ext","count","largest"}]`, sorted by largest
    pub types: &'a str,
    /// The largest wanted file, 0 when none
    pub largest_wanted: Option<i64>,
    /// The minimum in force
    pub min_size: Option<i64>,
    /// JSON list of host names
    pub tracker_hosts: &'a str,
    /// The torrent is private
    pub is_private: bool,
    /// Why it was rejected
    pub reason: Option<&'a str>,
    /// The operator's note
    pub note: Option<&'a str>,
}

impl Default for NewReport<'_> {
    fn default() -> Self {
        Self {
            kind: ReportKind::Rejected,
            check_id: None,
            arr_hash: None,
            file_id: None,
            transfer_id: None,
            source: None,
            category: None,
            watch_folder: None,
            stage: None,
            name: "",
            hash: None,
            total_size: None,
            file_count: None,
            types: "[]",
            largest_wanted: None,
            min_size: None,
            tracker_hosts: "[]",
            is_private: false,
            reason: None,
            note: None,
        }
    }
}

/// Repository for `release_checks` and `release_reports`
pub struct ReleasesRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> ReleasesRepository<'a> {
    /// Create a new releases repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Record a check; returns its id
    pub async fn insert_check(&self, check: &NewCheck<'_>) -> DbResult<i64> {
        let state = check.state.unwrap_or(CheckState::Pending);
        let result = sqlx::query(
            r#"
            INSERT INTO release_checks
                (hash, name, source, category, watch_folder, stage, state, lookup, lookup_cache,
                 transfer_id, total_size, reason, resolved_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                    CASE WHEN ? = 'pending' THEN NULL ELSE CURRENT_TIMESTAMP END)
            "#,
        )
        .bind(check.hash)
        .bind(check.name)
        .bind(check.source)
        .bind(check.category)
        .bind(check.watch_folder)
        .bind(check.stage)
        .bind(state.as_str())
        .bind(check.lookup)
        .bind(check.lookup_cache)
        .bind(check.transfer_id)
        .bind(check.total_size)
        .bind(check.reason)
        .bind(state.as_str())
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to insert release check"))?;
        Ok(result.last_insert_rowid())
    }

    /// Settle a pending check, checked on Put.io. A row already settled is left alone.
    pub async fn resolve_check(
        &self,
        id: i64,
        state: CheckState,
        reason: Option<&str>,
        putio_bytes: i64,
    ) -> DbResult<bool> {
        let result = sqlx::query(
            r#"
            UPDATE release_checks SET
                state = ?, reason = ?, putio_bytes = ?, stage = 'putio',
                resolved_at = CURRENT_TIMESTAMP
            WHERE id = ? AND state = 'pending'
            "#,
        )
        .bind(state.as_str())
        .bind(reason)
        .bind(putio_bytes)
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to resolve release check"))?;
        Ok(result.rows_affected() > 0)
    }

    /// Settle the pending checks of a release as rejected for `reason` (its *arr item
    /// was removed, say). Returns how many rows changed.
    pub async fn resolve_pending_by_hash(&self, hash: &str, reason: &str) -> DbResult<u64> {
        let result = sqlx::query(
            r#"
            UPDATE release_checks SET
                state = 'rejected', reason = ?, resolved_at = CURRENT_TIMESTAMP
            WHERE hash = ? AND state = 'pending'
            "#,
        )
        .bind(reason)
        .bind(hash)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to resolve release checks"))?;
        Ok(result.rows_affected())
    }

    /// Checks waiting for Put.io, oldest first
    pub async fn pending_checks(&self) -> DbResult<Vec<PendingCheck>> {
        let rows = sqlx::query(
            r#"
            SELECT id, hash, name, source, category, watch_folder, transfer_id
            FROM release_checks WHERE state = 'pending' ORDER BY id
            "#,
        )
        .fetch_all(self.pool)
        .await
        .map_err(tracked(
            DbOp::Other,
            "Failed to list pending release checks",
        ))?;
        Ok(rows
            .iter()
            .map(|row| PendingCheck {
                id: row.get("id"),
                hash: row.get("hash"),
                name: row.get("name"),
                source: row.get("source"),
                category: row.get("category"),
                watch_folder: row.get("watch_folder"),
                transfer_id: row.get("transfer_id"),
            })
            .collect())
    }

    /// Whether any check is waiting: the sync gate's one query when nothing is
    pub async fn has_pending(&self) -> DbResult<bool> {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM release_checks WHERE state = 'pending')")
            .fetch_one(self.pool)
            .await
            .map_err(tracked(
                DbOp::Other,
                "Failed to check pending release checks",
            ))
    }

    /// The transfers whose files the sync must not queue yet
    pub async fn pending_transfer_ids(&self) -> DbResult<Vec<i64>> {
        sqlx::query_scalar(
            r#"
            SELECT DISTINCT transfer_id FROM release_checks
            WHERE state = 'pending' AND transfer_id IS NOT NULL
            "#,
        )
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to list pending transfers"))
    }

    /// Record a rejection or flag; returns its id. A note over [`MAX_NOTE_CHARS`] is
    /// refused.
    pub async fn insert_report(&self, report: &NewReport<'_>) -> DbResult<i64> {
        if report
            .note
            .is_some_and(|n| n.chars().count() > MAX_NOTE_CHARS)
        {
            return Err(DbError::DatabaseError(format!(
                "note is longer than {MAX_NOTE_CHARS} characters"
            )));
        }
        let result = sqlx::query(
            r#"
            INSERT INTO release_reports
                (kind, check_id, arr_hash, file_id, transfer_id, source, category, watch_folder,
                 stage, name, hash, total_size, file_count, types, largest_wanted, min_size,
                 tracker_hosts, is_private, reason, note)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(report.kind.as_str())
        .bind(report.check_id)
        .bind(report.arr_hash)
        .bind(report.file_id)
        .bind(report.transfer_id)
        .bind(report.source)
        .bind(report.category)
        .bind(report.watch_folder)
        .bind(report.stage)
        .bind(report.name)
        .bind(report.hash)
        .bind(report.total_size)
        .bind(report.file_count)
        .bind(report.types)
        .bind(report.largest_wanted)
        .bind(report.min_size)
        .bind(report.tracker_hosts)
        .bind(report.is_private)
        .bind(report.reason)
        .bind(report.note)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to insert release report"))?;
        Ok(result.last_insert_rowid())
    }

    /// Delete checks and reports older than `days` days (research R10: 400). Pending
    /// checks are kept whatever their age, so the sync gate never loses one.
    pub async fn prune(&self, days: i64) -> DbResult<u64> {
        let cutoff = format!("-{days} days");
        let checks = sqlx::query(
            "DELETE FROM release_checks WHERE checked_at < datetime('now', ?) AND state != 'pending'",
        )
        .bind(&cutoff)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to prune release checks"))?;
        let reports =
            sqlx::query("DELETE FROM release_reports WHERE created_at < datetime('now', ?)")
                .bind(&cutoff)
                .execute(self.pool)
                .await
                .map_err(tracked(DbOp::Other, "Failed to prune release reports"))?;
        Ok(checks.rows_affected() + reports.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use tempfile::TempDir;

    async fn db() -> (TempDir, Database) {
        let dir = TempDir::new().unwrap();
        let db = Database::new(dir.path().join("releases.db")).await.unwrap();
        (dir, db)
    }

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    fn check(state: Option<CheckState>) -> NewCheck<'static> {
        NewCheck {
            hash: Some(HASH),
            name: "Show.S01E01",
            source: "magnet",
            category: Some("sonarr"),
            stage: "before",
            state,
            lookup: Some("not_found"),
            transfer_id: Some(9),
            total_size: 1024,
            ..NewCheck::default()
        }
    }

    #[tokio::test]
    async fn pending_checks_resolve_once() {
        let (_dir, db) = db().await;
        let repo = db.releases();
        assert!(!repo.has_pending().await.unwrap());

        repo.insert_check(&check(Some(CheckState::Passed)))
            .await
            .unwrap();
        let id = repo.insert_check(&check(None)).await.unwrap();
        assert!(repo.has_pending().await.unwrap());
        assert_eq!(repo.pending_transfer_ids().await.unwrap(), vec![9]);
        let pending = repo.pending_checks().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, id);
        assert_eq!(pending[0].category.as_deref(), Some("sonarr"));

        assert!(repo
            .resolve_check(id, CheckState::Rejected, Some("no video"), 500)
            .await
            .unwrap());
        assert!(!repo
            .resolve_check(id, CheckState::Passed, None, 0)
            .await
            .unwrap());
        assert!(!repo.has_pending().await.unwrap());
        let (stage, state, bytes): (String, String, i64) =
            sqlx::query_as("SELECT stage, state, putio_bytes FROM release_checks WHERE id = ?")
                .bind(id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(
            (stage.as_str(), state.as_str(), bytes),
            ("putio", "rejected", 500)
        );
    }

    #[tokio::test]
    async fn removed_item_resolves_its_pending_check() {
        let (_dir, db) = db().await;
        let repo = db.releases();
        repo.insert_check(&check(None)).await.unwrap();
        assert_eq!(
            repo.resolve_pending_by_hash(HASH, "transfer no longer on Put.io")
                .await
                .unwrap(),
            1
        );
        assert!(!repo.has_pending().await.unwrap());
    }

    #[tokio::test]
    async fn checks_enforce_the_allowed_values() {
        let (_dir, db) = db().await;
        let bad = NewCheck {
            source: "ftp",
            ..check(Some(CheckState::Passed))
        };
        assert!(db.releases().insert_check(&bad).await.is_err());
    }

    #[tokio::test]
    async fn reports_default_to_empty_lists_and_cap_notes() {
        let (_dir, db) = db().await;
        let repo = db.releases();
        let id = repo
            .insert_report(&NewReport {
                name: "Show.S01E01",
                hash: Some(HASH),
                reason: Some("no_wanted"),
                ..NewReport::default()
            })
            .await
            .unwrap();
        let (kind, types, hosts): (String, String, String) =
            sqlx::query_as("SELECT kind, types, tracker_hosts FROM release_reports WHERE id = ?")
                .bind(id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(
            (kind.as_str(), types.as_str(), hosts.as_str()),
            ("rejected", "[]", "[]")
        );

        let long = "x".repeat(MAX_NOTE_CHARS + 1);
        let flag = NewReport {
            kind: ReportKind::FlaggedFake,
            name: "x",
            note: Some(&long),
            ..NewReport::default()
        };
        assert!(repo.insert_report(&flag).await.is_err());
    }

    #[tokio::test]
    async fn prune_keeps_recent_and_pending_rows() {
        let (_dir, db) = db().await;
        let repo = db.releases();
        let old = repo
            .insert_check(&check(Some(CheckState::Passed)))
            .await
            .unwrap();
        let pending = repo.insert_check(&check(None)).await.unwrap();
        repo.insert_check(&check(Some(CheckState::Passed)))
            .await
            .unwrap();
        repo.insert_report(&NewReport {
            name: "old",
            ..NewReport::default()
        })
        .await
        .unwrap();
        for table in ["release_checks", "release_reports"] {
            let column = if table == "release_checks" {
                "checked_at"
            } else {
                "created_at"
            };
            sqlx::query(&format!(
                "UPDATE {table} SET {column} = datetime('now', '-401 days') WHERE id IN (?, ?)"
            ))
            .bind(old)
            .bind(pending)
            .execute(&db.pool)
            .await
            .unwrap();
        }
        // The old passed check and the old report go; the old pending check stays
        assert_eq!(repo.prune(400).await.unwrap(), 2);
        assert!(repo.has_pending().await.unwrap());
    }
}
