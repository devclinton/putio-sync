//! Held magnets (feature 043): magnets accepted but not yet sent to Put.io, because no
//! cache had their file list on the first ask. Times are Unix seconds. The magnet is kept
//! only here, to send to Put.io later, and is never logged.

use super::error::{tracked, DbResult};
use putio_sync_telemetry::DbOp;
use serde::Serialize;
use sqlx::{Row, SqlitePool};

/// `source` of a watch-folder hold, which the watcher re-asks itself
const WATCH: &str = "watch_magnet";

/// A magnet to hold
#[derive(Debug, Clone, Copy)]
pub struct NewHeld<'a> {
    pub hash: &'a str,
    pub magnet: &'a str,
    /// `magnet`, `url` or `watch_magnet`
    pub source: &'a str,
    pub category: Option<&'a str>,
    pub watch_folder: Option<&'a str>,
    /// The pending `release_checks` row
    pub check_id: Option<i64>,
    /// The first ask's result
    pub last_lookup: Option<&'a str>,
    pub held_at: i64,
    pub next_at: i64,
    pub until_at: i64,
}

/// A held magnet
#[derive(Clone, PartialEq, Eq)]
pub struct Held {
    pub hash: String,
    pub magnet: String,
    pub source: String,
    pub category: Option<String>,
    pub watch_folder: Option<String>,
    pub check_id: Option<i64>,
    /// Asks so far, the first included
    pub attempts: i64,
    pub last_lookup: Option<String>,
    pub held_at: i64,
    pub next_at: i64,
    pub until_at: i64,
}

// The magnet stays out of logs
impl std::fmt::Debug for Held {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Held")
            .field("hash", &self.hash)
            .field("magnet", &"<redacted>")
            .field("source", &self.source)
            .field("category", &self.category)
            .field("watch_folder", &self.watch_folder)
            .field("check_id", &self.check_id)
            .field("attempts", &self.attempts)
            .field("last_lookup", &self.last_lookup)
            .field("held_at", &self.held_at)
            .field("next_at", &self.next_at)
            .field("until_at", &self.until_at)
            .finish()
    }
}

const COLUMNS: &str = "hash, magnet, source, category, watch_folder, check_id, attempts, \
                       last_lookup, held_at, next_at, until_at";

fn from_row(row: &sqlx::sqlite::SqliteRow) -> Held {
    Held {
        hash: row.get("hash"),
        magnet: row.get("magnet"),
        source: row.get("source"),
        category: row.get("category"),
        watch_folder: row.get("watch_folder"),
        check_id: row.get("check_id"),
        attempts: row.get("attempts"),
        last_lookup: row.get("last_lookup"),
        held_at: row.get("held_at"),
        next_at: row.get("next_at"),
        until_at: row.get("until_at"),
    }
}

