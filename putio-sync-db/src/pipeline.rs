//! Active transfers pipeline (feature 039): holds on transfers still on Put.io, daemon
//! flags that survive a restart, Recently finished rows, and the file lookup the
//! pipeline is built from

use super::error::{tracked, DbResult};
use putio_sync_telemetry::DbOp;
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;

/// The daemon flag for the Pause NAS downloads switch (R11)
pub const NAS_DOWNLOADS_PAUSED: &str = "nas_downloads_paused";

/// A transfer paused while still on Put.io
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    pub transfer_id: i64,
    pub name: String,
    pub held_at: String,
}

/// Repository for `pipeline_holds`
pub struct HoldsRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> HoldsRepository<'a> {
    /// Create a new holds repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Hold a transfer; holding it again keeps the first hold
    pub async fn add(&self, transfer_id: i64, name: &str) -> DbResult<()> {
        sqlx::query("INSERT OR IGNORE INTO pipeline_holds (transfer_id, name) VALUES (?, ?)")
            .bind(transfer_id)
            .bind(name)
            .execute(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to hold transfer"))?;
        Ok(())
    }

    /// Release a hold. Returns whether there was one.
    pub async fn remove(&self, transfer_id: i64) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM pipeline_holds WHERE transfer_id = ?")
            .bind(transfer_id)
            .execute(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to release transfer hold"))?;
        Ok(result.rows_affected() > 0)
    }

    /// Every hold, by transfer id
    pub async fn list(&self) -> DbResult<Vec<Hold>> {
        let rows = sqlx::query(
            "SELECT transfer_id, name, held_at FROM pipeline_holds ORDER BY transfer_id",
        )
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to list transfer holds"))?;
        Ok(rows
            .into_iter()
            .map(|row| Hold {
                transfer_id: row.get("transfer_id"),
                name: row.get("name"),
                held_at: row.get::<Option<String>, _>("held_at").unwrap_or_default(),
            })
            .collect())
    }

    /// Whether a transfer is held
    pub async fn is_held(&self, transfer_id: i64) -> DbResult<bool> {
        let found: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM pipeline_holds WHERE transfer_id = ?")
                .bind(transfer_id)
                .fetch_optional(self.pool)
                .await
                .map_err(tracked(DbOp::Other, "Failed to check transfer hold"))?;
        Ok(found.is_some())
    }
}

/// Repository for `daemon_flags`
pub struct FlagsRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> FlagsRepository<'a> {
    /// Create a new flags repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// A flag's value; a missing key means off
    pub async fn get_bool(&self, key: &str) -> DbResult<bool> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM daemon_flags WHERE key = ?")
                .bind(key)
                .fetch_optional(self.pool)
                .await
                .map_err(tracked(DbOp::Other, "Failed to read daemon flag"))?;
        Ok(value.as_deref() == Some("1"))
    }

    /// Set a flag, stored as "1" or "0"
    pub async fn set_bool(&self, key: &str, value: bool) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT INTO daemon_flags (key, value) VALUES (?, ?)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(key)
        .bind(if value { "1" } else { "0" })
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to set daemon flag"))?;
        Ok(())
    }
}

/// How a Recently finished item ended
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishedOutcome {
    Done,
    Failed,
}

impl FinishedOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            FinishedOutcome::Done => "done",
            FinishedOutcome::Failed => "failed",
        }
    }
}

/// A Recently finished item (R12). Timestamps are SQLite's `YYYY-MM-DD HH:MM:SS`, UTC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedRow {
    /// The item key: `transfer:<id>`, `folder:<relative_path>` or `file:<file_id>`
    pub key: String,
    pub name: String,
    /// Bytes of the downloadable files
    pub size: i64,
    pub outcome: FinishedOutcome,
    /// Failure reason, for `Failed` only
    pub reason: Option<String>,
    /// When the transfer was added to Put.io; `None` for items with no transfer
    pub added_at: Option<String>,
    /// When Put.io finished the transfer
    pub putio_done_at: Option<String>,
    /// When its first file was queued
    pub queued_at: Option<String>,
    /// When it became done, or when a failed item left the active list
    pub done_at: String,
}

/// Repository for `pipeline_finished`
pub struct FinishedRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> FinishedRepository<'a> {
    /// Create a new Recently finished repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Write a row; a later write for the same key replaces it
    pub async fn upsert(&self, row: &FinishedRow) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO pipeline_finished
                (key, name, size, outcome, reason, added_at, putio_done_at, queued_at, done_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&row.key)
        .bind(&row.name)
        .bind(row.size)
        .bind(row.outcome.as_str())
        .bind(row.reason.as_deref())
        .bind(row.added_at.as_deref())
        .bind(row.putio_done_at.as_deref())
        .bind(row.queued_at.as_deref())
        .bind(&row.done_at)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to record finished item"))?;
        Ok(())
    }

    /// Every row, newest first
    pub async fn list(&self) -> DbResult<Vec<FinishedRow>> {
        let rows = sqlx::query(
            r#"
            SELECT key, name, size, outcome, reason, added_at, putio_done_at, queued_at, done_at
            FROM pipeline_finished ORDER BY done_at DESC, key
            "#,
        )
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to list finished items"))?;
        Ok(rows
            .into_iter()
            .map(|row| FinishedRow {
                key: row.get("key"),
                name: row.get("name"),
                size: row.get("size"),
                outcome: if row.get::<String, _>("outcome") == "failed" {
                    FinishedOutcome::Failed
                } else {
                    FinishedOutcome::Done
                },
                reason: row.get("reason"),
                added_at: row.get("added_at"),
                putio_done_at: row.get("putio_done_at"),
                queued_at: row.get("queued_at"),
                done_at: row.get("done_at"),
            })
            .collect())
    }

    /// Delete rows whose `done_at` is older than `hours`. Returns the count.
    pub async fn prune_older_than(&self, hours: u64) -> DbResult<usize> {
        let result =
            sqlx::query("DELETE FROM pipeline_finished WHERE done_at < datetime('now', ?)")
                .bind(format!("-{hours} hours"))
                .execute(self.pool)
                .await
                .map_err(tracked(DbOp::Other, "Failed to prune finished items"))?;
        Ok(result.rows_affected() as usize)
    }
}

