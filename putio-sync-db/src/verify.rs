//! Download verification (feature 034): "verify everything" runs and the files that did
//! not verify, for History's Verification panel and the doctor.

use super::error::{tracked, DbResult};
use putio_sync_telemetry::DbOp;
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

/// Runs kept; older ones are deleted with their problems
pub const RUNS_KEPT: i64 = 10;

/// How many files of a run ended each way
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, FromRow)]
pub struct RunCounts {
    pub verified: i64,
    pub mismatch: i64,
    pub missing: i64,
    pub not_on_putio: i64,
    pub busy: i64,
    pub unreadable: i64,
    pub not_checked: i64,
}

impl RunCounts {
    /// Add one file with `result` (`verified`, `mismatch`, …); unknown results are ignored
    pub fn add(&mut self, result: &str) {
        match result {
            "verified" => self.verified += 1,
            "mismatch" => self.mismatch += 1,
            "missing" => self.missing += 1,
            "not_on_putio" => self.not_on_putio += 1,
            "busy" => self.busy += 1,
            "unreadable" => self.unreadable += 1,
            "not_checked" => self.not_checked += 1,
            _ => {}
        }
    }
}

/// A recorded run
#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromRow)]
pub struct VerifyRun {
    pub id: i64,
    /// `everything` or `paths`
    pub scope: String,
    pub report_only: bool,
    /// `running`, `finished`, `stopped` or `failed`
    pub state: String,
    pub error: Option<String>,
    /// UTC, `YYYY-MM-DD HH:MM:SS`
    pub started_at: String,
    pub ended_at: Option<String>,
    pub files_total: i64,
    pub files_done: i64,
    pub bytes_total: i64,
    pub bytes_done: i64,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub counts: RunCounts,
}

/// A file that did not verify, as it is recorded
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProblem {
    pub run_id: Option<i64>,
    /// `download` or `run`
    pub source: String,
    pub file_id: i64,
    pub name: String,
    pub local_path: Option<String>,
    /// `mismatch`, `missing`, `not_on_putio`, `busy`, `unreadable`, `not_checked` or
    /// `failed`
    pub result: String,
    /// Token-free
    pub detail: Option<String>,
    pub queued: bool,
}

/// A recorded problem
#[derive(Debug, Clone, PartialEq, Eq, Serialize, FromRow)]
pub struct VerifyProblem {
    pub id: i64,
    pub run_id: Option<i64>,
    pub source: String,
    pub file_id: i64,
    pub name: String,
    pub local_path: Option<String>,
    pub result: String,
    pub detail: Option<String>,
    pub queued: bool,
    /// UTC, `YYYY-MM-DD HH:MM:SS`
    pub at: String,
    pub fixed_at: Option<String>,
}

