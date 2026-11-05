//! "Verify everything" (feature 034, research R5)
//!
//! A run reads downloaded files back at a limited rate and compares each with Put.io's
//! CRC32 (or size). A mismatch is queued to be downloaded again over the bad copy unless
//! the run is "report only". One run goes at a time, owned by the [`DownloadManager`];
//! it can be paused, resumed and stopped, and waits while 032 says "paused".

use crate::files::{crc_equal, valid_crc32, Crc32, HASH_BLOCK};
use crate::manager::{sanitize_filename, DownloadManager};
use putio_sync_core::events::{self, Payload};
use putio_sync_core::redact;
use putio_sync_db::{NewProblem, RunCounts};
use putio_sync_telemetry as telemetry;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::sync::watch;
use tokio::time::Instant;
use tracing::{info, warn};

/// How often the run row is written while a run goes
const ROW_EVERY: Duration = Duration::from_secs(1);
/// How long the walk of Put.io may take
const WALK_DEADLINE: Duration = Duration::from_secs(600);
/// A paths run over at most this many files asks Put.io per file instead of walking
const DETAILS_UP_TO: usize = 50;
/// Runs kept in `verify_runs`
const KEEP_RUNS: i64 = 10;
/// How often a pause or a 032 pause is looked at again
const RECHECK: Duration = Duration::from_millis(250);

/// What a run covers
#[derive(Debug, Clone, Default)]
pub struct VerifyRequest {
    /// Local paths to check; `None` checks every downloaded file
    pub paths: Option<Vec<String>>,
    /// Only report mismatches, do not queue them
    pub report_only: bool,
}

/// Why a run could not start
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerifyStartError {
    #[error("a verify run is already going")]
    Running,
    #[error("none of the paths is a file putio-sync downloaded")]
    NoFiles,
    #[error("the database is not available")]
    NoDatabase,
    #[error("{0}")]
    Database(String),
}

/// Why pause, resume or stop was refused
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerifyControlError {
    #[error("no verify run is going")]
    NotRunning,
    #[error("the verify run is not paused")]
    NotPaused,
}

/// The desired state, set by the controls
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Run,
    Pause,
    Stop,
}

/// A run's live progress for `GET /verify` (data-model.md `VerifyStatus`)
#[derive(Debug, Clone, Serialize)]
pub struct VerifyStatus {
    /// `idle`, `running`, `paused`, `waiting` or `stopping`
    pub state: &'static str,
    pub run_id: Option<i64>,
    pub scope: Option<&'static str>,
    pub report_only: bool,
    pub files_total: u64,
    pub files_done: u64,
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub counts: RunCounts,
    /// Name of the file being read
    pub current: Option<String>,
    /// Why a run waits, such as "bandwidth paused"
    pub waiting_reason: Option<String>,
    pub read_limit_bytes_per_sec: u64,
}

impl Default for VerifyStatus {
    fn default() -> Self {
        Self {
            state: "idle",
            run_id: None,
            scope: None,
            report_only: false,
            files_total: 0,
            files_done: 0,
            bytes_total: 0,
            bytes_done: 0,
            counts: RunCounts::default(),
            current: None,
            waiting_reason: None,
            read_limit_bytes_per_sec: 0,
        }
    }
}

/// The run state the manager owns; clones share it
#[derive(Clone, Default)]
pub struct VerifyRunner {
    status: Arc<Mutex<VerifyStatus>>,
    control: Arc<Mutex<Option<watch::Sender<Control>>>>,
}

impl VerifyRunner {
    fn status_mut(&self) -> std::sync::MutexGuard<'_, VerifyStatus> {
        self.status.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set(&self, control: Control) -> bool {
        let guard = self.control.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(tx) => {
                let _ = tx.send(control);
                true
            }
            None => false,
        }
    }
}

/// `DownloadManager::verify()`: start and steer verify runs
pub struct Verifier<'a> {
    manager: &'a DownloadManager,
}