/// Where a file of a pipeline item is (R1)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileState {
    /// It has a queue row, with this status
    Queued(String),
    /// No queue row, and a `completed` downloads row
    Done,
    /// Neither: the operator skipped it
    Skipped,
}

/// One file the pipeline is built from
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineFile {
    pub file_id: i64,
    /// From `download_sources`; `None` for a queue row with no transfer
    pub transfer_id: Option<i64>,
    pub state: FileState,
    /// Empty for a skipped file
    pub filename: String,
    /// Bytes; 0 for a skipped file
    pub size: i64,
    /// The folder it is written into, for queued files
    pub relative_path: Option<String>,
    /// `download_sources.origin`, when the file has a source row
    pub origin: Option<String>,
    /// When its source row was written (when it was first queued)
    pub recorded_at: Option<String>,
    /// When it finished downloading, for done files
    pub downloaded_at: Option<String>,
}

/// Read the files the pipeline is built from: every queued file, the files of
/// `transfer_ids` (the current Put.io list), and files done within `retention_hours` (R6)
pub(crate) async fn pipeline_files(
    pool: &SqlitePool,
    transfer_ids: &[i64],
    retention_hours: u64,
) -> DbResult<Vec<PipelineFile>> {
    let listed = if transfer_ids.is_empty() {
        "0".to_string()
    } else {
        format!(
            "s.transfer_id IN ({})",
            vec!["?"; transfer_ids.len()].join(", ")
        )
    };
    // Queue rows first, then source rows without a queue row. Done files are looked up
    // per file, through the downloads indexes, so the cost follows the active work.
    let sql = format!(
        r#"
        SELECT q.file_id, s.transfer_id, q.status AS queue_status, q.filename, q.file_size AS size,
               q.relative_path, s.origin, s.recorded_at, NULL AS downloaded_at
        FROM download_queue q
        LEFT JOIN download_sources s ON s.file_id = q.file_id
        UNION ALL
        SELECT f.file_id, f.transfer_id, NULL, COALESCE(d.filename, ''), COALESCE(d.file_size, 0),
               NULL, f.origin, f.recorded_at, d.downloaded_at
        FROM (
            SELECT s.file_id, s.transfer_id, s.origin, s.recorded_at
            FROM download_sources s
            WHERE NOT EXISTS (SELECT 1 FROM download_queue q WHERE q.file_id = s.file_id)
              AND ({listed} OR s.file_id IN (
                  SELECT file_id FROM downloads
                  WHERE status = 'completed' AND downloaded_at >= datetime('now', ?)
              ))
        ) f
        LEFT JOIN downloads d ON d.id = (
            SELECT id FROM downloads
            WHERE file_id = f.file_id AND status = 'completed'
            ORDER BY downloaded_at DESC LIMIT 1
        )
        "#
    );
    let mut query = sqlx::query(&sql);
    for id in transfer_ids {
        query = query.bind(id);
    }
    let rows = query
        .bind(format!("-{retention_hours} hours"))
        .fetch_all(pool)
        .await
        .map_err(tracked(DbOp::Queue, "Failed to read pipeline files"))?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let downloaded_at: Option<String> = row.get("downloaded_at");
            let state = match row.get::<Option<String>, _>("queue_status") {
                Some(status) => FileState::Queued(status),
                None if downloaded_at.is_some() => FileState::Done,
                None => FileState::Skipped,
            };
            PipelineFile {
                file_id: row.get("file_id"),
                transfer_id: row.get("transfer_id"),
                state,
                filename: row.get("filename"),
                size: row.get("size"),
                relative_path: row.get("relative_path"),
                origin: row.get("origin"),
                recorded_at: row.get("recorded_at"),
                downloaded_at,
            }
        })
        .collect())
}

/// The origin label of each of `transfer_ids` that putio-sync knows: `arr:<category>`
/// from `arr_items`, else `transfer_origins.origin`
pub(crate) async fn transfer_origins(
    pool: &SqlitePool,
    transfer_ids: &[i64],
) -> DbResult<HashMap<i64, String>> {
    if transfer_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let ids = vec!["?"; transfer_ids.len()].join(", ");
    // The arr rows sort last so they replace the transfer_origins label
    let sql = format!(
        r#"
        SELECT transfer_id, origin, 0 AS rank FROM transfer_origins WHERE transfer_id IN ({ids})
        UNION ALL
        SELECT transfer_id, 'arr:' || category, 1 FROM arr_items WHERE transfer_id IN ({ids})
        ORDER BY rank
        "#
    );
    let mut query = sqlx::query(&sql);
    for _ in 0..2 {
        for id in transfer_ids {
            query = query.bind(id);
        }
    }
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to read transfer origins"))?;
    Ok(rows
        .into_iter()
        .map(|row| (row.get::<i64, _>(0), row.get::<String, _>(1)))
        .collect())
}
