//! The pipeline's tables (feature 038): items, their steps and every attempt at a step.
//!
//! Timestamps are SQLite's `YYYY-MM-DD HH:MM:SS` in UTC, as in the other tables.

use sqlx::{sqlite::SqliteRow, Row, SqlitePool};

use crate::model::{AttemptResult, ItemState, SkipReason, Stage, StepKind, StepState};
use crate::plan::PlannedStep;
use putio_sync_hooks::step::tail;

pub type Result<T> = std::result::Result<T, sqlx::Error>;

/// One item the pipeline follows
#[derive(Debug, Clone, PartialEq)]
pub struct ItemRow {
    pub key: String,
    pub name: String,
    pub category: Option<String>,
    /// `arr`, `watch` or `sync`
    pub source: String,
    pub dir: String,
    pub transfer_id: Option<i64>,
    pub stage: Stage,
    pub state: ItemState,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
}

/// What a new item needs
#[derive(Debug, Clone)]
pub struct NewItem {
    pub key: String,
    pub name: String,
    pub category: Option<String>,
    pub source: String,
    pub dir: String,
    pub transfer_id: Option<i64>,
    pub stage: Stage,
    pub state: ItemState,
}

/// Provenance rows older than this many days are pruned
pub const PROVENANCE_DAYS: i64 = 90;

/// Where a saved subtitle file came from
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub path: String,
    pub language: String,
    /// `putio_embedded`, `putio_folder`, `opensubtitles`, `transcribed` or `translated`
    pub source: String,
    pub bytes: i64,
}

/// One translation job sent to a callback service (feature 026). The token is never stored,
/// only its SHA-256.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackJob {
    pub token_hash: String,
    pub job_id: String,
    pub step_id: i64,
    pub attempt_id: i64,
    pub source_lang: String,
    pub target_lang: String,
    pub source_path: String,
    pub source_sha256: String,
    pub target_path: String,
    /// `waiting`, `done`, `failed` or `expired`
    pub state: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub finished_at: Option<i64>,
}

fn callback_job(row: &SqliteRow) -> CallbackJob {
    CallbackJob {
        token_hash: row.get("token_hash"),
        job_id: row.get("job_id"),
        step_id: row.get("step_id"),
        attempt_id: row.get("attempt_id"),
        source_lang: row.get("source_lang"),
        target_lang: row.get("target_lang"),
        source_path: row.get("source_path"),
        source_sha256: row.get("source_sha256"),
        target_path: row.get("target_path"),
        state: row.get("state"),
        created_at: row.get("created_at"),
        expires_at: row.get("expires_at"),
        finished_at: row.get("finished_at"),
    }
}

/// Steps that a restart interrupts (they run again) unless a callback job holds them
const RESTARTABLE: &str = "('hook', 'subtitle_scan', 'subtitles', 'translate_scan', 'translate')";

/// One planned step and where it is
#[derive(Debug, Clone, PartialEq)]
pub struct StepRow {
    pub id: i64,
    pub item_key: String,
    pub stage: Stage,
    pub branch: Option<String>,
    pub position: i64,
    pub kind: StepKind,
    pub name: String,
    pub required: bool,
    pub changes_files: bool,
    pub state: StepState,
    pub skip_reason: Option<SkipReason>,
    pub next_attempt_at: Option<String>,
    pub progress: Option<f64>,
}

/// One run of a step
#[derive(Debug, Clone, PartialEq)]
pub struct AttemptRow {
    pub id: i64,
    pub step_id: i64,
    pub n: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub result: Option<AttemptResult>,
    pub exit_code: Option<i32>,
    pub output: String,
}

/// Access to the pipeline tables
#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