impl DownloadManager {
    /// The verify run of this manager (feature 034)
    pub fn verify(&self) -> Verifier<'_> {
        Verifier { manager: self }
    }
}

/// Put.io's view of one file
#[derive(Debug, Clone)]
struct Remote {
    name: String,
    size: u64,
    crc32: Option<String>,
}

/// Where Put.io's sizes and CRC32s come from for a run
enum Lookup {
    /// One walk of the account; `complete` false when it was cut short
    Walk {
        files: HashMap<i64, Remote>,
        complete: bool,
    },
    /// One `get_file_details` call per file
    Details,
}

/// One file's result
enum Outcome {
    Verified(&'static str),
    Mismatch {
        method: &'static str,
        detail: String,
        expected: Option<String>,
        actual: Option<String>,
        remote: Remote,
    },
    Problem(&'static str, String),
}

impl Verifier<'_> {
    fn runner(&self) -> &VerifyRunner {
        &self.manager.verifier
    }

    /// The live status
    pub fn status(&self) -> VerifyStatus {
        self.runner().status_mut().clone()
    }

    /// Mark runs a previous process left `running` as `stopped`; call at startup
    pub async fn recover(&self) {
        if let Some(db) = &self.manager.database {
            match db.verify().stop_stale().await {
                Ok(0) => {}
                Ok(n) => info!(runs = n, "Marked interrupted verify runs stopped"),
                Err(e) => warn!("Failed to mark interrupted verify runs stopped: {}", e),
            }
        }
    }

    /// Start a run in the background
    pub async fn start(&self, request: VerifyRequest) -> Result<VerifyStatus, VerifyStartError> {
        let db = self
            .manager
            .database
            .clone()
            .ok_or(VerifyStartError::NoDatabase)?;
        let runner = self.runner().clone();
        let (tx, rx) = watch::channel(Control::Run);
        {
            let mut control = runner.control.lock().unwrap_or_else(|e| e.into_inner());
            if control.is_some() {
                return Err(VerifyStartError::Running);
            }
            *control = Some(tx);
        }
        let release = |runner: &VerifyRunner| {
            *runner.control.lock().unwrap_or_else(|e| e.into_inner()) = None;
        };

        let rows = match db.downloads().completed_rows().await {
            Ok(rows) => rows,
            Err(e) => {
                release(&runner);
                return Err(VerifyStartError::Database(e.to_string()));
            }
        };
        // The newest row of each file id
        let mut by_id: HashMap<i64, putio_sync_db::models::Download> = HashMap::new();
        for row in rows {
            by_id.insert(row.file_id, row);
        }
        let mut files: Vec<_> = by_id.into_values().collect();
        if let Some(paths) = &request.paths {
            files.retain(|row| paths.iter().any(|p| p == &row.local_path));
            if files.is_empty() {
                release(&runner);
                return Err(VerifyStartError::NoFiles);
            }
        }
        files.sort_by_key(|row| row.id);
        let scope = if request.paths.is_some() {
            "paths"
        } else {
            "everything"
        };
        let bytes_total: u64 = files.iter().map(|row| row.file_size.max(0) as u64).sum();
        let _ = db.verify().stop_stale().await;
        let run_id = match db
            .verify()
            .start_run(
                scope,
                request.report_only,
                files.len() as i64,
                bytes_total as i64,
            )
            .await
        {
            Ok(id) => id,
            Err(e) => {
                release(&runner);
                return Err(VerifyStartError::Database(e.to_string()));
            }
        };
        let read_limit = self
            .manager
            .config
            .read()
            .await
            .verify
            .read_limit_bytes_per_sec;
        {
            let mut status = runner.status_mut();
            *status = VerifyStatus {
                state: "running",
                run_id: Some(run_id),
                scope: Some(scope),
                report_only: request.report_only,
                files_total: files.len() as u64,
                bytes_total,
                read_limit_bytes_per_sec: read_limit,
                ..VerifyStatus::default()
            };
        }
        info!(
            run_id,
            scope,
            files = files.len(),
            report_only = request.report_only,
            "Verify run started"
        );
        telemetry::verify::set_run_active(true);

        let manager = self.manager.clone();
        let status = self.status();
        tokio::spawn(async move {
            let run = Run {
                manager: &manager,
                runner: &runner,
                db: &db,
                run_id,
                report_only: request.report_only,
                control: rx,
                counts: RunCounts::default(),
                files_done: 0,
                bytes_done: 0,
                last_row: Instant::now(),
            };
            let (state, error) = run.go(files, scope == "paths").await;
            if let Err(e) = db
                .verify()
                .finish_run(run_id, state, error.as_deref())
                .await
            {
                warn!(run_id, "Failed to finish the verify run row: {}", e);
            }
            let _ = db.verify().prune(KEEP_RUNS).await;
            info!(run_id, state, "Verify run ended");
            telemetry::verify::set_run_active(false);
            *runner.status_mut() = VerifyStatus::default();
            release(&runner);
        });
        Ok(status)
    }

    /// Hold the run after the block being read
    pub fn pause(&self) -> Result<VerifyStatus, VerifyControlError> {
        if !self.runner().set(Control::Pause) {
            return Err(VerifyControlError::NotRunning);
        }
        self.runner().status_mut().state = "paused";
        Ok(self.status())
    }

    /// Go on after a pause
    pub fn resume(&self) -> Result<VerifyStatus, VerifyControlError> {
        if self.status().state != "paused" {
            let going = self
                .runner()
                .control
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_some();
            return Err(if going {
                VerifyControlError::NotPaused
            } else {
                VerifyControlError::NotRunning
            });
        }
        self.runner().set(Control::Run);
        self.runner().status_mut().state = "running";
        Ok(self.status())
    }

    /// End the run; its row is left `stopped`
    pub fn stop(&self) -> Result<VerifyStatus, VerifyControlError> {
        if !self.runner().set(Control::Stop) {
            return Err(VerifyControlError::NotRunning);
        }
        self.runner().status_mut().state = "stopping";
        Ok(self.status())
    }
}

/// One run in progress
struct Run<'a> {
    manager: &'a DownloadManager,
    runner: &'a VerifyRunner,
    db: &'a putio_sync_db::Database,
    run_id: i64,
    report_only: bool,
    control: watch::Receiver<Control>,
    counts: RunCounts,
    files_done: u64,
    bytes_done: u64,
    last_row: Instant,
}