/// Repository for `verify_runs` and `verify_problems`
pub struct VerifyRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> VerifyRepository<'a> {
    /// Create a new verify repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Record a new `running` run; its id. Older runs past [`RUNS_KEPT`] are pruned.
    pub async fn start_run(
        &self,
        scope: &str,
        report_only: bool,
        files_total: i64,
        bytes_total: i64,
    ) -> DbResult<i64> {
        let id = sqlx::query(
            "INSERT INTO verify_runs (scope, report_only, state, files_total, bytes_total) \
             VALUES (?, ?, 'running', ?, ?)",
        )
        .bind(scope)
        .bind(report_only)
        .bind(files_total)
        .bind(bytes_total)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to start a verify run"))?
        .last_insert_rowid();
        self.prune(RUNS_KEPT).await?;
        Ok(id)
    }

    /// Store a run's progress
    pub async fn update_run(
        &self,
        id: i64,
        files_done: i64,
        bytes_done: i64,
        counts: &RunCounts,
    ) -> DbResult<()> {
        sqlx::query(
            "UPDATE verify_runs SET files_done = ?, bytes_done = ?, verified = ?, mismatch = ?, \
             missing = ?, not_on_putio = ?, busy = ?, unreadable = ?, not_checked = ? WHERE id = ?",
        )
        .bind(files_done)
        .bind(bytes_done)
        .bind(counts.verified)
        .bind(counts.mismatch)
        .bind(counts.missing)
        .bind(counts.not_on_putio)
        .bind(counts.busy)
        .bind(counts.unreadable)
        .bind(counts.not_checked)
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to update a verify run"))?;
        Ok(())
    }

    /// End a run as `finished`, `stopped` or `failed`
    pub async fn finish_run(&self, id: i64, state: &str, error: Option<&str>) -> DbResult<()> {
        sqlx::query(
            "UPDATE verify_runs SET state = ?, error = ?, ended_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(state)
        .bind(error)
        .bind(id)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to end a verify run"))?;
        Ok(())
    }

    /// Mark runs left `running` by a daemon that stopped as `stopped`; how many
    pub async fn stop_stale(&self) -> DbResult<u64> {
        let done = sqlx::query(
            "UPDATE verify_runs SET state = 'stopped', ended_at = CURRENT_TIMESTAMP \
             WHERE state = 'running'",
        )
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to stop old verify runs"))?;
        Ok(done.rows_affected())
    }

    /// Keep the latest `keep` runs and the problems of those runs and of downloads
    pub async fn prune(&self, keep: i64) -> DbResult<()> {
        const CONTEXT: &str = "Failed to prune verify runs";
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(tracked(DbOp::Other, CONTEXT))?;
        let old = "SELECT id FROM verify_runs ORDER BY id DESC LIMIT -1 OFFSET ?";
        sqlx::query(&format!(
            "DELETE FROM verify_problems WHERE run_id IN ({old})"
        ))
        .bind(keep)
        .execute(&mut *tx)
        .await
        .map_err(tracked(DbOp::Other, CONTEXT))?;
        sqlx::query(&format!("DELETE FROM verify_runs WHERE id IN ({old})"))
            .bind(keep)
            .execute(&mut *tx)
            .await
            .map_err(tracked(DbOp::Other, CONTEXT))?;
        tx.commit().await.map_err(tracked(DbOp::Other, CONTEXT))?;
        Ok(())
    }

    /// The latest run, going or not
    pub async fn latest_run(&self) -> DbResult<Option<VerifyRun>> {
        sqlx::query_as::<_, VerifyRun>("SELECT * FROM verify_runs ORDER BY id DESC LIMIT 1")
            .fetch_optional(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to read the last verify run"))
    }

    /// A run by id
    pub async fn run(&self, id: i64) -> DbResult<Option<VerifyRun>> {
        sqlx::query_as::<_, VerifyRun>("SELECT * FROM verify_runs WHERE id = ?")
            .bind(id)
            .fetch_optional(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to read a verify run"))
    }

    /// Record a file that did not verify; its id
    pub async fn add_problem(&self, problem: &NewProblem) -> DbResult<i64> {
        let id = sqlx::query(
            "INSERT INTO verify_problems (run_id, source, file_id, name, local_path, result, detail, queued) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(problem.run_id)
        .bind(&problem.source)
        .bind(problem.file_id)
        .bind(&problem.name)
        .bind(&problem.local_path)
        .bind(&problem.result)
        .bind(&problem.detail)
        .bind(problem.queued)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to record a verify problem"))?
        .last_insert_rowid();
        Ok(id)
    }

    /// The file passed a check: its open problems are fixed; how many
    pub async fn mark_fixed(&self, file_id: i64) -> DbResult<u64> {
        let done = sqlx::query(
            "UPDATE verify_problems SET fixed_at = CURRENT_TIMESTAMP \
             WHERE file_id = ? AND fixed_at IS NULL",
        )
        .bind(file_id)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to mark verify problems fixed"))?;
        Ok(done.rows_affected())
    }

    /// Problems not yet fixed, newest first, at most `limit`
    pub async fn open_problems(&self, limit: i64) -> DbResult<Vec<VerifyProblem>> {
        sqlx::query_as::<_, VerifyProblem>(
            "SELECT * FROM verify_problems WHERE fixed_at IS NULL ORDER BY id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to list verify problems"))
    }

    /// Open problems of downloads that gave up in the last `days` days
    pub async fn open_download_failures(&self, days: i64) -> DbResult<i64> {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM verify_problems WHERE source = 'download' AND fixed_at IS NULL \
             AND at >= datetime('now', ?)",
        )
        .bind(format!("-{days} days"))
        .fetch_one(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to count verify problems"))?;
        Ok(n)
    }

    /// Open `mismatch` problems of run `run_id`
    pub async fn open_mismatches(&self, run_id: i64) -> DbResult<i64> {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM verify_problems WHERE run_id = ? AND result = 'mismatch' \
             AND fixed_at IS NULL",
        )
        .bind(run_id)
        .fetch_one(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to count verify mismatches"))?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    async fn db() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::new(dir.path().join("t.db")).await.unwrap();
        (dir, db)
    }

    fn problem(run_id: Option<i64>, file_id: i64, result: &str) -> NewProblem {
        NewProblem {
            run_id,
            source: if run_id.is_some() { "run" } else { "download" }.to_string(),
            file_id,
            name: format!("f{file_id}.mkv"),
            local_path: Some(format!("/data/f{file_id}.mkv")),
            result: result.to_string(),
            detail: Some("expected 8f2c1a07, got 00a1b2c3".to_string()),
            queued: result == "mismatch",
        }
    }

    #[tokio::test]
    async fn a_run_records_progress_and_ends() {
        let (_dir, db) = db().await;
        let repo = db.verify();
        assert!(repo.latest_run().await.unwrap().is_none());
        let id = repo.start_run("everything", false, 3, 300).await.unwrap();
        let mut counts = RunCounts::default();
        counts.add("verified");
        counts.add("mismatch");
        counts.add("nonsense");
        repo.update_run(id, 2, 200, &counts).await.unwrap();
        let run = repo.latest_run().await.unwrap().unwrap();
        assert_eq!(run.state, "running");
        assert_eq!((run.files_done, run.bytes_done), (2, 200));
        assert_eq!((run.counts.verified, run.counts.mismatch), (1, 1));
        assert!(run.ended_at.is_none());
        repo.finish_run(id, "finished", None).await.unwrap();
        let run = repo.run(id).await.unwrap().unwrap();
        assert_eq!(run.state, "finished");
        assert!(run.ended_at.is_some());
    }

    #[tokio::test]
    async fn runs_left_running_are_stopped() {
        let (_dir, db) = db().await;
        let repo = db.verify();
        let id = repo.start_run("paths", true, 1, 1).await.unwrap();
        assert_eq!(repo.stop_stale().await.unwrap(), 1);
        let run = repo.run(id).await.unwrap().unwrap();
        assert_eq!(run.state, "stopped");
        assert!(run.report_only);
        assert_eq!(repo.stop_stale().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn only_the_latest_runs_are_kept_with_their_problems() {
        let (_dir, db) = db().await;
        let repo = db.verify();
        let first = repo.start_run("everything", false, 1, 1).await.unwrap();
        repo.add_problem(&problem(Some(first), 1, "mismatch"))
            .await
            .unwrap();
        repo.add_problem(&problem(None, 2, "failed")).await.unwrap();
        for _ in 0..RUNS_KEPT {
            repo.start_run("everything", false, 1, 1).await.unwrap();
        }
        assert!(repo.run(first).await.unwrap().is_none());
        let open = repo.open_problems(200).await.unwrap();
        assert_eq!(open.len(), 1, "the download's problem stays");
        assert_eq!(open[0].source, "download");
    }

    #[tokio::test]
    async fn a_pass_fixes_the_files_problems() {
        let (_dir, db) = db().await;
        let repo = db.verify();
        let run = repo.start_run("everything", false, 2, 2).await.unwrap();
        repo.add_problem(&problem(Some(run), 1, "mismatch"))
            .await
            .unwrap();
        repo.add_problem(&problem(Some(run), 2, "missing"))
            .await
            .unwrap();
        repo.add_problem(&problem(None, 3, "failed")).await.unwrap();
        assert_eq!(repo.open_mismatches(run).await.unwrap(), 1);
        assert_eq!(repo.open_download_failures(7).await.unwrap(), 1);

        assert_eq!(repo.mark_fixed(1).await.unwrap(), 1);
        assert_eq!(repo.mark_fixed(3).await.unwrap(), 1);
        assert_eq!(repo.open_mismatches(run).await.unwrap(), 0);
        assert_eq!(repo.open_download_failures(7).await.unwrap(), 0);
        let open = repo.open_problems(200).await.unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].result, "missing");
        assert!(!open[0].queued);
    }

    #[tokio::test]
    async fn downloads_history_and_queue_carry_the_verify_columns() {
        use crate::models::{DownloadStatus, HistoryAction, QueueStatus};
        let (_dir, db) = db().await;
        let id = db
            .downloads()
            .insert(7, "a.mkv", 10, "/data/a.mkv", DownloadStatus::Completed)
            .await
            .unwrap();
        db.downloads()
            .set_verified(id, Some("8f2c1a07"), "crc32")
            .await
            .unwrap();
        let row = db.downloads().get(id).await.unwrap().unwrap();
        assert_eq!(row.crc32.as_deref(), Some("8f2c1a07"));
        assert_eq!(row.verify.as_deref(), Some("crc32"));
        assert!(row.verified_at.is_some());
        let rows = db.downloads().completed_rows().await.unwrap();
        assert_eq!(rows.len(), 1);

        let h = db
            .history()
            .insert(7, "a.mkv", 10, HistoryAction::Downloaded, Some(1), Some(10))
            .await
            .unwrap();
        assert_eq!(db.history().get(h).await.unwrap().unwrap().verify, None);
        db.history().set_verify(h, "size", 1).await.unwrap();
        let row = db.history().get(h).await.unwrap().unwrap();
        assert_eq!(row.verify.as_deref(), Some("size"));
        assert_eq!(row.redownloads, Some(1));

        let queue = db.queue();
        queue
            .add(7, "a.mkv", 10, "", "Movies", 5, QueueStatus::Pending)
            .await
            .unwrap();
        assert!(!queue.get_by_file_id(7).await.unwrap().unwrap().redownload);
        queue.update_status(7, QueueStatus::Failed).await.unwrap();
        queue
            .add_redownload(7, "a.mkv", 10, "", "Movies", Some("a (1).mkv"))
            .await
            .unwrap();
        let item = queue.get_by_file_id(7).await.unwrap().unwrap();
        assert!(item.redownload);
        assert_eq!(item.status, "pending");
        assert_eq!(item.priority, 5, "keeps its place");
        assert_eq!(item.save_as.as_deref(), Some("a (1).mkv"));
        queue
            .add_redownload(8, "b.mkv", 20, "", "", None)
            .await
            .unwrap();
        assert!(queue.get_by_file_id(8).await.unwrap().unwrap().redownload);
    }
}