fn item(row: &SqliteRow) -> ItemRow {
    ItemRow {
        key: row.get("key"),
        name: row.get("name"),
        category: row.get("category"),
        source: row.get("source"),
        dir: row.get("dir"),
        transfer_id: row.get("transfer_id"),
        stage: Stage::parse(&row.get::<String, _>("stage")).unwrap_or(Stage::PreProcessing),
        state: ItemState::parse(&row.get::<String, _>("state")).unwrap_or(ItemState::Failed),
        created_at: row
            .get::<Option<String>, _>("created_at")
            .unwrap_or_default(),
        updated_at: row
            .get::<Option<String>, _>("updated_at")
            .unwrap_or_default(),
        finished_at: row.get("finished_at"),
    }
}

fn step(row: &SqliteRow) -> StepRow {
    StepRow {
        id: row.get("id"),
        item_key: row.get("item_key"),
        stage: Stage::parse(&row.get::<String, _>("stage")).unwrap_or(Stage::PreProcessing),
        branch: row.get("branch"),
        position: row.get("position"),
        kind: StepKind::parse(&row.get::<String, _>("kind")).unwrap_or(StepKind::Hook),
        name: row.get("name"),
        required: row.get::<i64, _>("required") != 0,
        changes_files: row.get::<i64, _>("changes_files") != 0,
        state: StepState::parse(&row.get::<String, _>("state")).unwrap_or(StepState::Failed),
        skip_reason: row
            .get::<Option<String>, _>("skip_reason")
            .and_then(|s| SkipReason::parse(&s)),
        next_attempt_at: row.get("next_attempt_at"),
        progress: row.get("progress"),
    }
}

fn attempt(row: &SqliteRow) -> AttemptRow {
    AttemptRow {
        id: row.get("id"),
        step_id: row.get("step_id"),
        n: row.get("n"),
        started_at: row
            .get::<Option<String>, _>("started_at")
            .unwrap_or_default(),
        ended_at: row.get("ended_at"),
        result: row
            .get::<Option<String>, _>("result")
            .and_then(|s| AttemptResult::parse(&s)),
        exit_code: row.get("exit_code"),
        output: row.get("output"),
    }
}

impl Store {
    pub fn new(pool: SqlitePool) -> Self {
        Store { pool }
    }