/// The run was stopped
struct Stopped;

impl Run<'_> {
    async fn go(
        mut self,
        files: Vec<putio_sync_db::models::Download>,
        few: bool,
    ) -> (&'static str, Option<String>) {
        let lookup = if few && files.len() <= DETAILS_UP_TO {
            Lookup::Details
        } else {
            let walk = self
                .manager
                .client
                .walk_files(4, Instant::now() + WALK_DEADLINE)
                .await;
            if walk.files.is_empty() && !walk.errors.is_empty() {
                let error = redact::text(&walk.errors[0].error);
                warn!(
                    run_id = self.run_id,
                    "Verify run could not list Put.io: {}", error
                );
                return ("failed", Some(format!("Could not list Put.io: {error}")));
            }
            let files = walk
                .files
                .into_iter()
                .filter(|f| !f.is_folder)
                .map(|f| {
                    (
                        f.id,
                        Remote {
                            name: f.name,
                            size: f.size.max(0) as u64,
                            crc32: valid_crc32(f.crc32.as_deref()),
                        },
                    )
                })
                .collect();
            Lookup::Walk {
                files,
                complete: walk.complete && walk.errors.is_empty(),
            }
        };

        for row in files {
            if self.hold().await.is_err() {
                return ("stopped", None);
            }
            self.runner.status_mut().current = Some(row.filename.clone());
            // A file counts in full once done, read or not
            let done = self.bytes_done + row.file_size.max(0) as u64;
            let outcome = match self.check(&row, &lookup).await {
                Ok(outcome) => outcome,
                Err(Stopped) => return ("stopped", None),
            };
            self.record(&row, outcome).await;
            self.files_done += 1;
            self.bytes_done = done;
            self.publish(false).await;
        }
        self.publish(true).await;
        ("finished", None)
    }

    /// Wait while paused or 032 says paused; `Err` on stop
    async fn hold(&mut self) -> Result<(), Stopped> {
        loop {
            let control = *self.control.borrow();
            match control {
                Control::Stop => return Err(Stopped),
                Control::Pause => {
                    let _ = tokio::time::timeout(RECHECK, self.control.changed()).await;
                    continue;
                }
                Control::Run => {}
            }
            let follow = self
                .manager
                .config
                .read()
                .await
                .verify
                .follow_bandwidth_pause;
            if follow && self.manager.bandwidth.is_paused() {
                {
                    let mut status = self.runner.status_mut();
                    status.state = "waiting";
                    status.waiting_reason = Some("bandwidth paused".to_string());
                }
                let _ = tokio::time::timeout(RECHECK, self.control.changed()).await;
                continue;
            }
            let mut status = self.runner.status_mut();
            if status.state == "waiting" {
                status.state = "running";
                status.waiting_reason = None;
            }
            return Ok(());
        }
    }

    async fn check(
        &mut self,
        row: &putio_sync_db::models::Download,
        lookup: &Lookup,
    ) -> Result<Outcome, Stopped> {
        let path = PathBuf::from(&row.local_path);
        let meta = match tokio::fs::metadata(&path).await {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => return Ok(Outcome::Problem("missing", "not a file".to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Outcome::Problem(
                    "missing",
                    "no file at this path".to_string(),
                ))
            }
            Err(e) => return Ok(Outcome::Problem("unreadable", e.to_string())),
        };
        if self.busy(row, &path).await {
            return Ok(Outcome::Problem("busy", "being downloaded".to_string()));
        }
        let remote = match lookup {
            Lookup::Walk { files, complete } => match files.get(&row.file_id) {
                Some(remote) => remote.clone(),
                None if *complete => {
                    return Ok(Outcome::Problem(
                        "not_on_putio",
                        "not on Put.io".to_string(),
                    ))
                }
                None => {
                    return Ok(Outcome::Problem(
                        "not_checked",
                        "the Put.io listing was incomplete".to_string(),
                    ))
                }
            },
            Lookup::Details => match self.details(row.file_id).await {
                Ok(remote) => remote,
                Err(outcome) => return Ok(outcome),
            },
        };
        let remote = if remote.crc32.is_none() && matches!(lookup, Lookup::Walk { .. }) {
            // The listing had no CRC32: ask once for this file (R5)
            match self.details(row.file_id).await {
                Ok(details) => details,
                Err(_) => remote,
            }
        } else {
            remote
        };
        if meta.len() != remote.size {
            return Ok(Outcome::Mismatch {
                method: "size",
                detail: format!("size {} bytes, Put.io {} bytes", meta.len(), remote.size),
                expected: remote.crc32.clone(),
                actual: None,
                remote,
            });
        }
        let Some(expected) = remote.crc32.clone() else {
            return Ok(Outcome::Verified("size"));
        };
        let actual = match self.hash(&path, meta.len()).await? {
            Ok(actual) => actual,
            Err(e) => return Ok(Outcome::Problem("unreadable", e.to_string())),
        };
        if crc_equal(&actual, &expected) {
            Ok(Outcome::Verified("crc32"))
        } else {
            Ok(Outcome::Mismatch {
                method: "crc32",
                detail: format!("expected {expected}, got {actual}"),
                expected: Some(expected),
                actual: Some(actual),
                remote,
            })
        }
    }

    async fn details(&self, file_id: i64) -> Result<Remote, Outcome> {
        match self.manager.client.get_file_details(file_id).await {
            Ok(details) => Ok(Remote {
                name: details.file.name,
                size: details.file.size.max(0) as u64,
                crc32: valid_crc32(details.file.crc32.as_deref()),
            }),
            Err(e) if e.is_not_found() => Err(Outcome::Problem(
                "not_on_putio",
                "not on Put.io".to_string(),
            )),
            Err(e) => Err(Outcome::Problem(
                "not_checked",
                redact::text(&e.to_string()),
            )),
        }
    }

    /// A worker writes the path, or the file's queue row is downloading
    async fn busy(&self, row: &putio_sync_db::models::Download, path: &Path) -> bool {
        if self.manager.path_in_use(path) {
            return true;
        }
        matches!(
            self.db.queue().get_by_file_id(row.file_id).await,
            Ok(Some(item)) if item.status == "downloading"
        )
    }

    /// Hash `path` at the read limit; the outer `Err` is a stop
    async fn hash(&mut self, path: &Path, len: u64) -> Result<std::io::Result<String>, Stopped> {
        let mut file = match tokio::fs::File::open(path).await {
            Ok(file) => file,
            Err(e) => return Ok(Err(e)),
        };
        let mut digest = Crc32::new();
        let mut buf = vec![0u8; HASH_BLOCK];
        let mut read = 0u64;
        let base = self.bytes_done;
        let mut pacer = Pacer::new();
        while read < len {
            // Pause takes effect within one block (SC-003); time held is not paced
            if *self.control.borrow() != Control::Run || self.manager.bandwidth.is_paused() {
                self.hold().await?;
                pacer = Pacer::new();
            }
            let limit = self
                .manager
                .config
                .read()
                .await
                .verify
                .read_limit_bytes_per_sec;
            self.runner.status_mut().read_limit_bytes_per_sec = limit;
            // Blocks of a quarter second at a low limit, so the rate stays even
            let block = match limit {
                0 => HASH_BLOCK,
                limit => (limit / 4).clamp(4096, HASH_BLOCK as u64) as usize,
            };
            let n = match file.read(&mut buf[..block]).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => return Ok(Err(e)),
            };
            digest.update(&buf[..n]);
            read += n as u64;
            pacer.add(n as u64);
            telemetry::verify::record_read(n as u64);
            self.bytes_done = base + read;
            self.publish(false).await;
            pacer.wait(limit).await;
        }
        Ok(Ok(digest.hex()))
    }

    async fn record(&mut self, row: &putio_sync_db::models::Download, outcome: Outcome) {
        let name = Path::new(&row.local_path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| row.filename.clone());
        let relative_path = self.relative(&row.local_path);
        match outcome {
            Outcome::Verified(method) => {
                self.counts.add("verified");
                telemetry::verify::record_run_file("verified");
                telemetry::verify::record_check("run", method, true);
                let _ = self
                    .db
                    .downloads()
                    .touch_verified(row.file_id, method)
                    .await;
                let _ = self.db.verify().mark_fixed(row.file_id).await;
            }
            Outcome::Problem(result, detail) => {
                self.counts.add(result);
                telemetry::verify::record_run_file(result);
                self.problem(row, &name, result, &detail, false).await;
            }
            Outcome::Mismatch {
                method,
                detail,
                expected,
                actual,
                remote,
            } => {
                self.counts.add("mismatch");
                telemetry::verify::record_run_file("mismatch");
                telemetry::verify::record_check("run", method, false);
                warn!(
                    file_id = row.file_id,
                    name = %name,
                    method,
                    expected = expected.as_deref().unwrap_or(""),
                    actual = actual.as_deref().unwrap_or(""),
                    "Verify run found a mismatch"
                );
                let queued = !self.report_only && self.requeue(row, &remote).await;
                self.problem(row, &name, "mismatch", &detail, queued).await;
                events::emit(Payload::VerifyFailed(events::VerifyFailed {
                    file_id: row.file_id,
                    name: name.clone(),
                    relative_path: relative_path.clone(),
                    local_path: row.local_path.clone(),
                    size_bytes: row.file_size.max(0) as u64,
                    expected_crc32: expected,
                    actual_crc32: actual,
                    attempt: 1,
                    will_redownload: queued,
                    trigger: "run".to_string(),
                }));
                if queued {
                    let max = self.manager.config.read().await.verify.max_redownloads;
                    telemetry::verify::record_redownload("run");
                    events::emit(Payload::VerifyRedownload(events::VerifyRedownload {
                        file_id: row.file_id,
                        name,
                        relative_path,
                        size_bytes: row.file_size.max(0) as u64,
                        attempt: 1,
                        max_redownloads: max,
                        trigger: "run".to_string(),
                    }));
                }
            }
        }
    }

    async fn problem(
        &self,
        row: &putio_sync_db::models::Download,
        name: &str,
        result: &str,
        detail: &str,
        queued: bool,
    ) {
        let problem = NewProblem {
            run_id: Some(self.run_id),
            source: "run".to_string(),
            file_id: row.file_id,
            name: name.to_string(),
            local_path: Some(row.local_path.clone()),
            result: result.to_string(),
            detail: Some(redact::text(detail)),
            queued,
        };
        if let Err(e) = self.db.verify().add_problem(&problem).await {
            warn!("Failed to record the verify problem for {}: {}", name, e);
        }
    }

    /// Queue the file to be downloaded again over the bad copy (T013): the folder from
    /// its local path, its saved name when that differs from Put.io's
    async fn requeue(&self, row: &putio_sync_db::models::Download, remote: &Remote) -> bool {
        let target = self.manager.config.read().await.target_directory.clone();
        let local = Path::new(&row.local_path);
        let folder = local
            .parent()
            .and_then(|p| p.strip_prefix(&target).ok())
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let saved = local
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let save_as = (saved != sanitize_filename(&remote.name)).then_some(saved.as_str());
        match self
            .db
            .queue()
            .add_redownload(
                row.file_id,
                &remote.name,
                remote.size as i64,
                &self.manager.client.get_download_url(row.file_id),
                &folder,
                save_as,
            )
            .await
        {
            Ok(_) => true,
            Err(e) => {
                warn!("Failed to queue {} to download again: {}", remote.name, e);
                false
            }
        }
    }

    fn relative(&self, local_path: &str) -> String {
        let target = self
            .manager
            .config
            .try_read()
            .map(|c| c.target_directory.clone())
            .unwrap_or_default();
        Path::new(local_path)
            .strip_prefix(&target)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| local_path.to_string())
    }

    /// Update the live status and, at most once a second (or `force`), the run row
    async fn publish(&mut self, force: bool) {
        {
            let mut status = self.runner.status_mut();
            status.files_done = self.files_done;
            status.bytes_done = self.bytes_done;
            status.counts = self.counts;
        }
        if force || self.last_row.elapsed() >= ROW_EVERY {
            self.last_row = Instant::now();
            if let Err(e) = self
                .db
                .verify()
                .update_run(
                    self.run_id,
                    self.files_done as i64,
                    self.bytes_done as i64,
                    &self.counts,
                )
                .await
            {
                warn!(
                    run_id = self.run_id,
                    "Failed to update the verify run row: {}", e
                );
            }
        }
    }
}

/// Keeps a run's reads at or under the limit on average since it last started
struct Pacer {
    started: Instant,
    bytes: u64,
}

impl Pacer {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            bytes: 0,
        }
    }

    fn add(&mut self, bytes: u64) {
        self.bytes += bytes;
    }

    /// Sleep until the bytes read so far fit `limit` bytes a second
    async fn wait(&self, limit: u64) {
        if limit == 0 {
            return;
        }
        let due = Duration::from_secs_f64(self.bytes as f64 / limit as f64);
        let elapsed = self.started.elapsed();
        if due > elapsed {
            tokio::time::sleep(due - elapsed).await;
        }
    }
}