/// Repository for `held_magnets`
pub struct HeldRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> HeldRepository<'a> {
    /// Create a new held magnets repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Hold a magnet. A hash already held is an error.
    pub async fn insert(&self, held: &NewHeld<'_>) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT INTO held_magnets
                (hash, magnet, source, category, watch_folder, check_id, last_lookup,
                 held_at, next_at, until_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(held.hash)
        .bind(held.magnet)
        .bind(held.source)
        .bind(held.category)
        .bind(held.watch_folder)
        .bind(held.check_id)
        .bind(held.last_lookup)
        .bind(held.held_at)
        .bind(held.next_at)
        .bind(held.until_at)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to hold magnet"))?;
        Ok(())
    }

    /// The *arr holds due an ask at `now`, the longest-waiting first. Watch-folder holds
    /// are left to the watcher.
    pub async fn due(&self, now: i64, limit: i64) -> DbResult<Vec<Held>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM held_magnets WHERE next_at <= ? AND source != ? \
             ORDER BY next_at, held_at LIMIT ?"
        );
        let rows = sqlx::query(&sql)
            .bind(now)
            .bind(WATCH)
            .bind(limit)
            .fetch_all(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to list due held magnets"))?;
        Ok(rows.iter().map(from_row).collect())
    }

    /// Count an ask that missed and set the next one
    pub async fn reschedule(&self, hash: &str, next_at: i64, last_lookup: &str) -> DbResult<()> {
        sqlx::query(
            r#"
            UPDATE held_magnets SET attempts = attempts + 1, next_at = ?, last_lookup = ?
            WHERE hash = ?
            "#,
        )
        .bind(next_at)
        .bind(last_lookup)
        .bind(hash)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to reschedule held magnet"))?;
        Ok(())
    }

    /// One held magnet
    pub async fn get(&self, hash: &str) -> DbResult<Option<Held>> {
        let sql = format!("SELECT {COLUMNS} FROM held_magnets WHERE hash = ?");
        let row = sqlx::query(&sql)
            .bind(hash)
            .fetch_optional(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to get held magnet"))?;
        Ok(row.as_ref().map(from_row))
    }

    /// Every held magnet, the oldest first
    pub async fn list(&self) -> DbResult<Vec<Held>> {
        let sql = format!("SELECT {COLUMNS} FROM held_magnets ORDER BY held_at, hash");
        let rows = sqlx::query(&sql)
            .fetch_all(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to list held magnets"))?;
        Ok(rows.iter().map(from_row).collect())
    }

    /// The watch-folder holds of `watch_folder`
    pub async fn in_watch_folder(&self, watch_folder: &str) -> DbResult<Vec<Held>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM held_magnets WHERE source = ? AND watch_folder = ? ORDER BY held_at, hash"
        );
        let rows = sqlx::query(&sql)
            .bind(WATCH)
            .bind(watch_folder)
            .fetch_all(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to list watch folder holds"))?;
        Ok(rows.iter().map(from_row).collect())
    }

    /// End a hold and record how (`result`, a `HoldResult` label) for the Stats page.
    /// Returns the hold, or `None` when the hash wasn't held.
    pub async fn finish(&self, hash: &str, result: &str, now: i64) -> DbResult<Option<Held>> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(tracked(DbOp::Other, "Failed to end hold"))?;
        let sql = format!("DELETE FROM held_magnets WHERE hash = ? RETURNING {COLUMNS}");
        let row = sqlx::query(&sql)
            .bind(hash)
            .fetch_optional(&mut *tx)
            .await
            .map_err(tracked(DbOp::Other, "Failed to end hold"))?;
        let Some(held) = row.as_ref().map(from_row) else {
            return Ok(None);
        };
        sqlx::query(
            "INSERT INTO hold_ends (hash, result, held_secs, ended_at) VALUES (?, ?, ?, ?)",
        )
        .bind(hash)
        .bind(result)
        .bind((now - held.held_at).max(0))
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(tracked(DbOp::Other, "Failed to record hold end"))?;
        tx.commit()
            .await
            .map_err(tracked(DbOp::Other, "Failed to end hold"))?;
        Ok(Some(held))
    }

    /// How holds ended since `from` (Unix seconds), with the number held now
    pub async fn stats(&self, from: i64) -> DbResult<HoldStats> {
        let mut stats = HoldStats {
            now: self.count().await?,
            ..HoldStats::default()
        };
        let rows = sqlx::query(
            "SELECT result, COUNT(*) AS n FROM hold_ends WHERE ended_at >= ? GROUP BY result",
        )
        .bind(from)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to count hold ends"))?;
        for row in &rows {
            let n: i64 = row.get("n");
            match row.get::<String, _>("result").as_str() {
                "found_passed" | "found_rejected" => stats.found += n,
                "ran_out" => stats.ran_out += n,
                "removed" => stats.removed += n,
                _ => {}
            }
        }
        let found: Vec<i64> = sqlx::query_scalar(
            r#"
            SELECT held_secs FROM hold_ends
            WHERE ended_at >= ? AND result IN ('found_passed', 'found_rejected')
            ORDER BY held_secs
            "#,
        )
        .bind(from)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to read hold times"))?;
        stats.median_found_secs = found.get(found.len() / 2).copied();
        Ok(stats)
    }

    /// Delete hold ends older than `days` days
    pub async fn prune(&self, days: i64, now: i64) -> DbResult<u64> {
        let result = sqlx::query("DELETE FROM hold_ends WHERE ended_at < ?")
            .bind(now - days * 86_400)
            .execute(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to prune hold ends"))?;
        Ok(result.rows_affected())
    }

    /// End a hold; `false` when the hash wasn't held
    pub async fn delete(&self, hash: &str) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM held_magnets WHERE hash = ?")
            .bind(hash)
            .execute(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to delete held magnet"))?;
        Ok(result.rows_affected() > 0)
    }

    /// Number of magnets held now
    pub async fn count(&self) -> DbResult<i64> {
        sqlx::query_scalar("SELECT COUNT(*) FROM held_magnets")
            .fetch_one(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to count held magnets"))
    }
}

/// The Stats page's `held` block
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HoldStats {
    /// Held now
    pub now: i64,
    /// Found on a later ask, passed or rejected
    pub found: i64,
    /// Sent to Put.io unchecked when the hold ran out
    pub ran_out: i64,
    /// Removed while held
    pub removed: i64,
    /// Median time from hold to a later ask's find; `None` with none found
    pub median_found_secs: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use tempfile::TempDir;

    async fn db() -> (TempDir, Database) {
        let dir = TempDir::new().unwrap();
        let db = Database::new(dir.path().join("held.db")).await.unwrap();
        (dir, db)
    }

    fn held<'a>(hash: &'a str, next_at: i64) -> NewHeld<'a> {
        NewHeld {
            hash,
            magnet: "magnet:?xt=urn:btih:x",
            source: "magnet",
            category: Some("sonarr"),
            watch_folder: None,
            check_id: Some(7),
            last_lookup: Some("timeout"),
            held_at: 1000,
            next_at,
            until_at: 1300,
        }
    }

    #[tokio::test]
    async fn due_is_oldest_first_and_reschedule_counts() {
        let (_dir, db) = db().await;
        let repo = db.held();
        repo.insert(&held("b", 1060)).await.unwrap();
        repo.insert(&held("a", 1030)).await.unwrap();
        repo.insert(&held("c", 1200)).await.unwrap();
        assert_eq!(repo.count().await.unwrap(), 3);

        let due: Vec<_> = repo.due(1100, 10).await.unwrap();
        assert_eq!(
            due.iter().map(|h| h.hash.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(repo.due(1100, 1).await.unwrap().len(), 1);

        repo.reschedule("a", 1130, "not_found").await.unwrap();
        let a = repo.get("a").await.unwrap().unwrap();
        assert_eq!(a.attempts, 2);
        assert_eq!(a.next_at, 1130);
        assert_eq!(a.last_lookup.as_deref(), Some("not_found"));
        assert_eq!(a.check_id, Some(7));
    }

    #[tokio::test]
    async fn a_hash_is_held_once_and_delete_ends_it() {
        let (_dir, db) = db().await;
        let repo = db.held();
        repo.insert(&held("a", 1030)).await.unwrap();
        assert!(repo.insert(&held("a", 1030)).await.is_err());
        assert!(repo.delete("a").await.unwrap());
        assert!(!repo.delete("a").await.unwrap());
        assert!(repo.get("a").await.unwrap().is_none());
        assert!(repo.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn watch_holds_are_left_out_of_due() {
        let (_dir, db) = db().await;
        let repo = db.held();
        repo.insert(&held("a", 1030)).await.unwrap();
        let watch = NewHeld {
            source: "watch_magnet",
            category: None,
            watch_folder: Some("/watch"),
            ..held("w", 1030)
        };
        repo.insert(&watch).await.unwrap();
        let due = repo.due(1100, 10).await.unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].hash, "a");
        let watched = repo.in_watch_folder("/watch").await.unwrap();
        assert_eq!(watched.len(), 1);
        assert_eq!(watched[0].hash, "w");
        assert!(repo.in_watch_folder("/other").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn finish_records_the_end_for_stats() {
        let (_dir, db) = db().await;
        let repo = db.held();
        for hash in ["a", "b", "c", "d", "e"] {
            repo.insert(&held(hash, 1030)).await.unwrap();
        }
        // held_at is 1000
        repo.finish("a", "found_passed", 1100).await.unwrap();
        repo.finish("b", "found_rejected", 1300).await.unwrap();
        repo.finish("c", "found_passed", 1200).await.unwrap();
        repo.finish("d", "ran_out", 1300).await.unwrap();
        assert!(repo.finish("d", "removed", 1300).await.unwrap().is_none());
        let stats = repo.stats(0).await.unwrap();
        assert_eq!(
            stats,
            HoldStats {
                now: 1,
                found: 3,
                ran_out: 1,
                removed: 0,
                median_found_secs: Some(200),
            }
        );
        assert_eq!(repo.stats(1250).await.unwrap().found, 1);
        assert_eq!(repo.prune(1, 1300 + 86_400).await.unwrap(), 2);
        assert_eq!(repo.stats(0).await.unwrap().ran_out, 1);
    }

    #[tokio::test]
    async fn debug_hides_the_magnet() {
        let (_dir, db) = db().await;
        db.held().insert(&held("a", 1030)).await.unwrap();
        let shown = format!("{:?}", db.held().get("a").await.unwrap().unwrap());
        assert!(!shown.contains("magnet:?"));
        assert!(shown.contains("<redacted>"));
    }
}