    /// Add an item with its planned steps, in one transaction. An item already there is
    /// left as it is: returns `false`.
    pub async fn insert_item(&self, new: &NewItem, steps: &[PlannedStep]) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let done = sqlx::query(
            "INSERT OR IGNORE INTO pipeline_items \
             (key, name, category, source, dir, transfer_id, stage, state) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&new.key)
        .bind(&new.name)
        .bind(&new.category)
        .bind(&new.source)
        .bind(&new.dir)
        .bind(new.transfer_id)
        .bind(new.stage.as_str())
        .bind(new.state.as_str())
        .execute(&mut *tx)
        .await?;
        if done.rows_affected() == 0 {
            return Ok(false);
        }
        for s in steps {
            sqlx::query(
                "INSERT INTO pipeline_steps \
                 (item_key, stage, branch, position, kind, name, required, changes_files, state, skip_reason) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&new.key)
            .bind(s.stage.as_str())
            .bind(&s.branch)
            .bind(s.position)
            .bind(s.kind.as_str())
            .bind(&s.name)
            .bind(s.required)
            .bind(s.changes_files)
            .bind(s.state.as_str())
            .bind(s.skip_reason.map(|r| r.as_str()))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    /// Add steps to an item that already exists (run-time expansion, such as one subtitles
    /// step per video). A step with the same stage, branch, kind and name is not added
    /// twice, so a scan that runs again changes nothing. Returns how many were added.
    pub async fn insert_steps(&self, item_key: &str, steps: &[PlannedStep]) -> Result<usize> {
        // Read-then-write, so the write lock comes first (see `start_attempt`)
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let mut added = 0;
        for s in steps {
            let exists: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM pipeline_steps WHERE item_key = ? AND stage = ? \
                 AND COALESCE(branch, '') = COALESCE(?, '') AND kind = ? AND name = ?",
            )
            .bind(item_key)
            .bind(s.stage.as_str())
            .bind(&s.branch)
            .bind(s.kind.as_str())
            .bind(&s.name)
            .fetch_optional(&mut *tx)
            .await?;
            if exists.is_some() {
                continue;
            }
            sqlx::query(
                "INSERT INTO pipeline_steps \
                 (item_key, stage, branch, position, kind, name, required, changes_files, state, skip_reason) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(item_key)
            .bind(s.stage.as_str())
            .bind(&s.branch)
            .bind(s.position)
            .bind(s.kind.as_str())
            .bind(&s.name)
            .bind(s.required)
            .bind(s.changes_files)
            .bind(s.state.as_str())
            .bind(s.skip_reason.map(|r| r.as_str()))
            .execute(&mut *tx)
            .await?;
            added += 1;
        }
        tx.commit().await?;
        Ok(added)
    }

    pub async fn item(&self, key: &str) -> Result<Option<ItemRow>> {
        Ok(sqlx::query("SELECT * FROM pipeline_items WHERE key = ?")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| item(&r)))
    }

    /// Every item, oldest first
    pub async fn items(&self) -> Result<Vec<ItemRow>> {
        Ok(
            sqlx::query("SELECT * FROM pipeline_items ORDER BY created_at, key")
                .fetch_all(&self.pool)
                .await?
                .iter()
                .map(item)
                .collect(),
        )
    }

    /// Move an item to a stage and state; a terminal state also stamps `finished_at`
    pub async fn set_item(&self, key: &str, stage: Stage, state: ItemState) -> Result<()> {
        sqlx::query(
            "UPDATE pipeline_items SET stage = ?, state = ?, updated_at = CURRENT_TIMESTAMP, \
             finished_at = CASE WHEN ? THEN COALESCE(finished_at, CURRENT_TIMESTAMP) ELSE NULL END \
             WHERE key = ?",
        )
        .bind(stage.as_str())
        .bind(state.as_str())
        .bind(state.is_terminal())
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Set the item folder (the staging path while staging, then the final one)
    pub async fn set_dir(&self, key: &str, dir: &str) -> Result<()> {
        sqlx::query(
            "UPDATE pipeline_items SET dir = ?, updated_at = CURRENT_TIMESTAMP WHERE key = ?",
        )
        .bind(dir)
        .bind(key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Set the category and name once the files are known
    pub async fn set_category(&self, key: &str, category: Option<&str>) -> Result<()> {
        sqlx::query("UPDATE pipeline_items SET category = ? WHERE key = ?")
            .bind(category)
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// An item's steps in running order
    pub async fn steps(&self, key: &str) -> Result<Vec<StepRow>> {
        Ok(
            sqlx::query("SELECT * FROM pipeline_steps WHERE item_key = ? ORDER BY id")
                .bind(key)
                .fetch_all(&self.pool)
                .await?
                .iter()
                .map(step)
                .collect(),
        )
    }

    pub async fn step(&self, id: i64) -> Result<Option<StepRow>> {
        Ok(sqlx::query("SELECT * FROM pipeline_steps WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| step(&r)))
    }

    /// Set a step's state; `next_attempt_in` is seconds from now
    pub async fn set_step(
        &self,
        id: i64,
        state: StepState,
        skip_reason: Option<SkipReason>,
        next_attempt_in: Option<u64>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE pipeline_steps SET state = ?, skip_reason = ?, \
             next_attempt_at = CASE WHEN ? IS NULL THEN NULL ELSE datetime('now', '+' || ? || ' seconds') END \
             WHERE id = ?",
        )
        .bind(state.as_str())
        .bind(skip_reason.map(|r| r.as_str()))
        .bind(next_attempt_in.map(|n| n as i64))
        .bind(next_attempt_in.map(|n| n as i64))
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_progress(&self, id: i64, progress: Option<f64>) -> Result<()> {
        sqlx::query("UPDATE pipeline_steps SET progress = ? WHERE id = ?")
            .bind(progress)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Open an attempt for a step and mark the step running. Returns the attempt id and
    /// its number, from 1.
    pub async fn start_attempt(&self, step_id: i64) -> Result<(i64, i64)> {
        // Read-then-write: take the write lock up front, so two branches starting at once
        // wait for each other instead of one failing with "database is locked"
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) + 1 FROM pipeline_attempts WHERE step_id = ?")
                .bind(step_id)
                .fetch_one(&mut *tx)
                .await?;
        let id = sqlx::query("INSERT INTO pipeline_attempts (step_id, n) VALUES (?, ?)")
            .bind(step_id)
            .bind(n)
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
        sqlx::query(
            "UPDATE pipeline_steps SET state = 'running', next_attempt_at = NULL WHERE id = ?",
        )
        .bind(step_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok((id, n))
    }

    /// Close an attempt, keeping the tail of its output (the last 200 lines or 64 KB)
    pub async fn end_attempt(
        &self,
        id: i64,
        result: AttemptResult,
        exit_code: Option<i32>,
        output: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE pipeline_attempts SET ended_at = CURRENT_TIMESTAMP, result = ?, exit_code = ?, output = ? \
             WHERE id = ?",
        )
        .bind(result.as_str())
        .bind(exit_code)
        .bind(tail(output))
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// An item's attempts at a step, oldest first
    pub async fn attempts(&self, step_id: i64) -> Result<Vec<AttemptRow>> {
        Ok(
            sqlx::query("SELECT * FROM pipeline_attempts WHERE step_id = ? ORDER BY n")
                .bind(step_id)
                .fetch_all(&self.pool)
                .await?
                .iter()
                .map(attempt)
                .collect(),
        )
    }

    /// At startup: steps that were running when the daemon stopped become `interrupted`,
    /// and their open attempts are closed. A step that waits for a callback (a `waiting`
    /// job) stays `running` with its attempt open: the service may still call back.
    /// Returns how many steps were interrupted.
    pub async fn interrupt_running(&self) -> Result<u64> {
        sqlx::query(
            "UPDATE pipeline_attempts SET ended_at = CURRENT_TIMESTAMP, result = 'interrupted' \
             WHERE ended_at IS NULL AND step_id NOT IN \
             (SELECT step_id FROM translation_callbacks WHERE state = 'waiting')",
        )
        .execute(&self.pool)
        .await?;
        let n = sqlx::query(&format!(
            "UPDATE pipeline_steps SET state = 'interrupted' WHERE state = 'running' \
             AND kind IN {RESTARTABLE} AND id NOT IN \
             (SELECT step_id FROM translation_callbacks WHERE state = 'waiting')"
        ))
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(n)
    }

    /// Record a job before the request is sent, so a callback that arrives before the reply
    /// is read finds it
    pub async fn insert_job(&self, job: &CallbackJob) -> Result<()> {
        sqlx::query(
            "INSERT INTO translation_callbacks (token_hash, job_id, step_id, attempt_id, \
             source_lang, target_lang, source_path, source_sha256, target_path, state, \
             created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'waiting', ?, ?)",
        )
        .bind(&job.token_hash)
        .bind(&job.job_id)
        .bind(job.step_id)
        .bind(job.attempt_id)
        .bind(&job.source_lang)
        .bind(&job.target_lang)
        .bind(&job.source_path)
        .bind(&job.source_sha256)
        .bind(&job.target_path)
        .bind(job.created_at)
        .bind(job.expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Use up a token: one atomic update, so of any number of callers with the same token
    /// exactly one gets the job. A token that is unknown, used, expired by state or past
    /// `expires_at` gets nothing.
    pub async fn claim(&self, token_hash: &str, now: i64) -> Result<Option<CallbackJob>> {
        Ok(sqlx::query(
            "UPDATE translation_callbacks SET state = 'done', finished_at = ? \
             WHERE token_hash = ? AND state = 'waiting' AND expires_at > ? RETURNING *",
        )
        .bind(now)
        .bind(token_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| callback_job(&r)))
    }

    /// A job by token hash
    pub async fn job(&self, token_hash: &str) -> Result<Option<CallbackJob>> {
        Ok(
            sqlx::query("SELECT * FROM translation_callbacks WHERE token_hash = ?")
                .bind(token_hash)
                .fetch_optional(&self.pool)
                .await?
                .map(|r| callback_job(&r)),
        )
    }

    /// The `waiting` job of a step, if it has one
    pub async fn waiting_job(&self, step_id: i64) -> Result<Option<CallbackJob>> {
        Ok(sqlx::query(
            "SELECT * FROM translation_callbacks WHERE step_id = ? AND state = 'waiting' \
             ORDER BY created_at DESC LIMIT 1",
        )
        .bind(step_id)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| callback_job(&r)))
    }

    /// Every job that is `waiting`
    pub async fn waiting_jobs(&self) -> Result<Vec<CallbackJob>> {
        Ok(
            sqlx::query("SELECT * FROM translation_callbacks WHERE state = 'waiting'")
                .fetch_all(&self.pool)
                .await?
                .iter()
                .map(callback_job)
                .collect(),
        )
    }

    /// Settle a job that was claimed or sent but did not end well (`failed`), or whose
    /// request never got a usable answer
    pub async fn finish_job(&self, token_hash: &str, state: &str, now: i64) -> Result<()> {
        sqlx::query(
            "UPDATE translation_callbacks SET state = ?, finished_at = ? WHERE token_hash = ?",
        )
        .bind(state)
        .bind(now)
        .bind(token_hash)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Jobs whose time is up become `expired`; returns them
    pub async fn expire_due(&self, now: i64) -> Result<Vec<CallbackJob>> {
        Ok(sqlx::query(
            "UPDATE translation_callbacks SET state = 'expired', finished_at = ? \
             WHERE state = 'waiting' AND expires_at <= ? RETURNING *",
        )
        .bind(now)
        .bind(now)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(callback_job)
        .collect())
    }

    /// The `waiting` jobs of one step become `expired`; returns them
    pub async fn expire_for_step(&self, step_id: i64, now: i64) -> Result<Vec<CallbackJob>> {
        Ok(sqlx::query(
            "UPDATE translation_callbacks SET state = 'expired', finished_at = ? \
             WHERE state = 'waiting' AND step_id = ? RETURNING *",
        )
        .bind(now)
        .bind(step_id)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(callback_job)
        .collect())
    }

    /// The `waiting` jobs of every step of an item become `expired`; returns them
    pub async fn expire_for_item(&self, item_key: &str, now: i64) -> Result<Vec<CallbackJob>> {
        Ok(sqlx::query(
            "UPDATE translation_callbacks SET state = 'expired', finished_at = ? \
             WHERE state = 'waiting' AND step_id IN \
             (SELECT id FROM pipeline_steps WHERE item_key = ?) RETURNING *",
        )
        .bind(now)
        .bind(item_key)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(callback_job)
        .collect())
    }

    /// At startup, after the steps are interrupted: a `waiting` job whose step is no longer
    /// `running` can never be answered usefully. Returns how many.
    pub async fn expire_orphans(&self, now: i64) -> Result<u64> {
        Ok(sqlx::query(
            "UPDATE translation_callbacks SET state = 'expired', finished_at = ? \
             WHERE state = 'waiting' AND step_id NOT IN \
             (SELECT id FROM pipeline_steps WHERE state = 'running')",
        )
        .bind(now)
        .execute(&self.pool)
        .await?
        .rows_affected())
    }

    /// Steps that can run now: waiting or interrupted, with no retry time or one that has
    /// passed, for items that are running
    pub async fn due_steps(&self) -> Result<Vec<StepRow>> {
        Ok(sqlx::query(
            "SELECT s.* FROM pipeline_steps s JOIN pipeline_items i ON i.key = s.item_key \
             WHERE i.state = 'running' AND s.state IN ('waiting', 'interrupted') \
             AND (s.next_attempt_at IS NULL OR s.next_attempt_at <= datetime('now')) \
             ORDER BY s.id",
        )
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(step)
        .collect())
    }

    /// Remember where a saved sidecar came from (features 025 and 026). Saving over the
    /// same path replaces the row.
    pub async fn upsert_provenance(&self, row: &Provenance) -> Result<()> {
        sqlx::query(
            "INSERT INTO subtitle_provenance (path, language, source, bytes, created_at) \
             VALUES (?, ?, ?, ?, strftime('%s','now')) \
             ON CONFLICT(path) DO UPDATE SET language = excluded.language, \
             source = excluded.source, bytes = excluded.bytes, created_at = excluded.created_at",
        )
        .bind(&row.path)
        .bind(&row.language)
        .bind(&row.source)
        .bind(row.bytes)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Where the sidecar at `path` came from. A row whose recorded size differs from
    /// `bytes` is stale (the file was replaced by hand) and counts as unknown.
    pub async fn provenance(&self, path: &str, bytes: Option<i64>) -> Result<Option<Provenance>> {
        let row = sqlx::query(
            "SELECT path, language, source, bytes FROM subtitle_provenance WHERE path = ?",
        )
        .bind(path)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row
            .map(|r| Provenance {
                path: r.get("path"),
                language: r.get("language"),
                source: r.get("source"),
                bytes: r.get("bytes"),
            })
            .filter(|p| bytes.is_none_or(|b| b == p.bytes)))
    }

    /// Staging rename: rows whose path starts with `from` now live under `to`. Returns how
    /// many were re-keyed.
    pub async fn rekey_provenance(&self, from: &str, to: &str) -> Result<u64> {
        let from = from.trim_end_matches('/');
        let to = to.trim_end_matches('/');
        Ok(sqlx::query(
            "UPDATE OR REPLACE subtitle_provenance SET path = ? || substr(path, ?) \
             WHERE path = ? OR substr(path, 1, ?) = ? || '/'",
        )
        .bind(to)
        .bind(from.chars().count() as i64 + 1)
        .bind(from)
        .bind(from.chars().count() as i64 + 1)
        .bind(from)
        .execute(&self.pool)
        .await?
        .rows_affected())
    }

    /// Remember the Put.io files an item downloaded, so History can find the item (FR-024)
    pub async fn link_files(&self, key: &str, file_ids: &[i64]) -> Result<()> {
        for id in file_ids {
            sqlx::query(
                "INSERT OR IGNORE INTO pipeline_item_files (item_key, file_id) VALUES (?, ?)",
            )
            .bind(key)
            .bind(id)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Drop finished items older than `history_days`, with their steps and attempts, and
    /// clear the output of attempts whose item finished more than `output_hours` ago
    pub async fn prune(&self, history_days: u32, output_hours: u64) -> Result<u64> {
        let n = sqlx::query(
            "DELETE FROM pipeline_items WHERE finished_at IS NOT NULL \
             AND finished_at < datetime('now', '-' || ? || ' days')",
        )
        .bind(history_days as i64)
        .execute(&self.pool)
        .await?
        .rows_affected();
        sqlx::query(
            "UPDATE pipeline_attempts SET output = '' WHERE output != '' AND step_id IN \
             (SELECT s.id FROM pipeline_steps s JOIN pipeline_items i ON i.key = s.item_key \
              WHERE i.finished_at IS NOT NULL \
              AND i.finished_at < datetime('now', '-' || ? || ' hours'))",
        )
        .bind(output_hours as i64)
        .execute(&self.pool)
        .await?;
        sqlx::query("DELETE FROM subtitle_provenance WHERE created_at < strftime('%s','now') - ?")
            .bind(PROVENANCE_DAYS * 86_400)
            .execute(&self.pool)
            .await?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{plan, PlanInput};
    use putio_sync_core::hooks::{HookConfig, HookStage};
    use putio_sync_db::Database;

    async fn store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::new(&dir.path().join("t.db")).await.unwrap();
        (Store::new(db.pool().clone()), dir)
    }

    fn new_item(key: &str) -> NewItem {
        NewItem {
            key: key.to_string(),
            name: "Some.Show.S01E01".to_string(),
            category: Some("sonarr".to_string()),
            source: "arr".to_string(),
            dir: "/data/sonarr/Some.Show.S01E01".to_string(),
            transfer_id: Some(1),
            stage: Stage::PreProcessing,
            state: ItemState::Running,
        }
    }

    fn steps() -> Vec<PlannedStep> {
        let hook = HookConfig {
            name: "tidy".to_string(),
            stage: Some(HookStage::PostProcessing),
            command: Some(vec!["/bin/true".to_string()]),
            ..Default::default()
        };
        plan(
            PlanInput {
                category: Some("sonarr"),
                has_transfer: true,
                downloaded: true,
            },
            &[hook],
        )
    }

    #[tokio::test]
    async fn an_item_and_its_steps_round_trip_and_insert_is_idempotent() {
        let (store, _dir) = store().await;
        assert!(store
            .insert_item(&new_item("transfer:1"), &steps())
            .await
            .unwrap());
        assert!(!store
            .insert_item(&new_item("transfer:1"), &steps())
            .await
            .unwrap());
        let item = store.item("transfer:1").await.unwrap().unwrap();
        assert_eq!(item.stage, Stage::PreProcessing);
        assert_eq!(item.state, ItemState::Running);
        let rows = store.steps("transfer:1").await.unwrap();
        assert_eq!(rows.len(), 3, "download, tidy, notify");
        assert_eq!(rows[0].state, StepState::Done);
        assert_eq!(rows[1].name, "tidy");
        assert!(rows[1].changes_files && rows[1].required);
    }

    #[tokio::test]
    async fn attempts_count_up_and_keep_only_the_tail_of_the_output() {
        let (store, _dir) = store().await;
        store
            .insert_item(&new_item("transfer:1"), &steps())
            .await
            .unwrap();
        let tidy = store.steps("transfer:1").await.unwrap()[1].id;
        let (a1, n1) = store.start_attempt(tidy).await.unwrap();
        assert_eq!(n1, 1);
        assert_eq!(
            store.step(tidy).await.unwrap().unwrap().state,
            StepState::Running
        );
        let long: String = (1..=500).map(|n| format!("line {n}\n")).collect();
        store
            .end_attempt(a1, AttemptResult::Failed, Some(3), &long)
            .await
            .unwrap();
        let (_, n2) = store.start_attempt(tidy).await.unwrap();
        assert_eq!(n2, 2);
        let attempts = store.attempts(tidy).await.unwrap();
        assert_eq!(attempts[0].result, Some(AttemptResult::Failed));
        assert_eq!(attempts[0].exit_code, Some(3));
        assert!(attempts[0].output.ends_with("line 500"));
        assert!(!attempts[0].output.contains("line 100\n"));
        assert_eq!(attempts[1].result, None, "still open");
    }

    #[tokio::test]
    async fn startup_marks_running_hook_steps_interrupted_and_they_become_due() {
        let (store, _dir) = store().await;
        store
            .insert_item(&new_item("transfer:1"), &steps())
            .await
            .unwrap();
        let tidy = store.steps("transfer:1").await.unwrap()[1].id;
        store.start_attempt(tidy).await.unwrap();
        assert!(store
            .due_steps()
            .await
            .unwrap()
            .iter()
            .all(|s| s.id != tidy));
        assert_eq!(store.interrupt_running().await.unwrap(), 1);
        let step = store.step(tidy).await.unwrap().unwrap();
        assert_eq!(step.state, StepState::Interrupted);
        let attempts = store.attempts(tidy).await.unwrap();
        assert_eq!(attempts[0].result, Some(AttemptResult::Interrupted));
        assert!(store
            .due_steps()
            .await
            .unwrap()
            .iter()
            .any(|s| s.id == tidy));
    }

    #[tokio::test]
    async fn a_retry_time_in_the_future_is_not_due_and_done_steps_never_are() {
        let (store, _dir) = store().await;
        store
            .insert_item(&new_item("transfer:1"), &steps())
            .await
            .unwrap();
        let tidy = store.steps("transfer:1").await.unwrap()[1].id;
        store
            .set_step(tidy, StepState::Waiting, None, Some(3600))
            .await
            .unwrap();
        assert!(store
            .due_steps()
            .await
            .unwrap()
            .iter()
            .all(|s| s.id != tidy));
        store
            .set_step(tidy, StepState::Waiting, None, Some(0))
            .await
            .unwrap();
        assert!(store
            .due_steps()
            .await
            .unwrap()
            .iter()
            .any(|s| s.id == tidy));
        store
            .set_step(tidy, StepState::Done, None, None)
            .await
            .unwrap();
        assert!(store
            .due_steps()
            .await
            .unwrap()
            .iter()
            .all(|s| s.id != tidy));
    }

    #[tokio::test]
    async fn held_items_have_no_due_steps() {
        let (store, _dir) = store().await;
        store
            .insert_item(&new_item("transfer:1"), &steps())
            .await
            .unwrap();
        store
            .set_item("transfer:1", Stage::PreProcessing, ItemState::Held)
            .await
            .unwrap();
        assert!(store.due_steps().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn prune_drops_old_finished_items_with_their_rows() {
        let (store, _dir) = store().await;
        store
            .insert_item(&new_item("transfer:1"), &steps())
            .await
            .unwrap();
        store
            .insert_item(&new_item("transfer:2"), &steps())
            .await
            .unwrap();
        store
            .set_item("transfer:1", Stage::Notifications, ItemState::Done)
            .await
            .unwrap();
        sqlx::query("UPDATE pipeline_items SET finished_at = datetime('now', '-3 days') WHERE key = 'transfer:1'")
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(store.prune(30, 24).await.unwrap(), 0, "kept for History");
        assert_eq!(store.prune(2, 24).await.unwrap(), 1);
        assert!(store.item("transfer:1").await.unwrap().is_none());
        assert!(store.steps("transfer:1").await.unwrap().is_empty());
        assert!(
            store.item("transfer:2").await.unwrap().is_some(),
            "still running"
        );
    }

    /// FR-024: a finished item keeps its timeline for History, but its output only a day
    #[tokio::test]
    async fn prune_clears_old_output_and_keeps_the_timeline() {
        let (store, _dir) = store().await;
        for key in ["transfer:1", "transfer:2"] {
            store.insert_item(&new_item(key), &steps()).await.unwrap();
            let step = store.steps(key).await.unwrap()[0].id;
            let (attempt, _) = store.start_attempt(step).await.unwrap();
            sqlx::query("UPDATE pipeline_attempts SET output = 'log' WHERE id = ?")
                .bind(attempt)
                .execute(&store.pool)
                .await
                .unwrap();
            store
                .set_item(key, Stage::Notifications, ItemState::Done)
                .await
                .unwrap();
        }
        store.link_files("transfer:1", &[7, 8, 7]).await.unwrap();
        sqlx::query("UPDATE pipeline_items SET finished_at = datetime('now', '-2 days') WHERE key = 'transfer:1'")
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(store.prune(30, 24).await.unwrap(), 0);
        let output = |key: &'static str| {
            let store = &store;
            async move {
                let step = store.steps(key).await.unwrap()[0].id;
                store.attempts(step).await.unwrap()[0].output.clone()
            }
        };
        assert_eq!(output("transfer:1").await, "", "older than a day");
        assert_eq!(output("transfer:2").await, "log", "finished just now");
        let linked: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pipeline_item_files")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(linked, 2, "each file once");
    }

    #[tokio::test]
    async fn a_terminal_state_stamps_finished_at() {
        let (store, _dir) = store().await;
        store
            .insert_item(&new_item("transfer:1"), &steps())
            .await
            .unwrap();
        assert!(store
            .item("transfer:1")
            .await
            .unwrap()
            .unwrap()
            .finished_at
            .is_none());
        store
            .set_item(
                "transfer:1",
                Stage::Notifications,
                ItemState::DoneWithWarnings,
            )
            .await
            .unwrap();
        assert!(store
            .item("transfer:1")
            .await
            .unwrap()
            .unwrap()
            .finished_at
            .is_some());
    }
}
