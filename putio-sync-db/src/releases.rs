//! Release pre-checks and reports (feature 042): every check, and every rejection and
//! operator flag with the traits that tell fakes apart. Never a magnet link, URL, API
//! key or .torrent bytes.

use super::analytics::RangePreset;
use super::error::{tracked, DbError, DbResult};
use putio_sync_telemetry::DbOp;
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;

/// The longest operator note kept
pub const MAX_NOTE_CHARS: usize = 500;

/// File types and tracker hosts listed in `/stats/releases`
const TOP_TYPES: i64 = 8;
const TOP_TRACKERS: i64 = 10;

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

/// How a held magnet's check settled, for [`ReleasesRepository::settle_held`]
#[derive(Debug, Clone, Copy)]
pub struct HeldSettle<'a> {
    pub state: CheckState,
    /// `found` when a later ask had it; `None` keeps the first ask's result
    pub lookup: Option<&'a str>,
    pub lookup_cache: Option<&'a str>,
    pub total_size: i64,
    pub reason: Option<&'a str>,
    pub transfer_id: Option<i64>,
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

    /// Settle a held magnet's pending check before Put.io (feature 043): found on a later
    /// ask, or sent on unchecked. A row already settled is left alone.
    pub async fn settle_held(&self, id: i64, held: &HeldSettle<'_>) -> DbResult<bool> {
        let result = sqlx::query(
            r#"
            UPDATE release_checks SET
                state = ?, lookup = COALESCE(?, lookup), lookup_cache = ?, total_size = ?,
                reason = ?, transfer_id = ?, resolved_at = CURRENT_TIMESTAMP
            WHERE id = ? AND state = 'pending'
            "#,
        )
        .bind(held.state.as_str())
        .bind(held.lookup)
        .bind(held.lookup_cache)
        .bind(held.total_size)
        .bind(held.reason)
        .bind(held.transfer_id)
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to settle held release check"))?;
        Ok(result.rows_affected() > 0)
    }

    /// Give a pending check its Put.io transfer, so the check on Put.io (042 US3) picks it
    /// up: a held magnet whose hold ran out (feature 043)
    pub async fn set_check_transfer(&self, id: i64, transfer_id: i64) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE release_checks SET transfer_id = ? WHERE id = ? AND state = 'pending'",
        )
        .bind(transfer_id)
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to set release check transfer"))?;
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

    /// Every report made since the start of `range`, newest first (contracts/api.md
    /// `/releases/reports`)
    pub async fn reports(&self, range: RangePreset, now: i64) -> DbResult<Vec<ReportRow>> {
        let from = range.length().map_or(0, |l| now - l.num_seconds());
        let rows = sqlx::query(&format!(
            "SELECT {REPORT_COLUMNS} FROM release_reports \
             WHERE created_at >= datetime(?, 'unixepoch') ORDER BY created_at DESC, id DESC"
        ))
        .bind(from)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to list release reports"))?;
        Ok(rows.iter().map(report_from_row).collect())
    }

    /// One report
    pub async fn report(&self, id: i64) -> DbResult<Option<ReportRow>> {
        let row = sqlx::query(&format!(
            "SELECT {REPORT_COLUMNS} FROM release_reports WHERE id = ?"
        ))
        .bind(id)
        .fetch_optional(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to get release report"))?;
        Ok(row.as_ref().map(report_from_row))
    }

    /// Record a flag, or replace the note of the same kind of flag on the same release;
    /// returns its id. The release is the `arr_hash`, else the `file_id`, else the `hash`,
    /// else the `check_id` of a rejection with none of those (data-model.md: one active flag
    /// per release and kind).
    pub async fn upsert_flag(&self, flag: &NewReport<'_>) -> DbResult<i64> {
        let key = match (flag.arr_hash, flag.file_id, flag.hash) {
            (Some(h), _, _) => Some(("arr_hash", Key::Text(h))),
            (None, Some(id), _) => Some(("file_id", Key::Int(id))),
            (None, None, Some(h)) => Some(("hash", Key::Text(h))),
            _ => flag.check_id.map(|id| ("check_id", Key::Int(id))),
        };
        if let Some((column, value)) = key {
            let sql = format!("SELECT id FROM release_reports WHERE kind = ? AND {column} = ? ORDER BY id DESC LIMIT 1");
            let q = sqlx::query_scalar::<_, i64>(&sql).bind(flag.kind.as_str());
            let q = match value {
                Key::Text(t) => q.bind(t),
                Key::Int(i) => q.bind(i),
            };
            let existing = q
                .fetch_optional(self.pool)
                .await
                .map_err(tracked(DbOp::Other, "Failed to find release flag"))?;
            if let Some(id) = existing {
                if flag
                    .note
                    .is_some_and(|n| n.chars().count() > MAX_NOTE_CHARS)
                {
                    return Err(DbError::DatabaseError(format!(
                        "note is longer than {MAX_NOTE_CHARS} characters"
                    )));
                }
                sqlx::query("UPDATE release_reports SET note = ? WHERE id = ?")
                    .bind(flag.note)
                    .bind(id)
                    .execute(self.pool)
                    .await
                    .map_err(tracked(DbOp::Other, "Failed to update release flag"))?;
                return Ok(id);
            }
        }
        self.insert_report(flag).await
    }

    /// Undo a flag: delete its row. A `rejected` row is not a flag and stays.
    pub async fn delete_flag(&self, id: i64) -> DbResult<FlagUndo> {
        let kind: Option<String> =
            sqlx::query_scalar("SELECT kind FROM release_reports WHERE id = ?")
                .bind(id)
                .fetch_optional(self.pool)
                .await
                .map_err(tracked(DbOp::Other, "Failed to find release flag"))?;
        match kind.as_deref() {
            None => Ok(FlagUndo::NotFound),
            Some("rejected") => Ok(FlagUndo::NotAFlag),
            Some(_) => {
                sqlx::query("DELETE FROM release_reports WHERE id = ?")
                    .bind(id)
                    .execute(self.pool)
                    .await
                    .map_err(tracked(DbOp::Other, "Failed to delete release flag"))?;
                Ok(FlagUndo::Deleted)
            }
        }
    }

    /// The flags on any of these *arr hashes, Put.io files, info hashes or checks
    pub async fn flags_for(
        &self,
        arr_hashes: &[String],
        file_ids: &[i64],
        hashes: &[String],
        check_ids: &[i64],
    ) -> DbResult<Vec<ReportRow>> {
        let list = |n: usize| vec!["?"; n].join(",");
        let mut wheres = Vec::new();
        if !arr_hashes.is_empty() {
            wheres.push(format!("arr_hash IN ({})", list(arr_hashes.len())));
        }
        if !file_ids.is_empty() {
            wheres.push(format!("file_id IN ({})", list(file_ids.len())));
        }
        if !hashes.is_empty() {
            wheres.push(format!("hash IN ({})", list(hashes.len())));
        }
        if !check_ids.is_empty() {
            wheres.push(format!("check_id IN ({})", list(check_ids.len())));
        }
        if wheres.is_empty() {
            return Ok(Vec::new());
        }
        let sql = format!(
            "SELECT {REPORT_COLUMNS} FROM release_reports \
             WHERE kind != 'rejected' AND ({}) ORDER BY id",
            wheres.join(" OR ")
        );
        let mut q = sqlx::query(&sql);
        for h in arr_hashes {
            q = q.bind(h);
        }
        for id in file_ids {
            q = q.bind(id);
        }
        for h in hashes {
            q = q.bind(h);
        }
        for id in check_ids {
            q = q.bind(id);
        }
        let rows = q
            .fetch_all(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to list release flags"))?;
        Ok(rows.iter().map(report_from_row).collect())
    }

    /// Whether the check rejected this release: a `rejected` report on its *arr hash or
    /// info hash
    pub async fn was_rejected(&self, arr_hash: Option<&str>, hash: Option<&str>) -> DbResult<bool> {
        if arr_hash.is_none() && hash.is_none() {
            return Ok(false);
        }
        let found: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM release_reports WHERE kind = 'rejected' \
             AND ((? IS NOT NULL AND arr_hash = ?) OR (? IS NOT NULL AND hash = ?)) LIMIT 1",
        )
        .bind(arr_hash)
        .bind(arr_hash)
        .bind(hash)
        .bind(hash)
        .fetch_optional(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to look up release rejection"))?;
        Ok(found.is_some())
    }

    /// The latest check of a release, by info hash or Put.io transfer
    pub async fn latest_check(
        &self,
        hash: Option<&str>,
        transfer_id: Option<i64>,
    ) -> DbResult<Option<i64>> {
        sqlx::query_scalar(
            "SELECT id FROM release_checks WHERE (? IS NOT NULL AND hash = ?) \
             OR (? IS NOT NULL AND transfer_id = ?) ORDER BY id DESC LIMIT 1",
        )
        .bind(hash)
        .bind(hash)
        .bind(transfer_id)
        .bind(transfer_id)
        .fetch_optional(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to look up release check"))
    }

    /// Everything the Stats page's release tiles draw, for checks made since the start of
    /// `range` (contracts/api.md `/stats/releases`). A rejection on Put.io counts its
    /// bytes as avoided on the NAS; what Put.io downloaded is `fallback_putio_bytes`.
    pub async fn release_stats(&self, range: RangePreset, now: i64) -> DbResult<ReleaseStats> {
        let since = range.length().map(|l| now - l.num_seconds());
        let from = since.unwrap_or(0);
        let mut stats = ReleaseStats {
            range: StatsRange {
                preset: range.as_str(),
                from: since,
                to: now,
            },
            ..ReleaseStats::default()
        };

        let rows = sqlx::query(
            r#"
            SELECT stage, reason, COUNT(*) AS n, COALESCE(SUM(total_size), 0) AS bytes
            FROM release_checks
            WHERE state = 'rejected'
              AND COALESCE(resolved_at, checked_at) >= datetime(?, 'unixepoch')
            GROUP BY stage, reason
            "#,
        )
        .bind(from)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to count rejected releases"))?;
        let mut reasons: BTreeMap<String, i64> = BTreeMap::new();
        for row in rows {
            let (count, bytes): (i64, i64) = (row.get("n"), row.get("bytes"));
            let rejected = &mut stats.rejected;
            rejected.count += count;
            rejected.bytes += bytes;
            let stage = match row.get::<String, _>("stage").as_str() {
                "putio" => &mut rejected.by_stage.putio,
                _ => &mut rejected.by_stage.before,
            };
            stage.count += count;
            stage.bytes += bytes;
            let reason: Option<String> = row.get("reason");
            *reasons.entry(reason.unwrap_or_default()).or_default() += count;
        }
        stats.rejected.by_reason = reasons
            .into_iter()
            .map(|(reason, count)| ReasonCount { reason, count })
            .collect();
        stats
            .rejected
            .by_reason
            .sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.reason.cmp(&b.reason)));

        let rows = sqlx::query(
            r#"
            SELECT lookup, lookup_cache, stage, state, reason = 'gone' AS gone,
                   COUNT(*) AS n, COALESCE(SUM(putio_bytes), 0) AS putio_bytes
            FROM release_checks
            WHERE source IN ('magnet', 'watch_magnet')
              AND checked_at >= datetime(?, 'unixepoch')
            GROUP BY lookup, lookup_cache, stage, state, gone
            "#,
        )
        .bind(from)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to count magnet lookups"))?;
        let mut caches: BTreeMap<Option<String>, CacheCount> = BTreeMap::new();
        for row in rows {
            let count: i64 = row.get("n");
            let lookup: Option<String> = row.get("lookup");
            let magnets = &mut stats.magnets;
            magnets.checked += count;
            if lookup.as_deref() == Some("found") {
                magnets.found += count;
            } else {
                magnets.fell_back += count;
                let fake = row.get::<String, _>("stage") == "putio"
                    && row.get::<String, _>("state") == "rejected"
                    && !row.get::<Option<bool>, _>("gone").unwrap_or(false);
                if fake {
                    magnets.fallback_fake += count;
                    magnets.fallback_putio_bytes += row.get::<i64, _>("putio_bytes");
                }
            }
            // Only the cache that found a .torrent is kept, so misses share one row
            let Some(lookup) = lookup.filter(|l| l != "off") else {
                continue;
            };
            let cache: Option<String> = row.get("lookup_cache");
            let entry = caches.entry(cache.clone()).or_insert_with(|| CacheCount {
                cache,
                ..CacheCount::default()
            });
            match lookup.as_str() {
                "found" => entry.found += count,
                "not_found" => entry.not_found += count,
                "timeout" => entry.timeout += count,
                "wrong_hash" => entry.wrong_hash += count,
                _ => entry.error += count,
            }
        }
        // Named caches first, then the misses
        let (mut named, missed): (Vec<_>, Vec<_>) =
            caches.into_values().partition(|c| c.cache.is_some());
        named.extend(missed);
        stats.magnets.caches = named;

        let rows = sqlx::query(
            r#"
            SELECT kind, COUNT(*) AS n FROM release_reports
            WHERE created_at >= datetime(?, 'unixepoch')
            GROUP BY kind
            "#,
        )
        .bind(from)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to count release reports"))?;
        for row in rows {
            let count: i64 = row.get("n");
            match row.get::<String, _>("kind").as_str() {
                "rejected" => stats.reports.rejected += count,
                "flagged_fake" => stats.reports.flagged_fake += count,
                _ => stats.reports.flagged_not_fake += count,
            }
        }

        // Traits of releases known to be fake: rejections and "Fake" flags
        stats.reports.top_types = sqlx::query_as(
            r#"
            SELECT lower(json_extract(t.value, '$.ext')) AS ext, COUNT(DISTINCT r.id) AS n
            FROM release_reports r, json_each(r.types) t
            WHERE r.kind IN ('rejected', 'flagged_fake')
              AND r.created_at >= datetime(?, 'unixepoch')
              AND json_extract(t.value, '$.ext') IS NOT NULL
            GROUP BY ext ORDER BY n DESC, ext LIMIT ?
            "#,
        )
        .bind(from)
        .bind(TOP_TYPES)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to count report file types"))?
        .into_iter()
        .map(|(ext, count)| TypeCount { ext, count })
        .collect();
        stats.reports.top_trackers = sqlx::query_as(
            r#"
            SELECT lower(t.value) AS host, COUNT(DISTINCT r.id) AS n
            FROM release_reports r, json_each(r.tracker_hosts) t
            WHERE r.kind IN ('rejected', 'flagged_fake')
              AND r.created_at >= datetime(?, 'unixepoch')
            GROUP BY host ORDER BY n DESC, host LIMIT ?
            "#,
        )
        .bind(from)
        .bind(TOP_TRACKERS)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to count report trackers"))?
        .into_iter()
        .map(|(host, count)| TrackerCount { host, count })
        .collect();

        Ok(stats)
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

    fn flag(
        kind: ReportKind,
        file_id: Option<i64>,
        note: Option<&'static str>,
    ) -> NewReport<'static> {
        NewReport {
            kind,
            file_id,
            name: "Movie.2024",
            note,
            ..NewReport::default()
        }
    }

    #[tokio::test]
    async fn a_second_flag_replaces_the_note_and_undo_deletes_it() {
        let (_dir, db) = db().await;
        let repo = db.releases();
        let fake = flag(ReportKind::FlaggedFake, Some(7), Some("archive"));
        let id = repo.upsert_flag(&fake).await.unwrap();
        let again = flag(ReportKind::FlaggedFake, Some(7), Some("password"));
        assert_eq!(repo.upsert_flag(&again).await.unwrap(), id);
        let other = repo
            .upsert_flag(&flag(ReportKind::FlaggedNotFake, Some(7), None))
            .await
            .unwrap();
        assert_ne!(other, id);

        let flags = repo.flags_for(&[], &[7, 8], &[], &[]).await.unwrap();
        assert_eq!(flags.len(), 2);
        assert_eq!(flags[0].note.as_deref(), Some("password"));

        let rejected = repo
            .insert_report(&NewReport {
                hash: Some(HASH),
                ..flag(ReportKind::Rejected, None, None)
            })
            .await
            .unwrap();
        assert_eq!(
            repo.delete_flag(rejected).await.unwrap(),
            FlagUndo::NotAFlag
        );
        assert_eq!(repo.delete_flag(id).await.unwrap(), FlagUndo::Deleted);
        assert_eq!(repo.delete_flag(id).await.unwrap(), FlagUndo::NotFound);
        assert!(repo.was_rejected(None, Some(HASH)).await.unwrap());
        assert!(!repo.was_rejected(Some(HASH), None).await.unwrap());
        assert!(!repo.was_rejected(None, None).await.unwrap());
    }

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
    async fn held_checks_settle_once_and_take_a_transfer() {
        let (_dir, db) = db().await;
        let repo = db.releases();
        let held = NewCheck {
            transfer_id: None,
            ..check(None)
        };
        let id = repo.insert_check(&held).await.unwrap();
        // No transfer yet: the check on Put.io leaves it alone
        assert!(repo.pending_transfer_ids().await.unwrap().is_empty());

        let settle = HeldSettle {
            state: CheckState::Passed,
            lookup: Some("found"),
            lookup_cache: Some("itorrents.net"),
            total_size: 2048,
            reason: None,
            transfer_id: Some(11),
        };
        assert!(repo.settle_held(id, &settle).await.unwrap());
        assert!(!repo.settle_held(id, &settle).await.unwrap());
        let (state, lookup, transfer): (String, String, i64) =
            sqlx::query_as("SELECT state, lookup, transfer_id FROM release_checks WHERE id = ?")
                .bind(id)
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(
            (state.as_str(), lookup.as_str(), transfer),
            ("passed", "found", 11)
        );

        let ran_out = repo.insert_check(&held).await.unwrap();
        assert!(repo.set_check_transfer(ran_out, 12).await.unwrap());
        assert_eq!(repo.pending_transfer_ids().await.unwrap(), vec![12]);
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

const REPORT_COLUMNS: &str = "id, kind, check_id, arr_hash, file_id, transfer_id, source, \
     category, watch_folder, stage, name, hash, total_size, file_count, types, largest_wanted, \
     min_size, tracker_hosts, is_private, reason, note, created_at";

fn report_from_row(row: &sqlx::sqlite::SqliteRow) -> ReportRow {
    ReportRow {
        id: row.get("id"),
        kind: row.get("kind"),
        check_id: row.get("check_id"),
        arr_hash: row.get("arr_hash"),
        file_id: row.get("file_id"),
        transfer_id: row.get("transfer_id"),
        source: row.get("source"),
        category: row.get("category"),
        watch_folder: row.get("watch_folder"),
        stage: row.get("stage"),
        name: row.get("name"),
        hash: row.get("hash"),
        total_size: row.get("total_size"),
        file_count: row.get("file_count"),
        types: json_list(row, "types"),
        largest_wanted: row.get("largest_wanted"),
        min_size: row.get("min_size"),
        tracker_hosts: json_list(row, "tracker_hosts"),
        is_private: row.get("is_private"),
        reason: row.get("reason"),
        note: row.get("note"),
        created_at: row.get("created_at"),
    }
}

/// A flag's release key
enum Key<'a> {
    Text(&'a str),
    Int(i64),
}

/// What [`ReleasesRepository::delete_flag`] did
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagUndo {
    Deleted,
    NotFound,
    /// The row is a `rejected` report, which can't be undone
    NotAFlag,
}

/// A JSON list column; empty when it doesn't parse
fn json_list<T: serde::de::DeserializeOwned>(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Vec<T> {
    serde_json::from_str(&row.get::<String, _>(column)).unwrap_or_default()
}

/// One file type in a report
#[derive(Debug, Clone, Default, PartialEq, Serialize, serde::Deserialize)]
pub struct ReportType {
    pub ext: String,
    pub count: i64,
    pub largest: i64,
}

/// One `release_reports` row, with `types` and `tracker_hosts` as lists
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ReportRow {
    pub id: i64,
    pub kind: String,
    pub check_id: Option<i64>,
    pub arr_hash: Option<String>,
    pub file_id: Option<i64>,
    pub transfer_id: Option<i64>,
    pub source: Option<String>,
    pub category: Option<String>,
    pub watch_folder: Option<String>,
    pub stage: Option<String>,
    pub name: String,
    pub hash: Option<String>,
    pub total_size: Option<i64>,
    pub file_count: Option<i64>,
    pub types: Vec<ReportType>,
    pub largest_wanted: Option<i64>,
    pub min_size: Option<i64>,
    pub tracker_hosts: Vec<String>,
    pub is_private: bool,
    pub reason: Option<String>,
    pub note: Option<String>,
    /// UTC, `YYYY-MM-DD HH:MM:SS`
    pub created_at: String,
}

/// `GET /stats/releases` (contracts/api.md)
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ReleaseStats {
    pub range: StatsRange,
    pub rejected: RejectedStats,
    pub magnets: MagnetStats,
    pub reports: ReportStats,
}

/// The range counted, UTC epoch seconds; `from` is `None` for everything
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StatsRange {
    pub preset: &'static str,
    pub from: Option<i64>,
    pub to: i64,
}

/// Releases and bytes
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CountBytes {
    pub count: i64,
    pub bytes: i64,
}

/// Rejections by where the check ran
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ByStage {
    pub before: CountBytes,
    pub putio: CountBytes,
}

/// Rejections with one reason
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ReasonCount {
    pub reason: String,
    pub count: i64,
}

/// `rejected` section
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RejectedStats {
    pub count: i64,
    pub bytes: i64,
    pub by_stage: ByStage,
    /// Most first
    pub by_reason: Vec<ReasonCount>,
}

/// Lookup results for one cache. `cache` is `None` for misses, which aren't kept per
/// cache (the per-cache split is in `putio_sync_precheck_cache_lookups_total`).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CacheCount {
    pub cache: Option<String>,
    pub found: i64,
    pub not_found: i64,
    pub timeout: i64,
    pub wrong_hash: i64,
    pub error: i64,
}

/// `magnets` section
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MagnetStats {
    /// Magnets with an info hash
    pub checked: i64,
    /// .torrent found in a cache
    pub found: i64,
    /// Sent to Put.io unchecked
    pub fell_back: i64,
    /// Fallbacks rejected on Put.io
    pub fallback_fake: i64,
    /// What Put.io downloaded for those
    pub fallback_putio_bytes: i64,
    pub caches: Vec<CacheCount>,
}

/// Releases with one file type
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TypeCount {
    pub ext: String,
    pub count: i64,
}

/// Releases announced to one tracker
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TrackerCount {
    pub host: String,
    pub count: i64,
}

/// `reports` section
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ReportStats {
    pub rejected: i64,
    pub flagged_fake: i64,
    pub flagged_not_fake: i64,
    pub top_types: Vec<TypeCount>,
    pub top_trackers: Vec<TrackerCount>,
}
