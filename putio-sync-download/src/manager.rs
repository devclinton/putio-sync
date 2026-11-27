//! Download manager for orchestrating downloads

use crate::arrived::{self, ArrivedFile, ArrivedStep};
use crate::error::{DownloadError, DownloadResult};
use crate::files::{
    check_resume, cleanup_partial, crc32_file, crc32_prefix, crc_equal, create_temp_path,
    needs_checksum, next_free_name, plan_existing, valid_crc32, Crc32, ExistingAction, LocalState,
};
use crate::pipeline::{file_key, ActionResult, ItemResult, Pipeline, PipelineItem, Stage, State};
use crate::progress::{DownloadProgress, OverallProgress, ProgressCallback, ProgressTracker};
use crate::rate_limiter::RateLimiter;
use crate::stream::{finish_download, stream_to_temp, StopSignal};
use crate::sync_status::{SyncOutcome, SyncRequestError, SyncStatus};
use crate::transfer_events::{self, TransferChange, TransferClass, TransferEvents};
use crate::utils::retry_with_stall_detection;
use futures_util::FutureExt;
use putio_sync_client::models::Transfer;
use putio_sync_client::{error_chain, FileEntry, PutioApiClient};
use putio_sync_core::bandwidth::{
    Override, OverrideMode, Policy, ScheduleEntry, Status as BandwidthStatus, DEFAULT_SLOW_LIMIT,
};
use putio_sync_core::config::{ExistingFile, ExistingOutcome};
use putio_sync_core::events::{self, Payload};
use putio_sync_core::redact;
use putio_sync_core::rules::{extension, Decision, Effective, Reason, RuleSet};
use putio_sync_core::stall::StallTracker;
use putio_sync_core::sync_hooks::{FilterAnswer, Hook, HookFile, SyncHooks};
use putio_sync_db::models::{DownloadSource, DownloadStatus, Origin, QueueStatus, SourceType};
use putio_sync_db::pipeline::{FileState, NAS_DOWNLOADS_PAUSED, TRANSFER_EVENTS_SEEN};
use putio_sync_db::Database;
use putio_sync_telemetry::{self as telemetry, PipelineAction, RuleDecision, SyncKind, SyncResult};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{broadcast, Notify, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// Configuration for download operations
///
/// Build it from `config.toml` with `DownloadConfig::from(&config.download)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadConfig {
    /// Target directory for downloads
    pub target_directory: PathBuf,
    /// What to do when a file already exists at its local path (feature 040)
    pub on_existing: ExistingFile,
    /// Continue partial downloads instead of starting over
    pub resume_enabled: bool,
    /// Buffer size in megabytes
    pub buffer_size_mb: usize,
    /// Bandwidth limit in bytes per second for all downloads together, outside every
    /// schedule entry (None = unlimited)
    pub bandwidth_limit_bytes_per_sec: Option<u64>,
    /// Limits by time of day and day of week (feature 032)
    pub bandwidth_schedule: Vec<ScheduleEntry>,
    /// The limit while "Slow" is on, or the schedule's if lower
    pub slow_limit_bytes_per_sec: u64,
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay for retries in seconds
    pub retry_initial_delay_secs: u64,
    /// New downloads wait while free space (less the file) is under this; 0 = off
    pub min_free_space_bytes: u64,
    /// Number of concurrent download workers
    pub workers: usize,
    /// Delete files from Put.io after successful download
    pub delete_after_download: bool,
    /// Ask Put.io for an MP4 version of streamed videos (feature 022)
    pub stream_mp4: bool,
    /// Folder rules with the global `blocked_extensions` as the root filter
    pub rules: RuleSet,
    /// Download verification (feature 034)
    pub verify: VerifySettings,
}

/// The `[download]` `verify_*` keys as the manager uses them (feature 034)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifySettings {
    /// Compare each download with Put.io's CRC32 (or size) before it is renamed
    pub enabled: bool,
    /// Downloads again after a mismatch before the file fails
    pub max_redownloads: u32,
    /// "Verify everything" read limit in bytes per second
    pub read_limit_bytes_per_sec: u64,
    /// "Verify everything" waits while the bandwidth limiter says paused
    pub follow_bandwidth_pause: bool,
}

impl Default for VerifySettings {
    fn default() -> Self {
        Self::from(&putio_sync_core::config::DownloadConfig::default())
    }
}

impl From<&putio_sync_core::config::DownloadConfig> for VerifySettings {
    fn from(settings: &putio_sync_core::config::DownloadConfig) -> Self {
        Self {
            enabled: settings.verify,
            max_redownloads: settings
                .verify_max_redownloads
                .min(putio_sync_core::config::DownloadConfig::MAX_VERIFY_REDOWNLOADS),
            read_limit_bytes_per_sec: settings.verify_read_limit_bytes(),
            follow_bandwidth_pause: settings.verify_follow_bandwidth_pause,
        }
    }
}

impl DownloadConfig {
    /// The default limit, the schedule and the slow limit
    pub fn bandwidth_policy(&self) -> Policy {
        Policy::new(
            self.bandwidth_limit_bytes_per_sec,
            &self.bandwidth_schedule,
            self.slow_limit_bytes_per_sec,
        )
    }

    /// The rules in force in `folder_path` (relative to the Put.io root). Settings no
    /// folder rule sets take this config's `delete_after_download` and `on_existing`.
    pub fn effective(&self, folder_path: &str) -> Effective {
        let mut effective = self.rules.resolve(folder_path);
        if effective.delete_after_download.from.is_none() {
            effective.delete_after_download.value = self.delete_after_download;
        }
        if effective.on_existing.from.is_none() {
            effective.on_existing.value = self.on_existing;
        }
        effective
    }
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            target_directory: PathBuf::from("."),
            on_existing: ExistingFile::Resume,
            resume_enabled: true,
            buffer_size_mb: 64,
            bandwidth_limit_bytes_per_sec: None,
            bandwidth_schedule: Vec::new(),
            slow_limit_bytes_per_sec: DEFAULT_SLOW_LIMIT,
            max_retries: 3,
            retry_initial_delay_secs: 2,
            min_free_space_bytes: putio_sync_core::config::DownloadConfig::DEFAULT_MIN_FREE_SPACE_MB
                * 1024 * 1024,
            workers: 1,
            delete_after_download: false,
            stream_mp4: true,
            rules: RuleSet::default(),
            verify: VerifySettings::default(),
        }
    }
}

impl From<&putio_sync_core::config::DownloadConfig> for DownloadConfig {
    /// The one mapping from `[download]` to the manager, used by `daemon`, `server` and
    /// the settings API. `chunk_size_mb` and `max_concurrent` have no manager counterpart
    /// and are ignored everywhere. `progress_style`, sorting and `delete_empty_folders` are
    /// read by the one-off `download` command only. The glob whitelist/blacklist patterns
    /// are read by the daemon too, through `RuleSet::from_config` (core `rules.rs`).
    fn from(settings: &putio_sync_core::config::DownloadConfig) -> Self {
        Self {
            target_directory: settings.target_directory.clone(),
            on_existing: settings.on_existing(),
            resume_enabled: settings.resume_enabled,
            buffer_size_mb: settings.buffer_size_mb,
            bandwidth_limit_bytes_per_sec: settings.bandwidth_limit_bytes_per_sec,
            bandwidth_schedule: settings.bandwidth_schedule.clone(),
            slow_limit_bytes_per_sec: settings.slow_limit_bytes_per_sec,
            max_retries: settings.max_retries,
            retry_initial_delay_secs: settings.retry_initial_delay_secs,
            min_free_space_bytes: settings.min_free_space_bytes(),
            workers: settings.workers,
            delete_after_download: settings.delete_after_download,
            stream_mp4: settings.stream_mp4,
            rules: RuleSet::from_config(settings),
            verify: VerifySettings::from(settings),
        }
    }
}

tokio::task_local! {
    /// The pool worker running the current job, for per-worker metrics at `detailed`.
    static WORKER_ID: usize;
}

/// The current pool worker id, or `None` outside a worker (one-off downloads).
pub(crate) fn current_worker() -> Option<usize> {
    WORKER_ID.try_with(|id| *id).ok()
}

/// Marks the current worker busy until dropped. No-op outside a worker or at `basic`.
struct BusyGuard(Option<usize>);

impl BusyGuard {
    fn new() -> Self {
        let id = current_worker();
        if let Some(id) = id {
            telemetry::download::worker_busy(id, true);
        }
        Self(id)
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        if let Some(id) = self.0 {
            telemetry::download::worker_busy(id, false);
        }
    }
}

/// Removes a file's stop signal when its download ends
struct StopGuard {
    signals: Arc<Mutex<HashMap<i64, Arc<StopSignal>>>>,
    file_id: i64,
    signal: Arc<StopSignal>,
}

impl Drop for StopGuard {
    fn drop(&mut self) {
        let mut signals = locked(&self.signals);
        // A later registration for the same file keeps its own signal
        if signals
            .get(&self.file_id)
            .is_some_and(|s| Arc::ptr_eq(s, &self.signal))
        {
            signals.remove(&self.file_id);
        }
    }
}

/// How long an idle worker waits for a wakeup before looking at the queue anyway
/// Free space the download folder had when new downloads were last held back, and the
/// floor it was under
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LowSpace {
    pub free_bytes: u64,
    pub min_free_bytes: u64,
    /// Free space is above the floor, but the next file doesn't fit above it
    pub file_too_big: bool,
}

/// Whether a download of `next_size` bytes must wait: the floor is on and free space,
/// less the file, would fall below it. `next_size` is 0 when no file is chosen yet.
pub fn below_free_space_floor(free: u64, min_free: u64, next_size: u64) -> bool {
    min_free > 0 && free.saturating_sub(next_size) < min_free
}

/// How often the "free space can't be read" warning may repeat
const SPACE_WARN_EVERY: Duration = Duration::from_secs(300);

/// Bytes still to be written by the downloads running now, shared by the workers. A
/// download reserves its file size when it starts and gives it back when its
/// [`SpaceGuard`] drops (finish, failure, cancel or panic).
#[derive(Debug, Clone, Default)]
pub struct SpaceReservations(Arc<Mutex<u64>>);

impl SpaceReservations {
    /// Bytes reserved by running downloads
    pub fn reserved(&self) -> u64 {
        *locked(&self.0)
    }

    /// Reserve `size` bytes unless free space, less what is reserved and `size`, would
    /// fall under `min_free` (the floor is off at 0). The check and the reservation
    /// happen under one lock, so workers can't both pass on the same room.
    pub fn try_reserve(&self, free: u64, min_free: u64, size: u64) -> Option<SpaceGuard> {
        let mut reserved = locked(&self.0);
        if below_free_space_floor(free.saturating_sub(*reserved), min_free, size) {
            return None;
        }
        *reserved = reserved.saturating_add(size);
        Some(SpaceGuard {
            reservations: self.0.clone(),
            size,
        })
    }
}

/// A reservation in [`SpaceReservations`]; released on drop
#[derive(Debug)]
pub struct SpaceGuard {
    reservations: Arc<Mutex<u64>>,
    size: u64,
}

impl Drop for SpaceGuard {
    fn drop(&mut self) {
        let mut reserved = locked(&self.reservations);
        *reserved = reserved.saturating_sub(self.size);
    }
}

/// Free bytes on the volume that holds (or will hold) `dir`. A directory that doesn't
/// exist yet is measured through its nearest existing parent, since the download creates
/// it. `None` when it can't be read; that is logged at most every [`SPACE_WARN_EVERY`].
fn free_bytes_of(dir: &Path, warned: &Mutex<Option<Instant>>) -> Option<u64> {
    let mut probe = dir;
    loop {
        match putio_sync_telemetry::gauges::disk_usage(probe) {
            Ok((free, _)) => return Some(free),
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    if let Some(parent) = probe.parent() {
                        probe = if parent.as_os_str().is_empty() {
                            Path::new(".")
                        } else {
                            parent
                        };
                        continue;
                    }
                }
                let mut last = locked(warned);
                if last.is_none_or(|t| t.elapsed() >= SPACE_WARN_EVERY) {
                    *last = Some(Instant::now());
                    warn!(
                        dir = %dir.display(),
                        error = %e,
                        "Can't read free space of the download folder; new downloads are \
                         on hold until it can be read (or set min_free_space_mb = 0)"
                    );
                }
                return None;
            }
        }
    }
}

const IDLE_POLL: Duration = Duration::from_secs(30);

/// How many times a sync queues a transiently failed file again before it stays failed
const MAX_SYNC_REQUEUES: u32 = 3;

/// Lock a std mutex, taking the data back if a panicking thread poisoned it. The maps
/// behind these locks stay valid after a panic, and one panicked worker must not make
/// every later call panic too.
fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Put.io folder listings in flight at once during a sync's tree walk; small, for
/// Put.io's rate limits
const LIST_CONCURRENCY: usize = 4;

/// [`locked`] for the read side of a std `RwLock`
fn read_locked<T>(l: &std::sync::RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(|e| e.into_inner())
}

/// [`locked`] for the write side of a std `RwLock`
fn write_locked<T>(l: &std::sync::RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(|e| e.into_inner())
}

/// Note shown when a stopped file had to start again from 0 (feature 039, R8)
const RESTARTED_NOTE: &str = "Started again: the partial download was gone";

/// Sanitize a filename to remove invalid characters: the one rule `putio_sync_core::rules`
/// uses for rule paths, so a path built here is the path a rule names
pub(crate) fn sanitize_filename(filename: &str) -> String {
    putio_sync_core::rules::sanitize_name(filename)
}

/// Join a relative folder path and a name; an empty folder path is the root
fn join_path(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", folder, name)
    }
}

/// File information for download
#[derive(Debug, Clone)]
pub struct DownloadFile {
    /// File ID from Put.io
    pub file_id: i64,
    /// Original filename
    pub filename: String,
    /// Relative path from root (for maintaining directory structure)
    pub relative_path: String,
    /// File size in bytes
    pub file_size: u64,
    /// Download URL
    pub download_url: String,
    /// Put.io's CRC32 of the file, when known
    pub crc32: Option<String>,
    /// Numbered name chosen on an earlier attempt ("keep both"), from the queue row
    pub save_as: Option<String>,
    /// The file must keep its path (Radarr/Sonarr): overwrite, never a numbered name
    pub fixed_path: bool,
    /// Queued again after it failed a verify run (feature 034): download from the start
    /// over whatever is there, with no checksum read first
    pub overwrite: bool,
}

/// What [`DownloadManager::transfer`] got: the retry count and, when hashed, the
/// CRC32 of the whole hidden file
struct Transferred {
    attempts: u32,
    crc32: Option<String>,
}

/// How a finished download was checked (feature 034)
struct Verified {
    /// `crc32`, `size` or `none` (verification off)
    method: &'static str,
    crc32: Option<String>,
    redownloads: u32,
}

/// Result of a download operation
#[derive(Debug)]
pub struct DownloadOperationResult {
    /// File ID
    pub file_id: i64,
    /// Whether the download succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Bytes downloaded
    pub bytes_downloaded: u64,
    /// Local file path
    pub file_path: Option<PathBuf>,
    /// A failure that may pass: a later sync queues the file again
    pub transient: bool,
}

/// Active download information (combines file info with progress)
#[derive(Debug, Clone)]
pub struct ActiveDownload {
    /// File ID
    pub file_id: i64,
    /// Filename
    pub filename: String,
    /// File size in bytes
    pub file_size: u64,
    /// Bytes downloaded so far
    pub bytes_downloaded: u64,
    /// Download percentage (0.0-100.0)
    pub percentage: f64,
    /// Download speed in bytes per second
    pub speed_bps: f64,
    /// Estimated time remaining in seconds
    pub eta_seconds: Option<u64>,
    /// Elapsed time in seconds since download started
    pub elapsed_seconds: u64,
    /// "downloading", "checking" while an existing file's checksum is computed, or
    /// "verifying" while a resumed file's part on disk is hashed; then the bytes and
    /// speed measure that read
    pub phase: &'static str,
}

/// Statistics for a sync operation
#[derive(Debug, Clone, Default)]
pub struct SyncStats {
    /// Total files found on Put.io
    pub total_files: usize,
    /// Files successfully downloaded
    pub files_downloaded: usize,
    /// Files skipped (already downloaded)
    pub files_skipped: usize,
    /// Files that failed to download
    pub files_failed: usize,
    /// Files blocked or skipped by a folder rule or the blocked-extension list
    pub files_blocked: usize,
    /// Type-blocked files deleted from Put.io by `delete_blocked`
    pub files_deleted: usize,
    /// Files kept on Put.io by stream mode (feature 022)
    pub files_streamed: usize,
}

/// What `enqueue_tree_report` did with one file
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeDecision {
    /// Added to the download queue
    Queued,
    /// Already downloaded or already in the queue
    Known,
    /// A folder rule's type filter refuses it; holds its extension (empty if none)
    Blocked(String),
    /// A folder rule skips its folder
    Skipped,
    /// Stream mode keeps it on Put.io (feature 022)
    Streamed,
    /// Adding it to the queue failed
    Failed,
}

impl TreeDecision {
    /// The file will be (or already is) downloaded
    pub fn accepted(&self) -> bool {
        matches!(self, TreeDecision::Queued | TreeDecision::Known)
    }
}

/// One file of a tree and what happened to it
#[derive(Debug, Clone)]
pub struct TreeFile {
    /// Put.io file id
    pub file_id: i64,
    /// Put.io file name
    pub name: String,
    /// Bytes
    pub size: i64,
    /// Folder below the target directory it downloads into
    pub relative_path: String,
    /// What happened to it
    pub decision: TreeDecision,
}

/// Result of [`DownloadManager::enqueue_tree_report`]
#[derive(Debug, Clone, Default)]
pub struct TreeReport {
    /// Counts, as the folder sync reports them
    pub stats: SyncStats,
    /// Every file of the tree
    pub files: Vec<TreeFile>,
    /// The root's name on disk (sanitised), unless the root is the Put.io root
    pub root_name: Option<String>,
}

/// One file a rule keeps from being downloaded, in a preview
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewItem {
    /// Put.io file id
    pub file_id: i64,
    /// File name
    pub name: String,
    /// Folder path relative to the Put.io root
    pub path: String,
    /// Blocked or skipped, with the deciding rule
    pub decision: Decision,
    /// The file would be deleted from Put.io
    pub would_delete: bool,
}

/// One file a preview would download
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewDownload {
    /// Put.io file id
    pub file_id: i64,
    /// File name
    pub name: String,
    /// Folder path relative to the Put.io root
    pub path: String,
}

/// What rules would do under a folder (`preview_rules`); nothing is changed
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RulePreview {
    /// Files listed
    pub files: usize,
    /// Files that would be downloaded
    pub download: usize,
    /// Files the type filter blocks
    pub blocked: usize,
    /// Files in skipped folders
    pub skipped: usize,
    /// Files kept on Put.io by stream mode (feature 022)
    pub stream: usize,
    /// Blocked files that would be deleted from Put.io
    pub would_delete: usize,
    /// Blocked, skipped and streamed files, the first `PREVIEW_ITEMS`
    pub items: Vec<PreviewItem>,
    /// More blocked or skipped files than `items` holds
    pub truncated: bool,
    /// Files that would be downloaded, the first `PREVIEW_DOWNLOADS` (feature 020)
    pub downloads: Vec<PreviewDownload>,
}

/// How many files a preview lists
pub const PREVIEW_ITEMS: usize = 200;

/// How many downloads a preview lists
pub const PREVIEW_DOWNLOADS: usize = 50;

/// Worker statistics for bandwidth tracking
#[derive(Debug, Clone)]
pub struct WorkerStats {
    /// Worker ID
    pub worker_id: usize,
    /// Total bytes downloaded by this worker
    pub bytes_downloaded: Arc<RwLock<u64>>,
    /// Always `None`: the bandwidth limit is shared by all workers (feature 032)
    pub bandwidth_limit: Option<u64>,
    /// Worker start time
    pub started_at: Instant,
}

impl WorkerStats {
    fn new(worker_id: usize, bandwidth_limit: Option<u64>) -> Self {
        Self {
            worker_id,
            bytes_downloaded: Arc::new(RwLock::new(0)),
            bandwidth_limit,
            started_at: Instant::now(),
        }
    }

    /// Get average bandwidth usage in bytes per second
    pub async fn avg_bandwidth_bps(&self) -> f64 {
        let bytes = *self.bytes_downloaded.read().await;
        let elapsed = self.started_at.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            bytes as f64 / elapsed
        } else {
            0.0
        }
    }
}

/// Worker information for tracking
struct WorkerInfo {
    handle: JoinHandle<()>,
    shutdown_tx: broadcast::Sender<()>,
    stats: WorkerStats,
}

/// Two paths a worker could not write at once. Case-insensitive, so `A.mkv` and `a.mkv`
/// do not race on a case-insensitive disk (macOS, Windows, a Samba share); on a
/// case-sensitive one it only makes two such files take turns.
fn same_target(a: &Path, b: &Path) -> bool {
    a == b || a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

/// Holds a target path for one worker (R7); released on drop
pub struct PathClaim {
    set: Arc<Mutex<HashSet<PathBuf>>>,
    path: PathBuf,
}

impl Drop for PathClaim {
    fn drop(&mut self) {
        self.set
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.path);
    }
}

/// The extra field of an existing-file log line
enum Detail {
    /// Resumed from this byte
    FromBytes(u64),
    /// Saved under this numbered name, for this reason
    SavedAs(String, &'static str),
    /// Why it restarted or overwrote
    Reason(&'static str),
}

/// A file found at its local path: logs and counts what happened to it
struct Existing<'a> {
    file_id: i64,
    name: &'a str,
    choice: ExistingFile,
}

impl Existing<'_> {
    /// One structured event per outcome, `error` for refused, and the counter
    fn record(&self, path: &std::path::Path, outcome: ExistingOutcome, detail: Option<&Detail>) {
        let (from_bytes, saved_as, reason) = match detail {
            Some(Detail::FromBytes(n)) => (Some(*n), None, None),
            Some(Detail::SavedAs(name, reason)) => (None, Some(name.as_str()), Some(*reason)),
            Some(Detail::Reason(reason)) => (None, None, Some(*reason)),
            None => (None, None, None),
        };
        let path = path.display().to_string();
        if outcome == ExistingOutcome::Refused {
            error!(
                file_id = self.file_id,
                name = self.name,
                path = %path,
                choice = self.choice.as_str(),
                outcome = outcome.as_str(),
                "A folder is in the way of {}; not downloaded",
                self.name
            );
        } else {
            info!(
                file_id = self.file_id,
                name = self.name,
                path = %path,
                choice = self.choice.as_str(),
                outcome = outcome.as_str(),
                from_bytes,
                saved_as,
                reason,
                "Existing file {}: {}",
                self.name,
                outcome.as_str()
            );
        }
        telemetry::download::existing(match outcome {
            ExistingOutcome::Skipped => telemetry::ExistingOutcome::Skipped,
            ExistingOutcome::Resumed => telemetry::ExistingOutcome::Resumed,
            ExistingOutcome::Restarted => telemetry::ExistingOutcome::Restarted,
            ExistingOutcome::Overwritten => telemetry::ExistingOutcome::Overwritten,
            ExistingOutcome::SavedAs => telemetry::ExistingOutcome::SavedAs,
            ExistingOutcome::Refused => telemetry::ExistingOutcome::Refused,
        });
    }
}

/// A checksum read in progress: bytes read, when it started, and the phase shown
type Checking = (u64, Instant, &'static str);

/// Download manager
#[derive(Clone)]
pub struct DownloadManager {
    /// Put.io API client
    pub(crate) client: Arc<PutioApiClient>,
    /// Database (optional for fallback mode)
    pub(crate) database: Option<Arc<Database>>,
    /// Download configuration (shared and mutable for hot reload)
    pub(crate) config: Arc<RwLock<DownloadConfig>>,
    /// Progress trackers for active downloads
    progress_trackers: Arc<RwLock<HashMap<i64, ProgressTracker>>>,
    /// Milliseconds between two `download.progress` events of one file (feature 057)
    progress_event_ms: Arc<std::sync::atomic::AtomicU64>,
    /// Overall progress
    overall_progress: Arc<RwLock<OverallProgress>>,
    /// Active worker pool (for dynamic scaling)
    workers: Arc<RwLock<HashMap<usize, WorkerInfo>>>,
    /// Next worker ID counter
    next_worker_id: Arc<RwLock<usize>>,
    /// File IDs a rule already decided, so each is logged and recorded once per process
    decided: Arc<Mutex<HashSet<i64>>>,
    /// State of the folder sync, for the status page
    sync_status: Arc<RwLock<SyncStatus>>,
    /// Wakes `run_sync_loop` for "sync now"
    sync_notify: Arc<Notify>,
    /// Wakes idle workers when rows are queued, resumed or retried; they also poll every
    /// `IDLE_POLL` in case a row was changed straight in the database
    work_notify: Arc<Notify>,
    /// Set while new downloads are held back for lack of free space
    low_space: Arc<Mutex<Option<LowSpace>>>,
    /// Bytes the running downloads may still write, counted whole-file from their start
    space: SpaceReservations,
    /// When the unreadable-target-directory warning was last logged
    space_warned: Arc<Mutex<Option<Instant>>>,
    /// Transient failures per file id (retries used up on a flaky link): a later sync
    /// queues the row again, at most `MAX_SYNC_REQUEUES` times. In memory, so a restart
    /// starts the count again and the queue table needs no new column.
    transient_failures: Arc<Mutex<HashMap<i64, u32>>>,
    /// Set by `request_sync`, so the next sync's `sync.completed` says `manual` (feature 027)
    manual_sync: Arc<std::sync::atomic::AtomicBool>,
    /// Target paths a worker is writing (R7)
    paths_in_use: Arc<Mutex<HashSet<PathBuf>>>,
    /// Files whose checksum is being computed: bytes read, when it started, and the
    /// phase shown ("checking", or "verifying" for a resumed file's prefix)
    checking: Arc<Mutex<HashMap<i64, Checking>>>,
    /// Stop signals of running downloads, by file id (feature 039)
    stop_signals: Arc<Mutex<HashMap<i64, Arc<StopSignal>>>>,
    /// Files stopped with bytes on disk, to notice a restart from 0
    stopped_partial: Arc<Mutex<HashSet<i64>>>,
    /// In-memory notes per file id
    notes: Arc<Mutex<HashMap<i64, String>>>,
    /// What the last failure of a file was, by file id, for the queue's reason line
    /// (feature 066). In memory: after a restart a failed row has no kind.
    failures: Arc<Mutex<HashMap<i64, &'static str>>>,
    /// The last Put.io transfer list, shared by the Active page, Status and the daemon
    /// loop (feature 039, R3). A plain lock held for a copy, never across the fetch, so
    /// readers (`/status`) never wait on a slow Put.io.
    transfers_cache: Arc<Mutex<Option<CachedTransfers>>>,
    /// Held across the Put.io fetch: one refresh at a time
    transfers_refresh: Arc<tokio::sync::Mutex<()>>,
    /// The transfer classes behind `transfer.*` events; `None` until read from the
    /// database (feature 027)
    transfer_events: Arc<tokio::sync::Mutex<Option<TransferEvents>>>,
    /// The transfer items that were active on the last pipeline tick; `None` until the
    /// first tick (feature 039, R6)
    pub(crate) finish_seen: Arc<tokio::sync::Mutex<Option<crate::finished::SeenItems>>>,
    /// The one bandwidth limiter every download shares (feature 032)
    pub(crate) bandwidth: RateLimiter,
    /// The "verify everything" run (feature 034)
    pub(crate) verifier: crate::verify::VerifyRunner,
    /// Moves a file's folder while the processing pipeline stages it (feature 038)
    redirect: Arc<std::sync::RwLock<Option<Arc<dyn PathRedirect>>>>,
    /// Holds back the Put.io delete of a downloaded file while the pipeline works on it
    /// (feature 025)
    delete_gate: Arc<std::sync::RwLock<Option<Arc<dyn RemoteDeleteGate>>>>,
    /// What runs once for each file stream mode keeps on Put.io (feature 022)
    arrived_steps: Arc<std::sync::RwLock<Vec<Arc<dyn ArrivedStep>>>>,
    /// The plugins' sync rules: filter, rename and folder (feature 028)
    sync_hooks: Arc<std::sync::RwLock<Option<Arc<dyn SyncHooks>>>>,
}

/// What [`DownloadManager::delete_remote`] did
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RemoteDeletes {
    /// Deleted from Put.io
    pub deleted: Vec<i64>,
    /// Not deleted, and never will be: the download is not completed. The caller forgets them.
    pub refused: Vec<i64>,
}

/// The answer of [`RemoteDeleteGate::defer`]
pub type DeferFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send + 'a>>;

/// Lets the processing pipeline (feature 025) hold back the delete of a file that Put.io
/// still has to give subtitles for. `download` calls it and knows nothing of the pipeline.
pub trait RemoteDeleteGate: Send + Sync {
    /// `Some(item key)` when the delete of `file_id` must wait for that pipeline item, or
    /// `None` to delete now
    fn defer<'a>(
        &'a self,
        file_id: i64,
        relative_path: &'a str,
        filename: &'a str,
    ) -> DeferFuture<'a>;
}

/// The answer of [`PathRedirect::redirect`]
pub type RedirectFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send + 'a>>;

/// Lets the processing pipeline (feature 038) point a file at a hidden staging folder.
/// `download` calls it and knows nothing of the pipeline.
pub trait PathRedirect: Send + Sync {
    /// The folder, relative to the download folder, to download file `file_id` into
    /// instead of `relative_path`; `None` keeps it
    fn redirect<'a>(&'a self, file_id: i64, relative_path: &'a str) -> RedirectFuture<'a>;
}

/// The cached transfer list, when it was fetched, and the last attempt
struct CachedTransfers {
    /// The last fetch attempt, successful or not
    attempted: Instant,
    fetched_at: SystemTime,
    transfers: Arc<Vec<Transfer>>,
    /// The last attempt failed
    stale: bool,
}

impl CachedTransfers {
    fn snapshot(&self) -> TransfersSnapshot {
        TransfersSnapshot {
            transfers: self.transfers.clone(),
            fetched_at: self.fetched_at,
            stale: self.stale,
        }
    }
}

/// A Put.io transfer list from `transfers_snapshot`
#[derive(Debug, Clone)]
pub struct TransfersSnapshot {
    pub transfers: Arc<Vec<Transfer>>,
    /// When the list was fetched
    pub fetched_at: SystemTime,
    /// The last fetch failed, so this is an older list (FR-029)
    pub stale: bool,
}

/// The default gap between two `download.progress` events of one file
const PROGRESS_EVENT_MS: u64 = 10_000;

impl DownloadManager {
    /// Create a new download manager
    ///
    /// # Arguments
    ///
    /// * `client` - Put.io API client
    /// * `database` - Database for tracking downloads
    /// * `config` - Download configuration
    pub fn new(
        client: Arc<PutioApiClient>,
        database: Arc<Database>,
        config: DownloadConfig,
    ) -> Self {
        let overall_progress = Arc::new(RwLock::new(OverallProgress::new(0, 0)));

        Self {
            client,
            database: Some(database),
            bandwidth: RateLimiter::from_policy(config.bandwidth_policy()),
            config: Arc::new(RwLock::new(config)),
            progress_trackers: Arc::new(RwLock::new(HashMap::new())),
            progress_event_ms: Arc::new(std::sync::atomic::AtomicU64::new(PROGRESS_EVENT_MS)),
            overall_progress,
            workers: Arc::new(RwLock::new(HashMap::new())),
            next_worker_id: Arc::new(RwLock::new(0)),
            decided: Arc::new(Mutex::new(HashSet::new())),
            sync_status: Arc::new(RwLock::new(SyncStatus::default())),
            sync_notify: Arc::new(Notify::new()),
            work_notify: Arc::new(Notify::new()),
            low_space: Arc::new(Mutex::new(None)),
            space: SpaceReservations::default(),
            space_warned: Arc::new(Mutex::new(None)),
            transient_failures: Arc::new(Mutex::new(HashMap::new())),
            manual_sync: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            stop_signals: Arc::new(Mutex::new(HashMap::new())),
            stopped_partial: Arc::new(Mutex::new(HashSet::new())),
            notes: Arc::new(Mutex::new(HashMap::new())),
            failures: Arc::new(Mutex::new(HashMap::new())),
            transfers_cache: Arc::new(Mutex::new(None)),
            transfers_refresh: Arc::new(tokio::sync::Mutex::new(())),
            transfer_events: Arc::new(tokio::sync::Mutex::new(None)),
            finish_seen: Arc::new(tokio::sync::Mutex::new(None)),
            paths_in_use: Arc::new(Mutex::new(HashSet::new())),
            checking: Arc::new(Mutex::new(HashMap::new())),
            verifier: crate::verify::VerifyRunner::default(),
            redirect: Arc::new(std::sync::RwLock::new(None)),
            delete_gate: Arc::new(std::sync::RwLock::new(None)),
            arrived_steps: Arc::new(std::sync::RwLock::new(Vec::new())),
            sync_hooks: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Create a new download manager without database (fallback mode)
    ///
    /// # Arguments
    ///
    /// * `client` - Put.io API client
    /// * `config` - Download configuration
    pub fn new_without_database(client: Arc<PutioApiClient>, config: DownloadConfig) -> Self {
        let overall_progress = Arc::new(RwLock::new(OverallProgress::new(0, 0)));

        warn!("Download manager initialized without database - download tracking disabled");

        Self {
            client,
            database: None,
            bandwidth: RateLimiter::from_policy(config.bandwidth_policy()),
            config: Arc::new(RwLock::new(config)),
            progress_trackers: Arc::new(RwLock::new(HashMap::new())),
            progress_event_ms: Arc::new(std::sync::atomic::AtomicU64::new(PROGRESS_EVENT_MS)),
            overall_progress,
            workers: Arc::new(RwLock::new(HashMap::new())),
            next_worker_id: Arc::new(RwLock::new(0)),
            decided: Arc::new(Mutex::new(HashSet::new())),
            sync_status: Arc::new(RwLock::new(SyncStatus::default())),
            sync_notify: Arc::new(Notify::new()),
            work_notify: Arc::new(Notify::new()),
            low_space: Arc::new(Mutex::new(None)),
            space: SpaceReservations::default(),
            space_warned: Arc::new(Mutex::new(None)),
            transient_failures: Arc::new(Mutex::new(HashMap::new())),
            manual_sync: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            stop_signals: Arc::new(Mutex::new(HashMap::new())),
            stopped_partial: Arc::new(Mutex::new(HashSet::new())),
            notes: Arc::new(Mutex::new(HashMap::new())),
            failures: Arc::new(Mutex::new(HashMap::new())),
            transfers_cache: Arc::new(Mutex::new(None)),
            transfers_refresh: Arc::new(tokio::sync::Mutex::new(())),
            transfer_events: Arc::new(tokio::sync::Mutex::new(None)),
            finish_seen: Arc::new(tokio::sync::Mutex::new(None)),
            paths_in_use: Arc::new(Mutex::new(HashSet::new())),
            checking: Arc::new(Mutex::new(HashMap::new())),
            verifier: crate::verify::VerifyRunner::default(),
            redirect: Arc::new(std::sync::RwLock::new(None)),
            delete_gate: Arc::new(std::sync::RwLock::new(None)),
            arrived_steps: Arc::new(std::sync::RwLock::new(Vec::new())),
            sync_hooks: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// Update download configuration dynamically (hot reload)
    ///
    /// This allows changing configuration without restarting workers or losing queue state.
    /// Changes take effect immediately for:
    /// - Bandwidth limits (applied to active and new downloads)
    /// - Worker count (scales workers up/down gracefully)
    /// - Buffer sizes (applied to new downloads)
    /// - Other config options
    ///
    /// # Arguments
    ///
    /// * `new_config` - New configuration to apply
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use putio_sync_download::{DownloadManager, DownloadConfig};
    /// # async fn example(manager: &DownloadManager) {
    /// let mut new_config = DownloadConfig::default();
    /// new_config.workers = 4;
    /// new_config.bandwidth_limit_bytes_per_sec = Some(1_000_000); // 1 MB/s
    /// manager.update_config(new_config).await;
    /// # }
    /// ```
    pub async fn update_config(&self, new_config: DownloadConfig) {
        // Check if worker count changed for dynamic scaling
        let old_worker_count = {
            let config = self.config.read().await;
            config.workers
        };
        let new_worker_count = new_config.workers;

        self.bandwidth.set_policy(new_config.bandwidth_policy());

        // Update configuration
        {
            let mut config = self.config.write().await;
            *config = new_config;
            info!("Download configuration updated via hot reload");
            info!("  Workers: {}", config.workers);
            info!(
                "  Bandwidth limit: {:?}, {} schedule entries",
                config.bandwidth_limit_bytes_per_sec,
                config.bandwidth_schedule.len()
            );
            info!("  Buffer size: {} MB", config.buffer_size_mb);
        }

        // Handle worker scaling if count changed
        if new_worker_count != old_worker_count {
            info!(
                "Worker count changed: {} -> {}, scaling workers",
                old_worker_count, new_worker_count
            );
            self.scale_workers(new_worker_count).await;
        }
    }

    /// Get current configuration (read-only snapshot)
    pub async fn get_config(&self) -> DownloadConfig {
        self.config.read().await.clone()
    }

    /// The bandwidth limiter every download shares
    pub fn bandwidth(&self) -> &RateLimiter {
        &self.bandwidth
    }

    /// Pause or slow downloads for `minutes` (1 to 1440), replacing any pause or
    /// slow-down; stored so a restart keeps the same end time
    pub async fn set_bandwidth_override(
        &self,
        mode: OverrideMode,
        minutes: u32,
    ) -> DownloadResult<BandwidthStatus> {
        let ov = Override::for_minutes(mode, minutes, chrono::Utc::now())
            .map_err(DownloadError::InvalidInput)?;
        if let Some(db) = &self.database {
            let json = serde_json::to_string(&ov)
                .map_err(|e| DownloadError::InternalError(e.to_string()))?;
            db.flags()
                .set_text(putio_sync_db::pipeline::BANDWIDTH_OVERRIDE, &json)
                .await?;
        }
        self.bandwidth.set_override(Some(ov));
        info!(mode = ?mode, minutes, until = %ov.until, "Bandwidth override set");
        Ok(self.bandwidth.status())
    }

    /// End a pause or slow-down; the schedule applies again
    pub async fn clear_bandwidth_override(&self) -> DownloadResult<BandwidthStatus> {
        if let Some(db) = &self.database {
            db.flags()
                .remove(putio_sync_db::pipeline::BANDWIDTH_OVERRIDE)
                .await?;
        }
        if self.bandwidth.override_().is_some() {
            info!("Bandwidth override ended");
        }
        self.bandwidth.set_override(None);
        Ok(self.bandwidth.status())
    }

    /// Bring back a stored pause or slow-down that has not ended yet, at start-up.
    /// An expired or unreadable one is deleted.
    pub async fn restore_bandwidth_override(&self) -> DownloadResult<Option<Override>> {
        let Some(db) = &self.database else {
            return Ok(None);
        };
        let key = putio_sync_db::pipeline::BANDWIDTH_OVERRIDE;
        let Some(text) = db.flags().get_text(key).await? else {
            return Ok(None);
        };
        match serde_json::from_str::<Override>(&text) {
            Ok(ov) if ov.active(chrono::Utc::now()) => {
                info!(mode = ?ov.mode, until = %ov.until, "Bandwidth override restored");
                self.bandwidth.set_override(Some(ov));
                Ok(Some(ov))
            }
            Ok(_) => {
                db.flags().remove(key).await?;
                Ok(None)
            }
            Err(e) => {
                warn!("Ignoring the stored bandwidth override: {e}");
                db.flags().remove(key).await?;
                Ok(None)
            }
        }
    }

    /// Scale workers to target count (for hot reload)
    ///
    /// Dynamically adjusts the number of active workers:
    /// - If increasing: spawn new workers
    /// - If decreasing: gracefully shutdown excess workers
    async fn scale_workers(&self, target_count: usize) {
        let current_count = {
            let workers = self.workers.read().await;
            workers.len()
        };

        match target_count.cmp(&current_count) {
            std::cmp::Ordering::Greater => {
                // Scale up: spawn new workers
                let to_spawn = target_count - current_count;
                info!("Scaling up: spawning {} new workers", to_spawn);
                for _ in 0..to_spawn {
                    self.spawn_worker().await;
                }
            }
            std::cmp::Ordering::Less => {
                // Scale down: shutdown excess workers
                let to_shutdown = current_count - target_count;
                info!("Scaling down: shutting down {} workers", to_shutdown);

                let worker_ids: Vec<usize> = {
                    let workers = self.workers.read().await;
                    workers.keys().take(to_shutdown).copied().collect()
                };

                for worker_id in worker_ids {
                    self.shutdown_worker(worker_id).await;
                }
            }
            std::cmp::Ordering::Equal => {
                // No change needed
                debug!("Worker count unchanged at {}", target_count);
            }
        }
    }

    /// Spawn a new persistent worker
    async fn spawn_worker(&self) {
        let worker_id = {
            let mut next_id = self.next_worker_id.write().await;
            let id = *next_id;
            *next_id += 1;
            id
        };

        info!("Spawning worker {}", worker_id);

        // The bandwidth limit is shared, not split per worker
        let stats = WorkerStats::new(worker_id, None);

        // Create shutdown channel for this worker
        let (shutdown_tx, mut shutdown_rx) = broadcast::channel(1);

        let worker_dm = self.clone();
        let work_notify = self.work_notify.clone();
        let handle = tokio::spawn(async move {
            debug!("Worker {} started", worker_id);

            loop {
                // Check for shutdown signal
                match shutdown_rx.try_recv() {
                    Ok(_) | Err(broadcast::error::TryRecvError::Closed) => {
                        info!(
                            "Worker {} received shutdown signal, exiting gracefully",
                            worker_id
                        );
                        break;
                    }
                    Err(broadcast::error::TryRecvError::Empty)
                    | Err(broadcast::error::TryRecvError::Lagged(_)) => {
                        // No shutdown signal, continue working
                    }
                }

                // Try to process one item from queue
                match WORKER_ID.scope(worker_id, worker_dm.process_queue()).await {
                    Ok(true) => {
                        // Processed an item, continue
                        debug!("Worker {} processed an item", worker_id);
                    }
                    Ok(false) => {
                        // Nothing to do: wait for a wakeup, a shutdown or the fallback poll
                        tokio::select! {
                            _ = work_notify.notified() => {}
                            _ = shutdown_rx.recv() => {
                                info!("Worker {} received shutdown signal, exiting gracefully", worker_id);
                                break;
                            }
                            _ = tokio::time::sleep(IDLE_POLL) => {}
                        }
                    }
                    Err(e) => {
                        error!("Worker {} error: {}, sleeping before retry", worker_id, e);
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                }
            }

            debug!("Worker {} exited", worker_id);
        });

        // Store worker info
        let mut workers = self.workers.write().await;
        workers.insert(
            worker_id,
            WorkerInfo {
                handle,
                shutdown_tx,
                stats: stats.clone(),
            },
        );
        info!(
            "Worker {} spawned successfully (total workers: {})",
            worker_id,
            workers.len()
        );
    }

    /// Shutdown a specific worker gracefully
    async fn shutdown_worker(&self, worker_id: usize) {
        info!("Shutting down worker {}", worker_id);

        let worker_info = {
            let mut workers = self.workers.write().await;
            workers.remove(&worker_id)
        };

        if let Some(worker) = worker_info {
            // Send shutdown signal
            if let Err(e) = worker.shutdown_tx.send(()) {
                warn!(
                    "Failed to send shutdown signal to worker {}: {}",
                    worker_id, e
                );
            }

            // Wait for worker to finish (with timeout)
            match tokio::time::timeout(tokio::time::Duration::from_secs(30), worker.handle).await {
                Ok(Ok(())) => {
                    info!("Worker {} shut down gracefully", worker_id);
                }
                Ok(Err(e)) => {
                    warn!("Worker {} panicked during shutdown: {}", worker_id, e);
                }
                Err(_) => {
                    warn!(
                        "Worker {} shutdown timed out (may still be processing)",
                        worker_id
                    );
                }
            }
        } else {
            warn!("Worker {} not found in worker pool", worker_id);
        }

        let remaining = self.workers.read().await.len();
        info!(
            "Worker {} removed (remaining workers: {})",
            worker_id, remaining
        );
    }

    /// Put queue rows a crash left in `downloading` back to `pending`
    ///
    /// Call once at startup, before `start_workers`. Their partial `.<name>.downloading`
    /// files resume from where they stopped. Returns the count; 0 without a database.
    pub async fn recover_interrupted(&self) -> DownloadResult<usize> {
        self.verify().recover().await;
        let Some(db) = self.database.as_ref() else {
            return Ok(0);
        };
        let count = db.queue().requeue_interrupted().await.map_err(|e| {
            DownloadError::InternalError(format!("Failed to requeue interrupted downloads: {}", e))
        })?;
        if count > 0 {
            info!(
                count,
                "Requeued interrupted downloads left in downloading state"
            );
        }
        Ok(count)
    }

    /// A worker is writing `path` (feature 034)
    pub(crate) fn path_in_use(&self, path: &Path) -> bool {
        locked(&self.paths_in_use)
            .iter()
            .any(|p| same_target(p, path))
    }

    /// The `.downloading` paths workers are writing now: one per claimed target (044)
    pub fn partial_paths_in_use(&self) -> Vec<PathBuf> {
        self.paths_in_use
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(|target| create_temp_path(target).ok())
            .collect()
    }

    /// Get current worker count (for monitoring)
    pub async fn active_workers(&self) -> usize {
        self.workers.read().await.len()
    }

    /// Start persistent worker pool
    ///
    /// Spawns initial workers based on configuration.
    /// Workers run continuously until shutdown.
    pub async fn start_workers(&self) {
        let worker_count = {
            let config = self.config.read().await;
            config.workers
        };

        info!("Starting {} persistent workers", worker_count);
        for _ in 0..worker_count {
            self.spawn_worker().await;
        }
    }

    /// Shutdown all workers gracefully
    pub async fn shutdown_all_workers(&self) {
        info!("Shutting down all workers");

        let worker_ids: Vec<usize> = {
            let workers = self.workers.read().await;
            workers.keys().copied().collect()
        };

        for worker_id in worker_ids {
            self.shutdown_worker(worker_id).await;
        }

        info!("All workers shut down");
    }

    /// Let the processing pipeline stage watch-folder downloads (feature 038)
    pub fn set_redirect(&self, redirect: Arc<dyn PathRedirect>) {
        *write_locked(&self.redirect) = Some(redirect);
    }

    /// Let plugins filter, rename and place the files of a sync (feature 028)
    pub fn set_sync_hooks(&self, hooks: Arc<dyn SyncHooks>) {
        *write_locked(&self.sync_hooks) = Some(hooks);
    }

    /// The sync hooks, when one provides `hook`
    pub(crate) fn hooks_for(&self, hook: Hook) -> Option<Arc<dyn SyncHooks>> {
        self.sync_hooks
            .read()
            .unwrap()
            .clone()
            .filter(|h| h.provides(hook))
    }

    /// The local folder and name plugins give a download, `None` for the usual ones. The
    /// *arr's files keep theirs; a numbered name from an earlier attempt is kept.
    async fn placed(&self, file: &DownloadFile) -> (Option<String>, Option<String>) {
        if file.fixed_path {
            return (None, None);
        }
        let asked = hook_file(
            file.file_id,
            &file.filename,
            file.file_size,
            &file.relative_path,
            &Decision::Download,
        );
        let folder = match self.hooks_for(Hook::Folder) {
            Some(hooks) => hooks.folder(&asked).await,
            None => None,
        };
        let name = match self.hooks_for(Hook::Rename) {
            Some(hooks) if file.save_as.is_none() => hooks.rename(&asked).await,
            _ => None,
        };
        if folder.is_some() || name.is_some() {
            debug!(file_id = file.file_id, folder = ?folder, name = ?name, "Plugin placed download");
        }
        (folder, name)
    }

    /// Let the pipeline hold back deletes from Put.io (feature 025)
    pub fn set_delete_gate(&self, gate: Arc<dyn RemoteDeleteGate>) {
        *write_locked(&self.delete_gate) = Some(gate);
    }

    /// The steps each newly streamed file gets, in order (feature 022). Files already
    /// recorded keep the steps they were given.
    pub fn set_arrived_steps(&self, steps: Vec<Arc<dyn ArrivedStep>>) {
        *write_locked(&self.arrived_steps) = steps;
    }

    /// Delete files from Put.io that the pipeline held back and now released. Each id is
    /// checked against its `downloads` row first: a file whose download is not completed
    /// and on disk is never deleted.
    pub async fn delete_remote(&self, file_ids: &[i64]) -> RemoteDeletes {
        let mut result = RemoteDeletes::default();
        let Some(db) = self.database.as_ref() else {
            return result;
        };
        let mut ok = Vec::new();
        for id in file_ids {
            match db.downloads().find_by_file_id(*id).await {
                Ok(Some(d)) if d.status == "completed" && !d.local_path.is_empty() => ok.push(*id),
                Ok(None) => {
                    warn!(file_id = id, "Held-back delete not done: no download row")
                }
                Ok(Some(_)) => {
                    warn!(
                        file_id = id,
                        "Held-back delete refused: the download is not completed"
                    );
                    result.refused.push(*id);
                }
                Err(e) => {
                    warn!(file_id = id, error = %e, "Held-back delete not done: database error")
                }
            }
        }
        if ok.is_empty() {
            return result;
        }
        match self.client.delete_files(ok.clone()).await {
            Ok(_) => {
                info!(count = ok.len(), "Deleted held-back files from Put.io");
                cleanup_deleted(&ok);
                result.deleted = ok;
            }
            Err(e) => {
                warn!(count = ok.len(), error = %error_chain(&e), "Failed to delete held-back files from Put.io")
            }
        }
        result
    }

    /// How often one file's `download.progress` is sent; 10 seconds unless set
    pub fn set_progress_event_period(&self, period: Duration) {
        self.progress_event_ms.store(
            period.as_millis() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    /// Download a single file
    ///
    /// When something is already at the file's local path, what happens follows the
    /// `on_existing` choice in force for its folder (feature 040): see
    /// [`crate::files::plan_existing`].
    ///
    /// # Arguments
    ///
    /// * `file` - File information
    /// * `progress_callback` - Optional callback for progress updates
    ///
    /// # Returns
    ///
    /// Result of the download operation
    pub async fn download_file(
        &self,
        file: DownloadFile,
        progress_callback: Option<Arc<dyn ProgressCallback>>,
    ) -> DownloadResult<DownloadOperationResult> {
        // A snapshot, so no read lock is held across the download
        let config = self.config.read().await.clone();

        let file_id = file.file_id;
        let filename = sanitize_filename(&file.filename);

        debug!("Starting download: {} (ID: {})", filename, file_id);
        debug!(
            "Download config target_directory: {}",
            config.target_directory.display()
        );
        debug!("File relative_path: '{}'", file.relative_path);

        // Notify callback that file download is starting
        if let Some(ref callback) = progress_callback {
            callback.on_file_start(file_id, &filename, file.file_size);
        }

        // Construct the folder, preserving directory structure. A plugin may place or
        // rename it (028), then the pipeline may stage it (038).
        let (placed, renamed) = self.placed(&file).await;
        let folder = placed.unwrap_or_else(|| file.relative_path.clone());
        let redirect = read_locked(&self.redirect).clone();
        let relative_path = match redirect {
            Some(r) => r.redirect(file_id, &folder).await,
            None => None,
        }
        .unwrap_or(folder);
        // Put.io names are sanitised, but a plugin or the redirect may hand back any path:
        // nothing may leave the target directory
        if Path::new(&relative_path)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            let e = DownloadError::InvalidInput(format!(
                "folder path leaves the download directory: {}",
                relative_path
            ));
            telemetry::download::failed(e.fail_reason());
            self.note_failure(file_id, &e);
            if let Some(ref callback) = progress_callback {
                callback.on_file_complete(file_id, &filename, false);
            }
            return Ok(DownloadOperationResult {
                file_id,
                success: false,
                error: Some(e.to_string()),
                bytes_downloaded: 0,
                file_path: None,
                transient: false,
            });
        }
        let dir = if relative_path.is_empty() {
            config.target_directory.clone()
        } else {
            config.target_directory.join(&relative_path)
        };
        if !dir.exists() {
            std::fs::create_dir_all(&dir).map_err(DownloadError::IoError)?;
        }

        let choice = if file.overwrite {
            ExistingFile::Overwrite
        } else {
            config.effective(&file.relative_path).on_existing.value
        };
        // A numbered name chosen on an earlier attempt is kept (FR-010)
        let name = file
            .save_as
            .as_deref()
            .or(renamed.as_deref())
            .map(sanitize_filename)
            .unwrap_or_else(|| filename.clone());
        let mut target_path = dir.join(&name);
        debug!("Constructed target_path: {}", target_path.display());

        // One writer per path (R7): wait while another worker has it
        let mut claims = vec![self.claim_path(&target_path).await];

        let local = LocalState::of(&target_path).await;
        let crc_match = match file.crc32.as_deref() {
            Some(remote) if needs_checksum(local, file.file_size, Some(remote), choice) => {
                Some(self.checksum_matches(file_id, &target_path, remote).await?)
            }
            _ => None,
        };
        let action = plan_existing(
            local,
            file.file_size,
            crc_match,
            choice,
            config.resume_enabled,
            file.fixed_path,
        );
        let existing = Existing {
            file_id,
            name: &filename,
            choice,
        };

        let temp_path = create_temp_path(&target_path)?;
        let mut outcome = match action {
            ExistingAction::Fresh if file.save_as.is_some() => {
                Some((ExistingOutcome::SavedAs, None))
            }
            ExistingAction::Fresh => None,
            ExistingAction::Skip => {
                existing.record(&target_path, ExistingOutcome::Skipped, None);
                self.record_download(&file, &filename, &target_path, 0, None)
                    .await;
                self.record_history(
                    &file,
                    &filename,
                    &target_path,
                    None,
                    Some(ExistingOutcome::Skipped),
                    None,
                )
                .await;
                if let Some(ref callback) = progress_callback {
                    callback.on_file_complete(file_id, &filename, true);
                }
                return Ok(DownloadOperationResult {
                    file_id,
                    success: true,
                    error: None,
                    bytes_downloaded: 0,
                    file_path: Some(target_path),
                    transient: false,
                });
            }
            ExistingAction::Refuse => {
                existing.record(&target_path, ExistingOutcome::Refused, None);
                let e = DownloadError::InvalidInput(format!(
                    "target path is a folder: {}",
                    target_path.display()
                ));
                telemetry::download::failed(e.fail_reason());
                self.note_failure(file_id, &e);
                if let Some(ref callback) = progress_callback {
                    callback.on_file_complete(file_id, &filename, false);
                }
                return Ok(DownloadOperationResult {
                    file_id,
                    success: false,
                    error: Some(e.to_string()),
                    bytes_downloaded: 0,
                    file_path: None,
                    transient: false,
                });
            }
            ExistingAction::Resume { from } => {
                // The partial file continues from its hidden name, so a half-resumed
                // file never sits at the real name (R3)
                let _ = tokio::fs::remove_file(&temp_path).await;
                tokio::fs::rename(&target_path, &temp_path).await?;
                Some((ExistingOutcome::Resumed, Some(Detail::FromBytes(from))))
            }
            ExistingAction::Overwrite { fixed_path } => Some((
                ExistingOutcome::Overwritten,
                fixed_path.then_some(Detail::Reason("fixed_path")),
            )),
            ExistingAction::SaveAs { reason } => {
                let numbered = {
                    let mut in_use = self.paths_in_use.lock().unwrap_or_else(|e| e.into_inner());
                    let numbered = next_free_name(&dir, &name, &in_use);
                    in_use.insert(dir.join(&numbered));
                    numbered
                };
                target_path = dir.join(&numbered);
                claims.push(PathClaim {
                    set: self.paths_in_use.clone(),
                    path: target_path.clone(),
                });
                if let Some(db) = &self.database {
                    if let Err(e) = db.queue().set_save_as(file_id, Some(&numbered)).await {
                        warn!("Failed to keep the numbered name for {}: {}", filename, e);
                    }
                }
                Some((
                    ExistingOutcome::SavedAs,
                    Some(Detail::SavedAs(numbered, reason.as_str())),
                ))
            }
        };
        let temp_path = create_temp_path(&target_path)?;

        let _active = telemetry::download::active();
        let started = Instant::now();

        if file.overwrite {
            // A re-download after a failed verify starts from byte 0 (R5)
            let _ = tokio::fs::remove_file(&temp_path).await;
        }
        let resumed_from = if config.resume_enabled {
            check_resume(&temp_path, file.file_size).await?
        } else {
            // Resume off: a hidden partial file is started over (R9)
            let _ = tokio::fs::remove_file(&temp_path).await;
            None
        };
        if resumed_from.is_none() && locked(&self.stopped_partial).remove(&file_id) {
            self.notes
                .lock()
                .unwrap()
                .insert(file_id, RESTARTED_NOTE.to_string());
        }
        if let Some(from) = resumed_from {
            info!(
                "Attempting to resume download from byte {} of {}",
                from, file.file_size
            );
        }

        // Put.io's CRC32, when it is a real one (R1)
        let remote = valid_crc32(file.crc32.as_deref());
        let verify = config.verify.enabled;
        // With verification off, 040's rule stays: only a resumed file is compared, and
        // it is downloaded again once (R3)
        let max_redownloads = if verify {
            config.verify.max_redownloads
        } else {
            u32::from(resumed_from.is_some())
        };
        let hash = remote.is_some() && (verify || resumed_from.is_some());
        let mut redownloads = 0u32;
        // Every download has a start, sent before its finished or failed event (057)
        let starting_as = target_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| name.clone());
        events::emit(Payload::DownloadStarted(events::DownloadStarted {
            file_id,
            relative_path: event_path(&file.relative_path, &starting_as),
            name: starting_as,
            size_bytes: file.file_size,
            resumed: resumed_from.is_some_and(|from| from > 0),
            resumed_from_bytes: resumed_from.unwrap_or(0),
        }));
        let mut result = self
            .transfer(&file, &filename, &target_path, &config, resumed_from, hash)
            .await;
        while let (true, Ok(done), Some(expected)) = (hash, &result, remote.as_deref()) {
            let actual = done.crc32.clone().unwrap_or_default();
            let passed = crc_equal(&actual, expected);
            if verify {
                telemetry::verify::record_check("download", "crc32", passed);
            }
            if passed {
                break;
            }
            let attempt = redownloads + 1;
            let will_redownload = redownloads < max_redownloads;
            warn!(
                file_id,
                name = %filename,
                method = "crc32",
                expected,
                actual = %actual,
                attempt,
                "Download does not match Put.io's CRC32"
            );
            let _ = tokio::fs::remove_file(&temp_path).await;
            if verify {
                events::emit(Payload::VerifyFailed(events::VerifyFailed {
                    file_id,
                    name: name.clone(),
                    relative_path: event_path(&file.relative_path, &name),
                    local_path: target_path.display().to_string(),
                    size_bytes: file.file_size,
                    expected_crc32: Some(expected.to_string()),
                    actual_crc32: Some(actual.clone()),
                    attempt,
                    will_redownload,
                    trigger: "download".to_string(),
                }));
            }
            if !will_redownload {
                let message = format!(
                    "Checksum mismatch: {} did not match Put.io's CRC32 {} (got {}) after {} downloads",
                    filename, expected, actual, attempt
                );
                if verify {
                    self.record_problem(&file, &name, &target_path, &message)
                        .await;
                }
                result = Err(DownloadError::ApiError(message));
                break;
            }
            redownloads += 1;
            if verify {
                telemetry::verify::record_redownload("download");
                events::emit(Payload::VerifyRedownload(events::VerifyRedownload {
                    file_id,
                    name: name.clone(),
                    relative_path: event_path(&file.relative_path, &name),
                    size_bytes: file.file_size,
                    attempt: redownloads,
                    max_redownloads,
                    trigger: "download".to_string(),
                }));
            }
            if matches!(outcome, Some((ExistingOutcome::Resumed, _))) {
                outcome = Some((ExistingOutcome::Restarted, Some(Detail::Reason("checksum"))));
            }
            let previous = match &result {
                Ok(done) => done.attempts,
                Err(_) => 0,
            };
            result = self
                .transfer(&file, &filename, &target_path, &config, None, hash)
                .await
                .map(|done| Transferred {
                    attempts: previous + done.attempts,
                    ..done
                });
        }
        if verify && remote.is_none() && result.is_ok() {
            // stream_to_temp already refused a file of the wrong size
            telemetry::verify::record_check("download", "size", true);
        }
        let verified = Verified {
            method: match (verify, &remote) {
                (false, _) => "none",
                (true, Some(_)) => "crc32",
                (true, None) => "size",
            },
            crc32: remote.clone(),
            redownloads,
        };

        let result = match result {
            Ok(done) => finish_download(&temp_path, &target_path)
                .await
                .map(|_| done.attempts),
            Err(e) => Err(e),
        };

        match result {
            Ok(attempts) => {
                let duration = started.elapsed();
                // A successful result means the temp file was verified and renamed.
                telemetry::download::completed(duration);
                locked(&self.notes).remove(&file_id);
                locked(&self.failures).remove(&file_id);
                if let Some((kind, detail)) = &outcome {
                    existing.record(&target_path, *kind, detail.as_ref());
                }
                let fetched = file.file_size - resumed_from.unwrap_or(0);
                if let Some(ref callback) = progress_callback {
                    let mut progress =
                        DownloadProgress::new(file_id, filename.clone(), file.file_size);
                    progress.bytes_downloaded = file.file_size;
                    progress.complete = true;
                    callback.on_file_progress(&progress);
                }
                self.record_download(&file, &filename, &target_path, attempts, Some(&verified))
                    .await;
                let avg_speed =
                    (duration.as_secs() > 0).then(|| (fetched / duration.as_secs()) as i64);
                self.record_history(
                    &file,
                    &filename,
                    &target_path,
                    Some((duration.as_secs() as i64, avg_speed)),
                    outcome.map(|(kind, _)| kind),
                    Some(&verified),
                )
                .await;
                info!(
                    "Download completed: {} ({} bytes in {:.2}s)",
                    filename,
                    file.file_size,
                    duration.as_secs_f64()
                );
                let saved_as = target_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| name.clone());
                if verify {
                    info!(
                        file_id,
                        name = %saved_as,
                        method = verified.method,
                        crc32 = verified.crc32.as_deref().unwrap_or(""),
                        redownloads,
                        "Download verified"
                    );
                    self.mark_problems_fixed(file_id).await;
                    // Before download.finished, so 031's refresh only sees checked files (R4)
                    events::emit(Payload::VerifyPassed(events::VerifyPassed {
                        file_id,
                        relative_path: event_path(&file.relative_path, &saved_as),
                        name: saved_as.clone(),
                        local_path: target_path.display().to_string(),
                        size_bytes: file.file_size,
                        method: verified.method.to_string(),
                        crc32: verified.crc32.clone(),
                        redownloads,
                        trigger: "download".to_string(),
                    }));
                }
                events::emit(Payload::DownloadFinished(events::DownloadFinished {
                    file_id,
                    relative_path: event_path(&file.relative_path, &saved_as),
                    name: saved_as,
                    local_path: target_path.display().to_string(),
                    size_bytes: file.file_size,
                    duration_seconds: duration.as_secs_f64(),
                    average_bytes_per_second: (fetched as f64 / duration.as_secs_f64().max(0.001))
                        as u64,
                    resumed: resumed_from.is_some_and(|from| from > 0),
                }));
                if let Some(ref callback) = progress_callback {
                    callback.on_file_complete(file_id, &filename, true);
                }
                drop(claims);
                Ok(DownloadOperationResult {
                    file_id,
                    success: true,
                    error: None,
                    bytes_downloaded: file.file_size,
                    file_path: Some(target_path),
                    transient: false,
                })
            }
            Err(DownloadError::Stopped) => {
                info!(file_id, name = %filename, "Download stopped; partial file kept");
                if tokio::fs::metadata(&temp_path)
                    .await
                    .is_ok_and(|m| m.len() > 0)
                {
                    locked(&self.stopped_partial).insert(file_id);
                }
                Err(DownloadError::Stopped)
            }
            Err(e) => {
                telemetry::download::failed(e.fail_reason());
                self.note_failure(file_id, &e);
                error!("Download failed for {}: {}", filename, e);
                events::emit(Payload::DownloadFailed(events::DownloadFailed {
                    file_id,
                    relative_path: event_path(&file.relative_path, &name),
                    name: name.clone(),
                    size_bytes: file.file_size,
                    error: redact::text(&e.to_string()),
                    reason: e.fail_reason().as_str().to_string(),
                    attempts: config.max_retries + 1,
                }));
                if let DownloadError::ApiError(ref msg) = e {
                    if msg.contains("404") {
                        warn!("File no longer available (404): {}", filename);
                        cleanup_partial(&temp_path).await;
                    }
                }
                if let Some(ref callback) = progress_callback {
                    callback.on_file_complete(file_id, &filename, false);
                }
                Ok(DownloadOperationResult {
                    file_id,
                    success: false,
                    error: Some(e.to_string()),
                    bytes_downloaded: resumed_from.unwrap_or(0),
                    file_path: None,
                    transient: e.is_transient(),
                })
            }
        }
    }

    /// Wait until no other worker writes `path`, then hold it until the claim drops
    pub async fn claim_path(&self, path: &std::path::Path) -> PathClaim {
        // Names that differ only in case count as one path
        loop {
            {
                let mut in_use = self.paths_in_use.lock().unwrap_or_else(|e| e.into_inner());
                if !in_use.iter().any(|p| same_target(p, path)) {
                    in_use.insert(path.to_path_buf());
                    return PathClaim {
                        set: self.paths_in_use.clone(),
                        path: path.to_path_buf(),
                    };
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }

    /// Compare a local file's CRC32 with Put.io's. The file shows as "checking" on the
    /// Queue page meanwhile, with the bytes read as its progress (R4).
    async fn checksum_matches(
        &self,
        file_id: i64,
        path: &std::path::Path,
        remote: &str,
    ) -> DownloadResult<bool> {
        let started = Instant::now();
        let local = self
            .with_read_progress(file_id, "checking", |progress| crc32_file(path, progress))
            .await?;
        debug!(
            "Checksum of {}: {} (Put.io {}) in {:.2}s",
            path.display(),
            local,
            remote,
            started.elapsed().as_secs_f64()
        );
        Ok(crc_equal(&local, remote))
    }

    /// Run a file read with its progress shown on the Queue page as `phase`
    async fn with_read_progress<T, Fut>(
        &self,
        file_id: i64,
        phase: &'static str,
        read: impl FnOnce(Box<dyn Fn(u64) + Send + 'static>) -> Fut,
    ) -> T
    where
        Fut: std::future::Future<Output = T>,
    {
        self.checking
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(file_id, (0, Instant::now(), phase));
        let checking = self.checking.clone();
        let result = read(Box::new(move |bytes| {
            if let Some(entry) = checking
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_mut(&file_id)
            {
                entry.0 = bytes;
            }
        }))
        .await;
        self.checking
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&file_id);
        result
    }

    /// Stream `file` into the hidden file of `target_path`, with retries. Each attempt
    /// continues from what the hidden file holds, so a retry after a failed attempt
    /// never asks for a range the file lacks. With `hash`, each attempt's CRC32 starts
    /// from the part already on disk (read back, shown as "verifying") and goes on over
    /// the bytes written (feature 034).
    async fn transfer(
        &self,
        file: &DownloadFile,
        filename: &str,
        target_path: &std::path::Path,
        config: &DownloadConfig,
        resume_from: Option<u64>,
        hash: bool,
    ) -> DownloadResult<Transferred> {
        self.progress_trackers
            .write()
            .await
            .insert(file.file_id, ProgressTracker::new(resume_from.unwrap_or(0)));

        let temp_path = create_temp_path(target_path)?;
        let client = &self.client;
        let resume_enabled = config.resume_enabled;
        let stop = self.stop_signal(file.file_id);
        let label = format!("Download {}", filename);
        let download = retry_with_stall_detection(
            || async {
                let from = if resume_enabled {
                    check_resume(&temp_path, file.file_size).await?
                } else {
                    None
                };
                let digest = match from {
                    _ if !hash => None,
                    Some(len) if len > 0 => {
                        let digest = self
                            .with_read_progress(file.file_id, "verifying", |progress| {
                                crc32_prefix(&temp_path, len, progress)
                            })
                            .await?;
                        telemetry::verify::record_read(len);
                        Some(digest)
                    }
                    _ => Some(Crc32::new()),
                };
                stream_to_temp(
                    client,
                    &file.download_url,
                    target_path,
                    filename,
                    Some(file.file_size),
                    config.buffer_size_mb,
                    Some(&self.bandwidth),
                    from,
                    stop.as_deref(),
                    digest,
                )
                .await
            },
            config.max_retries,
            config.retry_initial_delay_secs,
            &label,
            stop.as_deref(),
        );
        // `download.progress` while it runs: at most one per file per period, and only
        // subscribers that name the type get it (feature 057)
        let period = Duration::from_millis(
            self.progress_event_ms
                .load(std::sync::atomic::Ordering::Relaxed)
                .max(1),
        );
        let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tokio::pin!(download);
        let result = loop {
            tokio::select! {
                done = &mut download => break done,
                _ = ticker.tick() => {
                    let Ok(meta) = tokio::fs::metadata(&temp_path).await else {
                        continue;
                    };
                    let downloaded = meta.len().min(file.file_size);
                    let name = target_path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| filename.to_string());
                    events::emit(Payload::DownloadProgress(events::DownloadProgress {
                        file_id: file.file_id,
                        relative_path: event_path(&file.relative_path, &name),
                        name,
                        size_bytes: file.file_size,
                        downloaded_bytes: downloaded,
                        percent: if file.file_size == 0 {
                            0.0
                        } else {
                            (downloaded as f64 * 1000.0 / file.file_size as f64).round() / 10.0
                        },
                    }));
                }
            }
        };
        result.map(|r| Transferred {
            attempts: r.attempts,
            crc32: r.value.crc32,
        })
    }

    /// Record a `completed` downloads row, so the file counts as downloaded
    async fn record_download(
        &self,
        file: &DownloadFile,
        filename: &str,
        path: &std::path::Path,
        retry_count: u32,
        verified: Option<&Verified>,
    ) {
        let Some(database) = &self.database else {
            debug!(
                "Database not available - skipping download record for {}",
                filename
            );
            return;
        };
        let id = match database
            .downloads()
            .insert_with_retry_count(
                file.file_id,
                filename,
                file.file_size as i64,
                path.to_str().unwrap_or(""),
                DownloadStatus::Completed,
                retry_count as i64,
            )
            .await
        {
            Ok(id) => id,
            Err(e) => {
                warn!("Database error (non-fatal): Failed to record download completion for {}: {}. Download succeeded.", filename, e);
                return;
            }
        };
        if let Some(v) = verified {
            if let Err(e) = database
                .downloads()
                .set_verified(id, v.crc32.as_deref(), v.method)
                .await
            {
                warn!("Failed to record how {} was verified: {}", filename, e);
            }
        }
    }

    /// Keep a download that failed verification for History and the doctor
    async fn record_problem(
        &self,
        file: &DownloadFile,
        name: &str,
        path: &std::path::Path,
        message: &str,
    ) {
        let Some(database) = &self.database else {
            return;
        };
        let problem = putio_sync_db::NewProblem {
            run_id: None,
            source: "download".to_string(),
            file_id: file.file_id,
            name: name.to_string(),
            local_path: Some(path.display().to_string()),
            result: "failed".to_string(),
            detail: Some(redact::text(message)),
            queued: false,
        };
        if let Err(e) = database.verify().add_problem(&problem).await {
            warn!(
                "Failed to record the verification problem for {}: {}",
                name, e
            );
        }
    }

    /// A file that now matches no longer counts as a problem
    async fn mark_problems_fixed(&self, file_id: i64) {
        if let Some(database) = &self.database {
            if let Err(e) = database.verify().mark_fixed(file_id).await {
                warn!(
                    "Failed to mark verification problems of {} fixed: {}",
                    file_id, e
                );
            }
        }
    }

    /// Record a history row with the local path and, for an existing file, the outcome
    async fn record_history(
        &self,
        file: &DownloadFile,
        filename: &str,
        path: &std::path::Path,
        timing: Option<(i64, Option<i64>)>,
        outcome: Option<ExistingOutcome>,
        verified: Option<&Verified>,
    ) {
        let Some(database) = &self.database else {
            return;
        };
        let history = database.history();
        let id = match history
            .insert(
                file.file_id,
                filename,
                file.file_size as i64,
                putio_sync_db::models::HistoryAction::Downloaded,
                timing.map(|(secs, _)| secs),
                timing.and_then(|(_, speed)| speed),
            )
            .await
        {
            Ok(id) => id,
            Err(e) => {
                warn!("Failed to record download history for {}: {}", filename, e);
                return;
            }
        };
        if let Err(e) = history
            .set_outcome(id, path.to_str(), outcome.map(|o| o.as_str()))
            .await
        {
            warn!("Failed to record the outcome for {}: {}", filename, e);
        }
        if let Some(v) = verified {
            if let Err(e) = history.set_verify(id, v.method, v.redownloads).await {
                warn!("Failed to record how {} was verified: {}", filename, e);
            }
        }
    }

    /// Sync downloads from Put.io
    ///
    /// Lists all files from Put.io, checks which ones have been downloaded,
    /// and adds new files to the download queue.
    ///
    /// # Returns
    ///
    /// Statistics about the sync operation
    pub async fn sync_downloads(&self) -> DownloadResult<SyncStats> {
        let started = Instant::now();
        let started_at = chrono::Utc::now();
        let manual = self
            .manual_sync
            .swap(false, std::sync::atomic::Ordering::SeqCst);
        self.sync_status.write().await.begin();
        telemetry::sync::started();
        // Paired with the `sync.completed` below by `started_at` (feature 057)
        events::emit(Payload::SyncStarted(events::SyncStarted {
            trigger: if manual { "manual" } else { "schedule" }.to_string(),
            started_at,
        }));
        let result = self.sync_downloads_inner().await;
        self.sync_status.write().await.finish(match &result {
            Ok(stats) => SyncOutcome::from(stats),
            Err(e) => SyncOutcome::Failure {
                error: error_chain(e),
            },
        });
        let (outcome, stats) = match &result {
            Ok(stats) => (SyncResult::Success, stats.clone()),
            Err(e) => {
                telemetry::sync::failed(e.sync_fail_reason());
                (SyncResult::Failure, SyncStats::default())
            }
        };
        telemetry::sync::record_run(
            outcome,
            started.elapsed(),
            &[
                (SyncKind::Found, stats.total_files),
                (SyncKind::Queued, stats.files_downloaded),
                (SyncKind::Skipped, stats.files_skipped),
                (SyncKind::Blocked, stats.files_blocked),
                (SyncKind::Failed, stats.files_failed),
                (SyncKind::Streamed, stats.files_streamed),
            ],
        );
        events::emit(Payload::SyncCompleted(events::SyncCompleted {
            trigger: if manual { "manual" } else { "schedule" }.to_string(),
            result: if result.is_ok() { "success" } else { "failure" }.to_string(),
            error: result.as_ref().err().map(|e| redact::text(&error_chain(e))),
            started_at,
            duration_seconds: started.elapsed().as_secs_f64(),
            files_found: stats.total_files as u64,
            files_queued: stats.files_downloaded as u64,
            files_skipped: stats.files_skipped as u64,
            files_blocked: stats.files_blocked as u64,
            files_failed: stats.files_failed as u64,
        }));
        result
    }

    async fn sync_downloads_inner(&self) -> DownloadResult<SyncStats> {
        info!("Starting download sync...");
        self.requeue_transient().await;
        Ok(self.enqueue_tree_inner(0, false, true).await?.stats)
    }

    /// Queue files again whose last attempt failed on something that may have passed (a
    /// dropped link, a 5xx), at most `MAX_SYNC_REQUEUES` times each
    async fn requeue_transient(&self) {
        let Some(db) = self.database.as_ref() else {
            return;
        };
        let tracked: Vec<(i64, u32)> = locked(&self.transient_failures)
            .iter()
            .map(|(id, n)| (*id, *n))
            .collect();
        if tracked.is_empty() {
            return;
        }
        let failed: HashSet<i64> = match db.queue().failed().await {
            Ok(rows) => rows.into_iter().map(|r| r.file_id).collect(),
            Err(e) => {
                warn!(error = %e, "Could not read the failed files to queue them again");
                return;
            }
        };
        let mut retry = Vec::new();
        {
            let mut map = locked(&self.transient_failures);
            for (id, tries) in tracked {
                if !failed.contains(&id) {
                    // Gone, retried by hand or done: nothing to track
                    map.remove(&id);
                } else if tries < MAX_SYNC_REQUEUES {
                    map.insert(id, tries + 1);
                    retry.push(id);
                } else {
                    map.remove(&id);
                }
            }
        }
        if retry.is_empty() {
            return;
        }
        match db.queue().retry_files(&retry).await {
            Ok(_) => {
                info!(files = retry.len(), "Queued transiently failed files again");
                self.work_notify.notify_waiters();
            }
            Err(e) => warn!(error = %e, "Could not queue the failed files again"),
        }
    }

    /// Queue every file under a Put.io folder, or a single file, through the same
    /// filters and dedupe as the folder sync. `0` is the Put.io root, which is what the
    /// sync uses.
    ///
    /// Files keep their Put.io path relative to the root, so a folder queued from the UI
    /// lands where the sync would put it.
    pub async fn enqueue_tree(&self, root_id: i64) -> DownloadResult<SyncStats> {
        Ok(self.enqueue_tree_report(root_id, false).await?.stats)
    }

    /// [`Self::enqueue_tree`], also returning what happened to each file and the
    /// root's name on disk. The *arr download client (feature 011) uses it to know which
    /// files an item waits for, and passes `fixed_path`: its files then overwrite an
    /// existing file rather than get a numbered name (feature 040, R6).
    ///
    /// Stream mode (feature 022) is not honoured here: the *arr needs local files, and a
    /// folder queued from the UI was asked for. Only the sync pass keeps files on Put.io.
    pub async fn enqueue_tree_report(
        &self,
        root_id: i64,
        fixed_path: bool,
    ) -> DownloadResult<TreeReport> {
        self.enqueue_tree_inner(root_id, fixed_path, false).await
    }

    async fn enqueue_tree_inner(
        &self,
        root_id: i64,
        fixed_path: bool,
        honour_stream: bool,
    ) -> DownloadResult<TreeReport> {
        let mut stats = SyncStats::default();
        let mut report_files = Vec::new();

        // Require database for queue-based downloads
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for sync downloads".to_string())
        })?;

        // List the tree from Put.io (root folder 0 for the sync)
        let (files_with_paths, root_name) = match self.list_tree(root_id).await {
            Ok(listed) => listed,
            Err(e) => {
                error!(root_id, error = %error_chain(&e), "Failed to list files from Put.io");
                return Err(e);
            }
        };

        // Parent of every listed item, to match files to the transfer that made them
        let parents: HashMap<i64, Option<i64>> = files_with_paths
            .iter()
            .map(|(f, _)| (f.id, f.parent_id))
            .collect();
        // Fetched the first time a file is queued, so a sync with nothing new costs no call
        let mut transfers: Option<HashMap<i64, Transfer>> = None;

        // Filter to only actual files (not folders)
        let downloadable_files: Vec<_> = files_with_paths
            .into_iter()
            .filter(|(f, _)| f.file_type != "FOLDER")
            .collect();

        stats.total_files = downloadable_files.len();

        if stats.total_files == 0 {
            debug!("No files found on Put.io");
            return Ok(TreeReport {
                stats,
                files: report_files,
                root_name,
            });
        }

        info!("Found {} files on Put.io", stats.total_files);

        let downloads_repo = db.downloads();
        let queue_repo = db.queue();
        let config = self.config.read().await.clone();
        // Rules resolved once per folder
        let mut folders: HashMap<String, Effective> = HashMap::new();
        // Type-blocked files to delete from Put.io, in one call after the loop
        let mut to_delete = Vec::new();
        // Files stream mode keeps on Put.io, recorded after the loop; the ones already
        // recorded are read once so a pass over them costs no query each (SC-005)
        let mut streamed = Vec::new();
        let known_streamed = if honour_stream {
            db.stream_files().ids().await.unwrap_or_else(|e| {
                warn!(error = %e, "Failed to read streamed files");
                HashSet::new()
            })
        } else {
            HashSet::new()
        };
        // What is already downloaded or queued, read once: a pass over thousands of files
        // costs two queries, not two per file
        let completed_ids: HashSet<i64> = match downloads_repo.completed().await {
            Ok(rows) => rows.into_iter().map(|(id, _, _)| id).collect(),
            Err(e) => {
                warn!(error = %e, "Failed to read completed downloads");
                HashSet::new()
            }
        };
        let mut queued_ids: HashMap<i64, String> = match queue_repo.statuses().await {
            Ok(rows) => rows.into_iter().collect(),
            Err(e) => {
                warn!(error = %e, "Failed to read the queue; queueing relies on the insert's conflict handling");
                HashMap::new()
            }
        };
        // Transfers whose release check waits for Put.io (feature 042, research R8)
        let gated = self.gated_transfers(db).await;
        if !gated.is_empty() {
            transfers = Some(self.transfers_by_file().await);
        }

        // Asked after the built-in rules (feature 028); not for the *arr's files
        let filter_hooks = if fixed_path {
            None
        } else {
            self.hooks_for(Hook::Filter)
        };

        // Check which files need to be added to queue
        for (file, relative_path) in downloadable_files {
            let mut decide = |decision: TreeDecision| {
                report_files.push(TreeFile {
                    file_id: file.id,
                    name: file.name.clone(),
                    size: file.size,
                    relative_path: relative_path.clone(),
                    decision,
                });
            };
            if !gated.is_empty() {
                let transfer = transfers
                    .as_ref()
                    .and_then(|t| matching_transfer(file.id, &parents, t));
                if transfer.is_some_and(|t| gated.contains(&t.id)) {
                    debug!(
                        file_id = file.id,
                        "Skipping file until its release is checked"
                    );
                    stats.files_skipped += 1;
                    decide(TreeDecision::Known);
                    continue;
                }
            }
            let rules = folders
                .entry(relative_path.clone())
                .or_insert_with(|| config.effective(&relative_path));
            let mut decision = rules.decide(&file.name);
            // A file already downloaded or queued is skipped whatever a plugin says about
            // it, so the hook is only asked about candidates (one plugin round trip each)
            let known = completed_ids.contains(&file.id) || queued_ids.contains_key(&file.id);
            let ask_hook =
                !(known && matches!(decision, Decision::Download | Decision::Stream { .. }));
            if let Some(hooks) = filter_hooks.as_ref().filter(|_| ask_hook) {
                decision = filtered(hooks.as_ref(), &file, &relative_path, decision).await;
            }
            if let Decision::Stream { rule } = &decision {
                if honour_stream
                    && !known_streamed.contains(&file.id)
                    && (queued_ids.contains_key(&file.id) || completed_ids.contains(&file.id))
                {
                    // Queued or downloaded anyway (the *arr, or "Download" on the Put.io page)
                    stats.files_skipped += 1;
                    decide(TreeDecision::Known);
                    continue;
                }
                if honour_stream {
                    stats.files_streamed += 1;
                    decide(TreeDecision::Streamed);
                    streamed.push(ArrivedFile {
                        file_id: file.id,
                        name: file.name.clone(),
                        folder: relative_path.clone(),
                        size: file.size,
                    });
                    continue;
                }
                if fixed_path {
                    info!(
                        file_id = file.id,
                        name = %file.name,
                        rule = rule.as_deref().unwrap_or("global"),
                        "Stream mode ignored: Radarr/Sonarr need the file downloaded"
                    );
                }
                decision = Decision::Download;
            }
            if decision != Decision::Download {
                stats.files_blocked += 1;
                decide(match decision {
                    Decision::Skipped { .. } => TreeDecision::Skipped,
                    _ => TreeDecision::Blocked(extension(&file.name).unwrap_or_default()),
                });
                let by_plugin = matches!(
                    decision,
                    Decision::Blocked {
                        reason: Reason::Plugin { .. },
                        ..
                    }
                );
                if matches!(decision, Decision::Blocked { .. })
                    && !by_plugin
                    && rules.delete_blocked.value
                {
                    to_delete.push((
                        file.id,
                        file.name.clone(),
                        relative_path.clone(),
                        decision.clone(),
                    ));
                }
                self.note_decision(db, file.id, &file.name, &relative_path, &decision)
                    .await;
                continue;
            }

            // Check if already downloaded
            if completed_ids.contains(&file.id) {
                debug!(
                    "Skipping already downloaded file: {} (ID: {})",
                    file.name, file.id
                );
                stats.files_skipped += 1;
                decide(TreeDecision::Known);
                continue;
            }

            // Check if already in queue (any status): skip to avoid duplicates
            if let Some(status) = queued_ids.get(&file.id) {
                debug!(
                    "Skipping file already in queue: {} (ID: {}, status: {})",
                    file.name, file.id, status
                );
                stats.files_skipped += 1;
                decide(TreeDecision::Known);
                continue;
            }

            // Add to queue
            debug!(
                "Adding to queue: {} ({} bytes) at path '{}'",
                file.name, file.size, relative_path
            );

            let download_url = self.client.get_download_url(file.id);

            // The transfer is looked up before the insert: a file of a held transfer is
            // queued paused, so no worker can claim it in between (feature 039, R9)
            if transfers.is_none() {
                transfers = Some(self.transfers_by_file().await);
            }
            let transfer = matching_transfer(file.id, &parents, transfers.as_ref().unwrap());
            let held = match transfer {
                Some(t) => db.holds().is_held(t.id).await.unwrap_or_else(|e| {
                    warn!(transfer_id = t.id, error = %e, "Failed to check the transfer hold");
                    false
                }),
                None => false,
            };

            match queue_repo
                .add_with_path_kept(
                    file.id,
                    &file.name,
                    file.size,
                    &download_url,
                    &relative_path,
                    0,
                    if held {
                        QueueStatus::Paused
                    } else {
                        QueueStatus::Pending
                    },
                    fixed_path,
                )
                .await
            {
                Ok(_) => {
                    queued_ids.insert(file.id, "pending".to_string());
                    stats.files_downloaded += 1; // Renamed but means "added to queue"
                    decide(TreeDecision::Queued);
                    debug!("Added to queue: {}", file.name);
                    self.forget_decision(db, file.id).await;
                    let source = download_source(db, transfer, &relative_path).await;
                    if let Err(e) = db.sources().record(file.id, &source).await {
                        warn!(file_id = file.id, error = %e, "Failed to record download source");
                    }
                }
                Err(e) => {
                    stats.files_failed += 1;
                    decide(TreeDecision::Failed);
                    error!("Failed to add {} to queue: {}", file.name, e);
                }
            }
        }

        if stats.files_downloaded > 0 {
            self.work_notify.notify_waiters();
        }
        stats.files_deleted = self.delete_blocked(db, to_delete).await;
        if honour_stream {
            let steps: Vec<Arc<dyn ArrivedStep>> = self
                .arrived_steps
                .read()
                .unwrap()
                .iter()
                .filter(|s| s.enabled(&config))
                .cloned()
                .collect();
            self.note_streamed(db, &streamed, &known_streamed, &steps, root_id == 0)
                .await;
            arrived::run_pending(db, &steps, arrived::STEPS_PER_PASS).await;
        }

        let summary = format!(
            "Sync complete - Queued: {}, Skipped: {}, Blocked: {}, Deleted: {}, Streamed: {}, Failed: {}",
            stats.files_downloaded,
            stats.files_skipped,
            stats.files_blocked,
            stats.files_deleted,
            stats.files_streamed,
            stats.files_failed
        );
        // Info only when the pass changed something; an idle pass every interval is noise
        if stats.files_downloaded + stats.files_deleted + stats.files_streamed + stats.files_failed
            > 0
        {
            info!("{}", summary);
        } else {
            debug!("{}", summary);
        }

        Ok(TreeReport {
            stats,
            files: report_files,
            root_name,
        })
    }

    /// Record files stream mode keeps on Put.io the first time a pass sees them, with
    /// the arrived steps they need. After a pass over the whole tree (`full`), forget
    /// recorded files it no longer streams: deleted, moved out or downloaded now.
    async fn note_streamed(
        &self,
        db: &Database,
        streamed: &[ArrivedFile],
        known: &HashSet<i64>,
        steps: &[Arc<dyn ArrivedStep>],
        full: bool,
    ) {
        let repo = db.stream_files();
        let steps: Vec<&'static str> = steps.iter().map(|s| s.name()).collect();
        for file in streamed.iter().filter(|f| !known.contains(&f.file_id)) {
            self.forget_decision(db, file.file_id).await;
            telemetry::sync::rule_decision(RuleDecision::Streamed);
            info!(
                file_id = file.file_id,
                name = %file.name,
                folder = %file.folder,
                "Kept on Put.io (stream mode)"
            );
            if let Err(e) = repo
                .add(file.file_id, &file.name, &file.folder, file.size, &steps)
                .await
            {
                warn!(file_id = file.file_id, error = %e, "Failed to record streamed file");
            }
        }
        if full {
            let seen: HashSet<i64> = streamed.iter().map(|f| f.file_id).collect();
            let gone: Vec<i64> = known.difference(&seen).copied().collect();
            match repo.remove(&gone).await {
                Ok(0) => {}
                Ok(n) => debug!(count = n, "Forgot files no longer streamed"),
                Err(e) => warn!(error = %e, "Failed to forget files no longer streamed"),
            }
        }
    }

    /// Log, count and record a rule decision the first time this process sees the file
    async fn note_decision(
        &self,
        db: &Database,
        file_id: i64,
        name: &str,
        folder: &str,
        decision: &Decision,
    ) {
        let first_time = self
            .decided
            .lock()
            .map(|mut decided| decided.insert(file_id))
            .unwrap_or(true);
        if !first_time {
            return;
        }
        let kind = match decision {
            Decision::Skipped { .. } => RuleDecision::Skipped,
            _ => RuleDecision::Blocked,
        };
        let rule = decision.rule();
        let reason = decision.reason();
        telemetry::sync::file_blocked();
        telemetry::sync::rule_decision(kind);
        match kind {
            RuleDecision::Skipped => warn!(
                file_id,
                name = %name,
                folder = %folder,
                rule = rule.unwrap_or("global"),
                reason = %reason,
                "Skipped file not downloaded"
            ),
            _ => warn!(
                file_id,
                name = %name,
                folder = %folder,
                rule = rule.unwrap_or("global"),
                reason = %reason,
                "Blocked file not downloaded"
            ),
        }
        if let Err(e) = db
            .rule_decisions()
            .record(file_id, name, folder, kind, &reason, rule)
            .await
        {
            warn!(file_id, error = %e, "Failed to record rule decision");
        }
    }

    /// A file is queued after all: drop its decision so a later block is noted again
    async fn forget_decision(&self, db: &Database, file_id: i64) {
        let was_decided = self
            .decided
            .lock()
            .map(|mut decided| decided.remove(&file_id))
            .unwrap_or(true);
        if was_decided {
            if let Err(e) = db.rule_decisions().remove(file_id).await {
                warn!(file_id, error = %e, "Failed to remove rule decision");
            }
        }
    }

    /// Delete type-blocked files from Put.io in one call, leaving any that are queued.
    /// Returns how many were deleted.
    async fn delete_blocked(
        &self,
        db: &Database,
        candidates: Vec<(i64, String, String, Decision)>,
    ) -> usize {
        let mut files = Vec::new();
        for candidate in candidates {
            match db.queue().get_by_file_id(candidate.0).await {
                Ok(None) => files.push(candidate),
                Ok(Some(_)) => debug!(file_id = candidate.0, "Blocked file is queued; not deleted"),
                Err(e) => {
                    warn!(file_id = candidate.0, error = %e, "Queue check failed; not deleted")
                }
            }
        }
        if files.is_empty() {
            return 0;
        }
        let ids: Vec<i64> = files.iter().map(|f| f.0).collect();
        if let Err(e) = self.client.delete_files(ids).await {
            warn!(count = files.len(), error = %error_chain(&e), "Failed to delete blocked files from Put.io");
            return 0;
        }
        for (file_id, name, folder, decision) in &files {
            let rule = decision.rule();
            let reason = decision.reason();
            telemetry::sync::rule_decision(RuleDecision::Deleted);
            info!(
                file_id,
                name = %name,
                folder = %folder,
                rule = rule.unwrap_or("global"),
                reason = %reason,
                "Deleted blocked file from Put.io"
            );
            if let Err(e) = db
                .rule_decisions()
                .record(*file_id, name, folder, RuleDecision::Deleted, &reason, rule)
                .await
            {
                warn!(file_id, error = %e, "Failed to record rule decision");
            }
        }
        files.len()
    }

    /// What `config`'s rules would do under a Put.io folder (`0` = root) or with a single
    /// file. Lists the tree; downloads, deletes and records nothing.
    pub async fn preview_rules(
        &self,
        folder_id: i64,
        config: &DownloadConfig,
    ) -> DownloadResult<RulePreview> {
        let mut preview = RulePreview::default();
        let mut folders: HashMap<String, Effective> = HashMap::new();
        for (file, path) in self.list_tree(folder_id).await?.0 {
            if file.file_type == "FOLDER" {
                continue;
            }
            preview.files += 1;
            let rules = folders
                .entry(path.clone())
                .or_insert_with(|| config.effective(&path));
            let decision = rules.decide(&file.name);
            let would_delete = match &decision {
                Decision::Download => {
                    preview.download += 1;
                    if preview.downloads.len() < PREVIEW_DOWNLOADS {
                        preview.downloads.push(PreviewDownload {
                            file_id: file.id,
                            name: file.name,
                            path,
                        });
                    }
                    continue;
                }
                Decision::Skipped { .. } => {
                    preview.skipped += 1;
                    false
                }
                Decision::Stream { .. } => {
                    preview.stream += 1;
                    false
                }
                Decision::Blocked { .. } => {
                    preview.blocked += 1;
                    rules.delete_blocked.value && !self.is_queued(file.id).await
                }
            };
            if would_delete {
                preview.would_delete += 1;
            }
            if preview.items.len() < PREVIEW_ITEMS {
                preview.items.push(PreviewItem {
                    file_id: file.id,
                    name: file.name,
                    path,
                    decision,
                    would_delete,
                });
            } else {
                preview.truncated = true;
            }
        }
        Ok(preview)
    }

    async fn is_queued(&self, file_id: i64) -> bool {
        match &self.database {
            Some(db) => matches!(db.queue().get_by_file_id(file_id).await, Ok(Some(_))),
            None => false,
        }
    }

    /// Path of a Put.io folder relative to the root, as rules and downloads use it: `""`
    /// for the root (`0`). A file is refused.
    pub async fn putio_folder_path(&self, folder_id: i64) -> DownloadResult<String> {
        if folder_id == 0 {
            return Ok(String::new());
        }
        let folder = self
            .client
            .get_file_details(folder_id)
            .await
            .map_err(DownloadError::ClientError)?
            .file;
        if folder.file_type != "FOLDER" {
            return Err(DownloadError::InvalidInput(format!(
                "{} is a file, not a folder",
                folder.name
            )));
        }
        let parent = self.folder_path(folder.parent_id).await?;
        Ok(join_path(&parent, &sanitize_filename(&folder.name)))
    }

    /// Process next item from the download queue
    ///
    /// Gets the next pending item from queue, verifies it still exists on Put.io,
    /// downloads it, and updates the queue status.
    ///
    /// # Returns
    ///
    /// true if an item was processed, false if queue is empty
    pub async fn process_queue(&self) -> DownloadResult<bool> {
        // Paused by the schedule or a pause: claim nothing (running files wait in place)
        if self.bandwidth.is_paused() {
            return Ok(false);
        }
        // The Pause local downloads switch: claim nothing (feature 039, R11)
        if self.nas_paused().await {
            return Ok(false);
        }

        // A snapshot: no read lock is held while the file downloads, so a settings write
        // never waits on (and, with tokio's fair lock, never blocks) the other workers
        let config = self.config.read().await.clone();

        // Not enough free space: claim nothing until it is back
        if self.hold_for_space(&config, 0) {
            return Ok(false);
        }

        // Require database for queue-based downloads
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for queue processing".to_string())
        })?;

        let queue_repo = db.queue();

        // Room for a file: what is free, less the running downloads' unwritten bytes and
        // the floor. Rows bigger than that stay pending, so a big head row never keeps
        // smaller ones behind it from running.
        let budget = self.room_for_next(&config);

        // Atomically claim next pending item (prevents race conditions between workers)
        let queue_item = queue_repo
            .claim_next_pending_fitting(budget)
            .await
            .map_err(|e| {
                DownloadError::InternalError(format!("Failed to claim next pending item: {}", e))
            })?;

        let queue_item = match queue_item {
            Some(item) => item,
            None => {
                debug!("Nothing claimable");
                let waiting = budget != i64::MAX
                    && queue_repo
                        .counts_by_status()
                        .await
                        .map(|c| c.iter().any(|(s, n)| s == "pending" && *n > 0))
                        .unwrap_or(false);
                if waiting {
                    // Pending rows, none of which fits: hold, with the usual notice
                    self.hold_for_space(&config, 1 + budget.max(0) as u64);
                } else {
                    // Nothing is waiting for room any more
                    let mut state = locked(&self.low_space);
                    if state.is_some_and(|s| s.file_too_big) {
                        *state = None;
                    }
                }
                return Ok(false);
            }
        };

        let _busy = BusyGuard::new();
        let file_id = queue_item.file_id;
        let _stop = self.register_stop(file_id);
        // A pause between the claim and the registration above found no signal to stop
        if queue_repo
            .get_by_file_id(file_id)
            .await
            .ok()
            .flatten()
            .is_some_and(|row| row.status != "downloading")
        {
            debug!(file_id, "Claimed row was paused before it started");
            return Ok(true);
        }
        // The switch went on between the check above and the claim
        if self.nas_paused().await {
            self.unclaim(&[file_id]).await;
            return Ok(false);
        }

        // Reserve the file's bytes, checking free space under the reservation lock: another
        // worker may have taken the room since the claim. Back it goes, without waking
        // the workers (the next idle poll looks again).
        let size = queue_item.file_size.max(0) as u64;
        let _space = match self.reserve_space(&config, size) {
            Some(guard) => guard,
            None => {
                self.hold_for_space(&config, size);
                if let Err(e) = queue_repo.unclaim_files(&[file_id]).await {
                    warn!(file_id, error = %e, "Could not put the claimed file back in the queue");
                }
                return Ok(false);
            }
        };
        // A file that fits is running: the "file too big" notice is over
        {
            let mut state = locked(&self.low_space);
            if state.is_some_and(|s| s.file_too_big) {
                *state = None;
            }
        }

        debug!(
            file_id,
            name = %queue_item.filename,
            "Processing queue item"
        );

        // A panic in the download must not leave the row stuck on "downloading" or end
        // the worker: put the row back and report an error, the worker backs off
        let ran = std::panic::AssertUnwindSafe(self.run_claimed(queue_item, &config))
            .catch_unwind()
            .await;
        match ran {
            Ok(done) => done,
            Err(_) => {
                error!(
                    file_id,
                    "Download task panicked; the file goes back in the queue"
                );
                self.unclaim(&[file_id]).await;
                Err(DownloadError::InternalError(format!(
                    "download of file {} panicked",
                    file_id
                )))
            }
        }
    }

    /// Free bytes of the download folder's volume, `None` if unreadable (see [`free_bytes_of`])
    fn free_bytes(&self, dir: &Path) -> Option<u64> {
        free_bytes_of(dir, &self.space_warned)
    }

    /// Bytes the next file may take: free space less the reserved bytes and the floor.
    /// `i64::MAX` when the floor is off.
    fn room_for_next(&self, config: &DownloadConfig) -> i64 {
        let min = config.min_free_space_bytes;
        if min == 0 {
            return i64::MAX;
        }
        let free = self.free_bytes(&config.target_directory).unwrap_or(0);
        let room = free
            .saturating_sub(self.space.reserved())
            .saturating_sub(min);
        i64::try_from(room).unwrap_or(i64::MAX - 1)
    }

    /// Reserve `size` bytes for a download about to start, or `None` if they don't fit
    /// above the floor. With the floor off the reservation always succeeds.
    fn reserve_space(&self, config: &DownloadConfig, size: u64) -> Option<SpaceGuard> {
        let min = config.min_free_space_bytes;
        let free = if min == 0 {
            0
        } else {
            self.free_bytes(&config.target_directory)?
        };
        self.space.try_reserve(free, min, size)
    }

    /// Whether a download of `next_size` bytes must wait for free space. Records the state
    /// for `low_space`, warns once when it begins and logs when it ends. With the floor
    /// on, a folder whose space can't be read holds everything back (fail closed).
    fn hold_for_space(&self, config: &DownloadConfig, next_size: u64) -> bool {
        let min = config.min_free_space_bytes;
        let free = if min == 0 {
            None
        } else {
            self.free_bytes(&config.target_directory)
        };
        let reserved = self.space.reserved();
        let unreadable = min > 0 && free.is_none();
        let hold = unreadable
            || free.is_some_and(|free| {
                below_free_space_floor(free.saturating_sub(reserved), min, next_size)
            });
        let mut state = locked(&self.low_space);
        match (hold, free, state.is_some()) {
            (true, Some(free), was_held) => {
                if !was_held {
                    warn!(
                        free_bytes = free,
                        min_free_bytes = min,
                        "Low disk space: new downloads are on hold until free space is back"
                    );
                }
                *state = Some(LowSpace {
                    free_bytes: free,
                    min_free_bytes: min,
                    file_too_big: next_size > 0
                        && !below_free_space_floor(free.saturating_sub(reserved), min, 0),
                });
            }
            // The pre-claim check passing doesn't clear a state a too-big file set
            (false, _, true) if next_size == 0 && state.is_some_and(|s| s.file_too_big) => {}
            (false, _, true) => {
                info!("Enough free space again: downloads resume");
                *state = None;
            }
            _ => {}
        }
        hold
    }

    /// Set while new downloads are held back for lack of free space (`[download]
    /// min_free_space_mb`)
    pub fn low_space(&self) -> Option<LowSpace> {
        *locked(&self.low_space)
    }

    /// Remember what failed for `file_id`, for its row's reason line (feature 066)
    fn note_failure(&self, file_id: i64, error: &DownloadError) {
        let kind = if error.is_storage_full() {
            "disk_full"
        } else {
            error.fail_reason().as_str()
        };
        locked(&self.failures).insert(file_id, kind);
    }

    /// What the workers check before they claim a row, as of now: the input of the
    /// queue's and the Active page's reason lines (feature 066)
    pub async fn queue_state(&self) -> crate::waiting::QueueState {
        let (min_free_bytes, workers, target) = {
            let config = self.config.read().await;
            (
                config.min_free_space_bytes,
                config.workers,
                config.target_directory.clone(),
            )
        };
        let running = match self.database.as_ref() {
            Some(db) => db
                .queue()
                .counts_by_status()
                .await
                .map(|counts| {
                    counts
                        .iter()
                        .filter(|(status, _)| status == "downloading")
                        .map(|(_, n)| usize::try_from(*n).unwrap_or(0))
                        .sum()
                })
                .unwrap_or(0),
            None => 0,
        };
        crate::waiting::QueueState {
            bandwidth: crate::waiting::bandwidth_hold(&self.bandwidth.status()),
            nas_paused: self.nas_paused().await,
            min_free_bytes,
            free_bytes: (min_free_bytes > 0)
                .then(|| self.free_bytes(&target))
                .flatten(),
            reserved_bytes: self.space.reserved(),
            workers,
            running,
            failures: locked(&self.failures).clone(),
        }
    }

    /// Put claimed rows back to pending, logging a failure: a row that stays on
    /// "downloading" is only found again by the next restart
    async fn unclaim(&self, file_ids: &[i64]) {
        let Some(db) = self.database.as_ref() else {
            return;
        };
        if let Err(e) = db.queue().unclaim_files(file_ids).await {
            warn!(?file_ids, error = %e, "Could not put the claimed files back in the queue");
        }
        self.work_notify.notify_waiters();
    }

    /// Download a row `process_queue` claimed, and settle the row
    async fn run_claimed(
        &self,
        queue_item: putio_sync_db::models::QueueItem,
        config: &DownloadConfig,
    ) -> DownloadResult<bool> {
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for queue processing".to_string())
        })?;
        let queue_repo = db.queue();
        let file_id = queue_item.file_id;

        // Verify file still exists on Put.io and get current info
        match self.client.get_file_details(file_id).await {
            Ok(file_response) => {
                let file_info = file_response.file;
                debug!(
                    file_id,
                    name = %file_info.name,
                    size_bytes = file_info.size,
                    "File verified on Put.io"
                );

                // Download the file using the stored relative path
                let download_file = DownloadFile {
                    file_id,
                    filename: file_info.name.clone(),
                    relative_path: queue_item.relative_path.clone(),
                    file_size: file_info.size as u64,
                    download_url: self.client.get_download_url(file_id),
                    crc32: file_info.crc32.clone(),
                    save_as: queue_item.save_as.clone(),
                    fixed_path: queue_item.fixed_path,
                    overwrite: queue_item.redownload,
                };

                match self.download_file(download_file, None).await {
                    Ok(result) if result.success => {
                        info!(
                            file_id,
                            name = %file_info.name,
                            size_bytes = file_info.size,
                            "Downloaded"
                        );

                        // Delete from Put.io if configured
                        if config
                            .effective(&queue_item.relative_path)
                            .delete_after_download
                            .value
                        {
                            let gate = read_locked(&self.delete_gate).clone();
                            let deferred = match gate {
                                Some(g) => {
                                    g.defer(file_id, &queue_item.relative_path, &file_info.name)
                                        .await
                                }
                                None => None,
                            };
                            if let Some(item_key) = deferred {
                                let now = SystemTime::now()
                                    .duration_since(SystemTime::UNIX_EPOCH)
                                    .map(|d| d.as_secs() as i64)
                                    .unwrap_or(0);
                                match db.deferred_deletes().insert(file_id, &item_key, now).await {
                                    Ok(()) => info!(
                                        file_id, item = %item_key,
                                        "Put.io delete waits for the subtitles stage"
                                    ),
                                    Err(e) => warn!(
                                        file_id, error = %e,
                                        "Could not defer the Put.io delete; the file stays on Put.io"
                                    ),
                                }
                            } else {
                                debug!(
                                    file_id,
                                    name = %file_info.name,
                                    "Deleting file from Put.io"
                                );
                                match self.client.delete_files(vec![file_id]).await {
                                    Ok(_) => {
                                        debug!(
                                            file_id,
                                            name = %file_info.name,
                                            "Deleted from Put.io"
                                        );
                                        cleanup_deleted(&[file_id]);
                                    }
                                    Err(e) => {
                                        warn!(
                                            "Failed to delete file {} from Put.io: {}",
                                            file_info.name, e
                                        );
                                    }
                                }
                            }
                        }

                        // Update queue status to completed and remove
                        if let Err(e) = queue_repo
                            .update_status(file_id, QueueStatus::Completed)
                            .await
                        {
                            warn!("Failed to update queue status to completed: {}", e);
                        }

                        if let Err(e) = queue_repo.remove(file_id).await {
                            warn!("Failed to remove completed item from queue: {}", e);
                        }
                        locked(&self.transient_failures).remove(&file_id);

                        Ok(true)
                    }
                    // Stopped on request: keep the partial and the row as the caller set it
                    Err(DownloadError::Stopped) => Ok(true),
                    outcome => {
                        error!("Failed to download: {}", file_info.name);
                        // Retries used up on a failure that may pass (a dropped link,
                        // a 5xx): the next syncs queue the row again, a few times
                        let transient = match &outcome {
                            Ok(result) => result.transient,
                            Err(e) => e.is_transient(),
                        };
                        if transient {
                            locked(&self.transient_failures).entry(file_id).or_insert(0);
                        } else {
                            locked(&self.transient_failures).remove(&file_id);
                        }

                        // Update queue status to failed
                        if let Err(e) = queue_repo.update_status(file_id, QueueStatus::Failed).await
                        {
                            warn!("Failed to update queue status to failed: {}", e);
                        }

                        Ok(true) // Still processed, even if failed
                    }
                }
            }
            // Put.io says the file is gone: drop the row
            Err(e) if e.is_not_found() => {
                warn!("File {} no longer exists on Put.io: {}", file_id, e);

                if let Err(e) = queue_repo.remove(file_id).await {
                    warn!("Failed to remove stale item from queue: {}", e);
                }

                Ok(true) // Processed (removed stale item)
            }
            // Anything else (a dropped link, a 5xx, a bad token) says nothing about the
            // file: keep the row, and let the worker back off before it tries again
            Err(e) => {
                warn!(
                    file_id,
                    "Could not check file on Put.io, keeping it queued: {}", e
                );
                self.unclaim(&[file_id]).await;
                Err(DownloadError::ClientError(e))
            }
        }
    }

    /// Run a sync loop that periodically refreshes the queue from Put.io
    ///
    /// This should be run in a separate task and will continuously sync
    /// the file list from Put.io to the download queue.
    ///
    /// # Arguments
    ///
    /// * `interval_secs` - How often to sync in seconds
    pub async fn run_sync_loop(&self, interval_secs: u64) {
        // The sender stays here, so the interval never changes
        let (tx, rx) = tokio::sync::watch::channel(interval_secs);
        let _keep = tx;
        self.run_sync_loop_live(rx).await;
    }

    /// [`run_sync_loop`](Self::run_sync_loop) with an interval that can change while it
    /// runs: each wait uses the latest value, and a change during a wait restarts the
    /// count against the new interval (measured from the start of the wait, so a shorter
    /// interval can end the wait at once). Dropping the sender freezes the interval.
    pub async fn run_sync_loop_live(&self, mut interval_secs: tokio::sync::watch::Receiver<u64>) {
        let secs = |rx: &mut tokio::sync::watch::Receiver<u64>| (*rx.borrow_and_update()).max(1);
        let first = secs(&mut interval_secs);
        info!("Starting Put.io sync loop with interval: {}s", first);
        self.sync_status
            .write()
            .await
            .schedule(std::time::Duration::from_secs(first));
        let mut cycle = 0;
        let mut sender_gone = false;

        loop {
            cycle += 1;
            debug!("Sync loop cycle #{}", cycle);

            match self.sync_downloads().await {
                Ok(stats) => {
                    if stats.files_downloaded > 0 {
                        info!(
                            "Sync completed - Queued: {}, Skipped: {}, Blocked: {}, Failed: {}",
                            stats.files_downloaded,
                            stats.files_skipped,
                            stats.files_blocked,
                            stats.files_failed
                        );
                    } else {
                        debug!("Sync completed - no new files");
                    }
                }
                Err(e) => {
                    error!(error = %error_chain(&e), "Error syncing from Put.io");
                }
            }

            // "Sync now" wakes the loop early; request_sync refuses while a sync runs,
            // so no wake-up is stored during one
            let started = tokio::time::Instant::now();
            loop {
                let wait = std::time::Duration::from_secs(secs(&mut interval_secs));
                self.sync_status.write().await.schedule(wait);
                tokio::select! {
                    _ = tokio::time::sleep_until(started + wait) => break,
                    _ = self.sync_notify.notified() => {
                        debug!("Sync requested");
                        break;
                    }
                    changed = async {
                        if sender_gone {
                            std::future::pending().await
                        } else {
                            interval_secs.changed().await
                        }
                    } => {
                        match changed {
                            Ok(()) => info!(
                                "Put.io sync interval is now {}s",
                                (*interval_secs.borrow()).max(1)
                            ),
                            Err(_) => sender_gone = true,
                        }
                    }
                }
            }
        }
    }

    /// Start a folder sync now ("sync now").
    ///
    /// Refused while a sync runs. Wakes `run_sync_loop` when one is running, otherwise
    /// runs the sync on a spawned task. Marks the sync in progress before returning, so
    /// a second request is refused straight away.
    pub async fn request_sync(&self) -> Result<(), SyncRequestError> {
        let has_loop = {
            let mut status = self.sync_status.write().await;
            if status.in_progress {
                return Err(SyncRequestError::InProgress);
            }
            status.begin();
            status.interval.is_some()
        };
        self.manual_sync
            .store(true, std::sync::atomic::Ordering::SeqCst);

        if has_loop {
            self.sync_notify.notify_one();
        } else {
            let manager = self.clone();
            tokio::spawn(async move {
                if let Err(e) = manager.sync_downloads().await {
                    error!(error = %error_chain(&e), "Error syncing from Put.io");
                }
            });
        }
        Ok(())
    }

    /// Current state of the folder sync
    pub async fn sync_status(&self) -> SyncStatus {
        self.sync_status.read().await.clone()
    }

    /// Run download worker loop
    ///
    /// Spawns multiple concurrent workers to process the queue.
    /// Does NOT sync from Put.io - that should be handled by run_sync_loop.
    ///
    /// # Returns
    ///
    /// Number of items processed
    pub async fn run_worker_cycle(&self) -> DownloadResult<usize> {
        // Check if there are pending items in queue
        if let Some(db) = &self.database {
            let queue_repo = db.queue();
            let pending_items = queue_repo.get_pending().await.map_err(|e| {
                DownloadError::InternalError(format!("Failed to check queue: {}", e))
            })?;

            if pending_items.is_empty() {
                debug!("Queue is empty, nothing to process");
                return Ok(0);
            }
        }

        // Spawn concurrent workers - read config once for worker count
        let num_workers = {
            let config = self.config.read().await;
            config.workers
        };
        info!("Starting {} concurrent download workers", num_workers);

        let mut handles = Vec::new();

        for worker_id in 0..num_workers {
            // Clone the entire DownloadManager (shares the same Arc<RwLock<Config>>)
            let worker_dm = self.clone();

            let handle = tokio::spawn(async move {
                let mut count = 0;

                debug!("Worker {} starting", worker_id);

                // Process items until queue is empty
                loop {
                    match WORKER_ID.scope(worker_id, worker_dm.process_queue()).await {
                        Ok(true) => {
                            count += 1;
                        }
                        Ok(false) => {
                            // Queue is empty
                            debug!("Worker {} finished - queue empty", worker_id);
                            break;
                        }
                        Err(e) => {
                            error!("Worker {} error: {}", worker_id, e);
                            break;
                        }
                    }
                }

                debug!("Worker {} processed {} items", worker_id, count);
                count
            });

            handles.push(handle);
        }

        // Wait for all workers to complete
        let mut total_processed = 0;
        for handle in handles {
            match handle.await {
                Ok(count) => total_processed += count,
                Err(e) => error!("Worker task failed: {}", e),
            }
        }

        if total_processed > 0 {
            info!(
                "Worker cycle completed - {} workers processed {} items total",
                num_workers, total_processed
            );
        }

        Ok(total_processed)
    }

    /// Put.io transfers keyed by the file or folder they produced (feature 006, R4).
    /// A failed listing leaves every source unknown rather than failing the sync.
    /// Transfer ids with a pending release check; one cheap query when there are none
    async fn gated_transfers(&self, db: &Database) -> HashSet<i64> {
        let releases = db.releases();
        match releases.has_pending().await {
            Ok(false) => HashSet::new(),
            Ok(true) => match releases.pending_transfer_ids().await {
                Ok(ids) => ids.into_iter().collect(),
                Err(e) => {
                    warn!(error = %e, "Failed to list pending release checks");
                    HashSet::new()
                }
            },
            Err(e) => {
                warn!(error = %e, "Failed to look up pending release checks");
                HashSet::new()
            }
        }
    }

    async fn transfers_by_file(&self) -> HashMap<i64, Transfer> {
        match self.client.list_transfers().await {
            Ok(response) => response
                .transfers
                .into_iter()
                .filter_map(|t| Some((t.file_id?, t)))
                .collect(),
            Err(e) => {
                warn!(error = %error_chain(&e), "Failed to list transfers; download sources unknown");
                HashMap::new()
            }
        }
    }

    /// List `root_id` for `enqueue_tree`: everything under a folder, or a one-element
    /// list for a file. Paths are relative to the Put.io root.
    async fn list_tree(
        &self,
        root_id: i64,
    ) -> DownloadResult<(Vec<(FileEntry, String)>, Option<String>)> {
        if root_id == 0 {
            return Ok((self.list_all_files(0, String::new()).await?, None));
        }

        let root = self
            .client
            .get_file_details(root_id)
            .await
            .map_err(DownloadError::ClientError)?
            .file;
        let parent_path = self.folder_path(root.parent_id).await?;
        let root_name = Some(sanitize_filename(&root.name));
        if root.file_type == "FOLDER" {
            let path = join_path(&parent_path, &sanitize_filename(&root.name));
            Ok((self.list_all_files(root.id, path).await?, root_name))
        } else {
            Ok((vec![(root, parent_path)], root_name))
        }
    }

    /// Relative path of a Put.io folder from the root, built the way `list_all_files`
    /// builds it while descending
    async fn folder_path(&self, folder_id: Option<i64>) -> DownloadResult<String> {
        // Guards against a parent cycle in a malformed response
        const MAX_DEPTH: usize = 64;
        let mut names = Vec::new();
        let mut current = folder_id.unwrap_or(0);
        while current != 0 && names.len() < MAX_DEPTH {
            let folder = self
                .client
                .get_file_details(current)
                .await
                .map_err(DownloadError::ClientError)?
                .file;
            names.push(sanitize_filename(&folder.name));
            current = folder.parent_id.unwrap_or(0);
        }
        names.reverse();
        Ok(names.join("/"))
    }

    /// List all files recursively from Put.io, up to [`LIST_CONCURRENCY`] folders at a time
    /// Returns a list of (FileEntry, relative_path) tuples, with paths under `base_path`
    async fn list_all_files(
        &self,
        parent_id: i64,
        base_path: String,
    ) -> DownloadResult<Vec<(FileEntry, String)>> {
        use futures_util::stream::{FuturesUnordered, StreamExt};

        // List the folders a few at a time: one round trip each adds up over a big library
        let mut listings: HashMap<i64, Vec<FileEntry>> = HashMap::new();
        let mut pending = std::collections::VecDeque::from([parent_id]);
        let mut seen = HashSet::from([parent_id]);
        let mut in_flight = FuturesUnordered::new();
        loop {
            while in_flight.len() < LIST_CONCURRENCY {
                let Some(id) = pending.pop_front() else {
                    break;
                };
                debug!("Fetching files from folder: {}", id);
                let client = &self.client;
                in_flight.push(async move { (id, client.list_files(id).await) });
            }
            // An error drops the listings still in flight
            let Some((id, response)) = in_flight.next().await else {
                break;
            };
            let files = response.map_err(DownloadError::ClientError)?.files;
            for file in &files {
                if file.file_type == "FOLDER" && seen.insert(file.id) {
                    pending.push_back(file.id);
                }
            }
            listings.insert(id, files);
        }

        // Assemble in the order a one-at-a-time walk gives, whichever listing came first:
        // a folder's files, then its last subfolder's tree first
        let mut all_files = Vec::new();
        let mut folders_to_process = vec![(parent_id, base_path)];
        while let Some((current_folder, current_path)) = folders_to_process.pop() {
            // Taken out of the map, so a folder listed twice in a malformed tree is not
            // walked twice
            let Some(files) = listings.remove(&current_folder) else {
                continue;
            };
            for file in files {
                if file.file_type == "FOLDER" {
                    // Build the folder's relative path
                    let folder_path = join_path(&current_path, &sanitize_filename(&file.name));

                    folders_to_process.push((file.id, folder_path));
                }
                // Store file with its relative path
                all_files.push((file, current_path.clone()));
            }
        }

        debug!("Found {} total items on Put.io", all_files.len());
        Ok(all_files)
    }

    /// Get overall progress
    pub async fn overall_progress(&self) -> OverallProgress {
        self.overall_progress.read().await.clone()
    }

    /// Get list of active downloads with progress information
    ///
    /// Returns a list of currently downloading files with their progress stats.
    /// Requires database to query download queue for file information.
    pub async fn get_active_downloads(&self) -> DownloadResult<Vec<ActiveDownload>> {
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for active downloads".to_string())
        })?;

        let queue_repo = db.queue();

        // Only the rows being downloaded: the page polls this, and a queue of thousands
        // must not be read each time
        let downloading_items = queue_repo.get_downloading().await.map_err(|e| {
            DownloadError::InternalError(format!("Failed to get queue items: {}", e))
        })?;

        let progress_trackers = self.progress_trackers.read().await;
        let mut active_downloads = Vec::new();

        let checking = self
            .checking
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        for item in downloading_items {
            // A checksum read shows its own progress (R4)
            if let Some((read, started, phase)) = checking.get(&item.file_id) {
                let elapsed = started.elapsed();
                let speed_bps = if elapsed.as_secs_f64() > 0.0 {
                    *read as f64 / elapsed.as_secs_f64()
                } else {
                    0.0
                };
                let size = item.file_size as u64;
                active_downloads.push(ActiveDownload {
                    file_id: item.file_id,
                    filename: item.filename,
                    file_size: size,
                    bytes_downloaded: *read,
                    percentage: if size > 0 {
                        (*read as f64 / size as f64) * 100.0
                    } else {
                        0.0
                    },
                    speed_bps,
                    eta_seconds: (speed_bps > 0.0)
                        .then(|| (size.saturating_sub(*read) as f64 / speed_bps) as u64),
                    elapsed_seconds: elapsed.as_secs(),
                    phase,
                });
                continue;
            }
            // Get progress tracker if exists
            if let Some(tracker) = progress_trackers.get(&item.file_id) {
                let (avg_speed, elapsed) = tracker.overall_stats();

                // For bytes_downloaded, we would need to track this separately
                // For now, we'll use a placeholder based on elapsed time and speed
                let bytes_downloaded = (avg_speed * elapsed.as_secs_f64()) as u64;
                let bytes_downloaded = bytes_downloaded.min(item.file_size as u64);

                let percentage = if item.file_size > 0 {
                    (bytes_downloaded as f64 / item.file_size as f64) * 100.0
                } else {
                    0.0
                };

                let eta_seconds = if avg_speed > 0.0 {
                    let remaining = (item.file_size as u64).saturating_sub(bytes_downloaded) as f64;
                    Some((remaining / avg_speed) as u64)
                } else {
                    None
                };

                active_downloads.push(ActiveDownload {
                    file_id: item.file_id,
                    filename: item.filename,
                    file_size: item.file_size as u64,
                    bytes_downloaded,
                    percentage,
                    speed_bps: avg_speed,
                    eta_seconds,
                    elapsed_seconds: elapsed.as_secs(),
                    phase: "downloading",
                });
            }
        }

        Ok(active_downloads)
    }

    /// Get progress for a specific download by file_id
    pub async fn get_download_progress(
        &self,
        file_id: i64,
    ) -> DownloadResult<Option<ActiveDownload>> {
        let active_downloads = self.get_active_downloads().await?;
        Ok(active_downloads.into_iter().find(|d| d.file_id == file_id))
    }

    /// Cancel an active download
    ///
    /// Updates the queue item status to 'pending' so it won't be processed by workers.
    /// Note: This doesn't forcefully stop an in-progress download, but marks it to not
    /// be retried. The current download will complete or fail naturally.
    pub async fn cancel_download(&self, file_id: i64) -> DownloadResult<()> {
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for cancel operation".to_string())
        })?;

        let queue_repo = db.queue();

        // Update status to pending (effectively pausing it)
        queue_repo
            .update_status(file_id, QueueStatus::Pending)
            .await
            .map_err(|e| {
                DownloadError::InternalError(format!("Failed to cancel download: {}", e))
            })?;

        // Remove from progress trackers
        self.progress_trackers.write().await.remove(&file_id);

        info!("Download cancelled for file_id: {}", file_id);
        Ok(())
    }

    /// Stop the running downloads of `file_ids` (feature 039, R7)
    ///
    /// Set each queue row's status first (`paused`, or `pending` under the switch): a
    /// stopped file keeps its partial and its row is left as it is. Returns the ids that
    /// were running.
    pub async fn stop_files(&self, file_ids: &[i64]) -> Vec<i64> {
        let signals = locked(&self.stop_signals);
        file_ids
            .iter()
            .filter_map(|id| {
                signals.get(id).map(|signal| {
                    signal.stop();
                    *id
                })
            })
            .collect()
    }

    /// The Put.io transfer list, fetched at most once per `max_age` (feature 039, R3).
    /// Concurrent callers wait for the same fetch. When Put.io fails, the last list is
    /// returned flagged stale, and the next try waits `max_age` too; with no list yet,
    /// the error is returned.
    pub async fn transfers_snapshot(&self, max_age: Duration) -> DownloadResult<TransfersSnapshot> {
        let fresh = |me: &Self| {
            locked(&me.transfers_cache)
                .as_ref()
                .filter(|c| c.attempted.elapsed() < max_age)
                .map(CachedTransfers::snapshot)
        };
        if let Some(snapshot) = fresh(self) {
            return Ok(snapshot);
        }
        // One refresh at a time. While another caller fetches, a caller with an older
        // list uses it; one with none waits for the fetch.
        let _refreshing = match self.transfers_refresh.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                if let Some(cached) = locked(&self.transfers_cache).as_ref() {
                    return Ok(cached.snapshot());
                }
                let guard = self.transfers_refresh.lock().await;
                if let Some(snapshot) = fresh(self) {
                    return Ok(snapshot);
                }
                guard
            }
        };
        // Another caller may have refreshed between the check and the lock
        if let Some(snapshot) = fresh(self) {
            return Ok(snapshot);
        }
        match self.client.list_transfers().await {
            Ok(response) => {
                let snapshot = {
                    let mut cache = locked(&self.transfers_cache);
                    cache
                        .insert(CachedTransfers {
                            attempted: Instant::now(),
                            fetched_at: SystemTime::now(),
                            transfers: Arc::new(response.transfers),
                            stale: false,
                        })
                        .snapshot()
                };
                self.track_transfers(&snapshot.transfers).await;
                Ok(snapshot)
            }
            Err(e) => match locked(&self.transfers_cache).as_mut() {
                Some(cached) => {
                    warn!(error = %error_chain(&e), "Failed to list transfers; using the last list");
                    cached.attempted = Instant::now();
                    cached.stale = true;
                    Ok(cached.snapshot())
                }
                None => Err(e.into()),
            },
        }
    }

    /// The last Put.io transfer list fetched, without asking Put.io
    pub(crate) async fn cached_transfers(&self) -> Option<Arc<Vec<Transfer>>> {
        locked(&self.transfers_cache)
            .as_ref()
            .map(|c| c.transfers.clone())
    }

    /// Compare a fresh transfer list with the last one and emit the `transfer.*` events
    /// due (feature 027, R6). A database error is logged and sends nothing.
    async fn track_transfers(&self, transfers: &[Transfer]) {
        let Some(db) = self.database.as_ref() else {
            return;
        };
        let mut guard = self.transfer_events.lock().await;
        if guard.is_none() {
            match db.flags().get_text(TRANSFER_EVENTS_SEEN).await {
                Ok(stored) => *guard = Some(TransferEvents::from_stored(stored.as_deref())),
                Err(e) => {
                    warn!(error = %e, "Failed to read the transfers seen for events");
                    return;
                }
            }
        }
        let Some(tracker) = guard.as_mut() else {
            return;
        };
        let list: Vec<_> = transfers
            .iter()
            .map(|t| (t.id, TransferClass::of(&t.status)))
            .collect();
        let (changes, changed) = tracker.observe(&list);
        if changed {
            if let Err(e) = db
                .flags()
                .set_text(TRANSFER_EVENTS_SEEN, &tracker.to_stored())
                .await
            {
                warn!(error = %e, "Failed to store the transfers seen for events");
            }
        }
        if changes.is_empty() {
            return;
        }
        let mut ids: Vec<i64> = changes.iter().map(|(id, _)| *id).collect();
        ids.dedup();
        let labels = db.transfer_origins(&ids).await.unwrap_or_else(|e| {
            warn!(error = %e, "Failed to read transfer origins for events");
            HashMap::new()
        });
        let by_id: HashMap<i64, &Transfer> = transfers.iter().map(|t| (t.id, t)).collect();
        for (id, change) in changes {
            let Some(t) = by_id.get(&id) else {
                continue;
            };
            let data = events::TransferChanged {
                transfer_id: id,
                name: t.name.clone(),
                origin: transfer_events::origin(labels.get(&id).map(String::as_str)),
                status: t.status.clone(),
                size_bytes: t.size,
                file_id: t.file_id,
                error: t.error_message.clone().filter(|e| !e.is_empty()),
            };
            events::emit(match change {
                TransferChange::Added => Payload::TransferAdded(data),
                TransferChange::Finished => Payload::TransferFinished(data),
                TransferChange::Failed => Payload::TransferFailed(data),
            });
        }
    }

    /// The Active transfers pipeline (feature 039): Put.io's transfer list, at most
    /// `max_age` old, joined with the queue, holds, live progress, stalls and origins.
    /// `retention_hours` is how far back done files are read.
    pub async fn pipeline(
        &self,
        max_age: Duration,
        stalls: &StallTracker,
        retention_hours: u64,
    ) -> DownloadResult<crate::pipeline::Pipeline> {
        let snapshot = self.transfers_snapshot(max_age).await?;
        self.pipeline_from(Some(snapshot), stalls, retention_hours)
            .await
    }

    /// The pipeline from the last Put.io list fetched, without asking Put.io; with no
    /// list yet, NAS items only (the Status card, FR-030)
    pub async fn pipeline_cached(
        &self,
        stalls: &StallTracker,
        retention_hours: u64,
    ) -> DownloadResult<crate::pipeline::Pipeline> {
        let snapshot = locked(&self.transfers_cache).as_ref().map(|c| c.snapshot());
        self.pipeline_from(snapshot, stalls, retention_hours).await
    }

    async fn pipeline_from(
        &self,
        snapshot: Option<TransfersSnapshot>,
        stalls: &StallTracker,
        retention_hours: u64,
    ) -> DownloadResult<crate::pipeline::Pipeline> {
        use crate::pipeline::{build, BuildInput, FileProgress, Origin};

        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for the pipeline".to_string())
        })?;
        let transfers = snapshot
            .as_ref()
            .map(|s| s.transfers.to_vec())
            .unwrap_or_default();
        let transfer_ids: Vec<i64> = transfers.iter().map(|t| t.id).collect();
        let files = db
            .pipeline_files(&transfer_ids, retention_hours)
            .await
            .map_err(internal)?;
        let holds = db.holds().list().await.map_err(internal)?;
        // Origins of the listed transfers and of transfers gone from Put.io
        let mut origin_ids = transfer_ids.clone();
        origin_ids.extend(files.iter().filter_map(|f| f.transfer_id));
        origin_ids.sort_unstable();
        origin_ids.dedup();
        let transfer_origins = db
            .transfer_origins(&origin_ids)
            .await
            .map_err(internal)?
            .into_iter()
            .filter_map(|(id, label)| Origin::from_db(&label).map(|o| (id, o)))
            .collect();
        let progress = self
            .get_active_downloads()
            .await?
            .into_iter()
            .map(|d| {
                (
                    d.file_id,
                    FileProgress {
                        downloaded: d.bytes_downloaded,
                        speed_bps: d.speed_bps.max(0.0) as u64,
                        phase: d.phase,
                    },
                )
            })
            .collect();
        let stalls = transfer_ids
            .iter()
            .filter_map(|id| stalls.get(*id).map(|s| (*id, s)))
            .collect();

        // The start of the last finished sync run: a transfer done after it waits for
        // the next run
        let sync = self.sync_status().await;
        let last_sync_start = if sync.in_progress {
            sync.last_finished_at
                .zip(sync.last_duration)
                .and_then(|(finished, took)| finished.checked_sub(took))
        } else {
            sync.last_started_at
        };

        Ok(build(&BuildInput {
            transfers,
            files,
            holds: holds.into_iter().map(|h| h.transfer_id).collect(),
            progress,
            stalls,
            transfer_origins,
            notes: self.file_notes(),
            queue: Some(self.queue_state().await),
            last_sync_start,
            next_sync_at: sync.next_due_at,
            now: SystemTime::now(),
            putio_as_of: snapshot.as_ref().map(|s| s.fetched_at),
            putio_stale: snapshot.is_some_and(|s| s.stale),
        }))
    }

    /// Whether the Pause local downloads switch is on (feature 039, R11). It lives in the
    /// database, so it survives a restart; a missing flag, or no database, means off.
    pub async fn nas_paused(&self) -> bool {
        let Some(db) = self.database.as_ref() else {
            return false;
        };
        db.flags()
            .get_bool(NAS_DOWNLOADS_PAUSED)
            .await
            .unwrap_or_else(|e| {
                warn!(error = %e, "Failed to read the Pause local downloads switch");
                false
            })
    }

    /// Turn the Pause local downloads switch on or off (feature 039, R11)
    ///
    /// On: workers claim nothing, and every running file stops and goes back to
    /// `pending` (not `paused`), keeping its partial. Files paused on their own stay
    /// paused. Off: workers claim again, and no row is touched.
    pub async fn set_nas_paused(&self, paused: bool) -> DownloadResult<()> {
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for the switch".to_string())
        })?;
        db.flags()
            .set_bool(NAS_DOWNLOADS_PAUSED, paused)
            .await
            .map_err(internal)?;
        telemetry::pipeline::set_nas_paused(paused);
        let mut stopped = 0;
        if paused {
            let running: Vec<i64> = locked(&self.stop_signals).keys().copied().collect();
            // Rows first, so a stopped file is left `pending`
            let requeued = db.queue().unclaim_files(&running).await.map_err(internal)?;
            stopped = self.stop_files(&requeued).await.len();
        }
        if !paused {
            self.work_notify.notify_waiters();
        }
        info!(paused, stopped, "Pause local downloads switched");
        Ok(())
    }

    /// Pause queue rows, and stop the ones that are running (feature 039, R10). Returns
    /// the ids that were paused.
    pub async fn pause_files(&self, file_ids: &[i64]) -> DownloadResult<Vec<i64>> {
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for pause".to_string())
        })?;
        let changed = db.queue().pause_files(file_ids).await.map_err(internal)?;
        self.stop_files(&changed).await;
        Ok(changed)
    }

    /// Wake idle workers now, after queue rows were set to pending outside the manager
    pub fn wake_workers(&self) {
        self.work_notify.notify_waiters();
    }

    /// Put paused queue rows back in the queue, in their old place. Returns the ids that
    /// were resumed.
    pub async fn resume_files(&self, file_ids: &[i64]) -> DownloadResult<Vec<i64>> {
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for resume".to_string())
        })?;
        let resumed = db.queue().resume_files(file_ids).await.map_err(internal)?;
        if !resumed.is_empty() {
            self.work_notify.notify_waiters();
        }
        Ok(resumed)
    }

    /// Pause items of the Active page by key (feature 039, FR-013, FR-014, FR-016). An item
    /// on the NAS has its files paused and any running one stopped; one still on Put.io (or
    /// finished and waiting for sync) is held, so its files are queued paused. Nothing is
    /// sent to Put.io.
    pub async fn pause_items(
        &self,
        keys: &[String],
        stalls: &StallTracker,
        retention_hours: u64,
    ) -> DownloadResult<Vec<ItemResult>> {
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for pause".to_string())
        })?;
        let (pipeline, queued) = self.action_context(stalls, retention_hours).await?;
        let items: HashMap<&str, &PipelineItem> =
            pipeline.items.iter().map(|i| (i.key.as_str(), i)).collect();

        let mut results = Vec::with_capacity(keys.len());
        for key in keys {
            let result = match items.get(key.as_str()) {
                None => ActionResult::NotFound,
                Some(item) => self.pause_item(db, item, queued.get(key)).await?,
            };
            results.push(ItemResult {
                key: key.clone(),
                result,
            });
        }
        Ok(results)
    }

    async fn pause_item(
        &self,
        db: &Database,
        item: &PipelineItem,
        files: Option<&Vec<(i64, String)>>,
    ) -> DownloadResult<ActionResult> {
        if item.stage == Stage::Nas && item.state != State::WaitingForSync {
            let ids: Vec<i64> = files
                .into_iter()
                .flatten()
                .filter(|(_, status)| status == "pending" || status == "downloading")
                .map(|(id, _)| *id)
                .collect();
            if ids.is_empty() {
                return Ok(ActionResult::Already);
            }
            // The rows may have changed since they were read, for a key named twice
            let changed = self.pause_files(&ids).await?;
            if changed.is_empty() {
                return Ok(ActionResult::Already);
            }
            info!(
                transfer_id = item.transfer_id,
                name = %item.name,
                files = changed.len(),
                "Paused transfer"
            );
            telemetry::pipeline::action(PipelineAction::Pause);
            return Ok(ActionResult::Paused);
        }

        // Still on Put.io, or finished and waiting for the sync: Put.io has no pause, so
        // the NAS download is held. An item in error has nothing to hold.
        let Some(transfer_id) = item.transfer_id.filter(|_| item.state != State::Error) else {
            return Ok(ActionResult::NotFound);
        };
        if db.holds().is_held(transfer_id).await.map_err(internal)? {
            return Ok(ActionResult::Already);
        }
        db.holds()
            .add(transfer_id, &item.name)
            .await
            .map_err(internal)?;
        info!(transfer_id, name = %item.name, files = 0, "Held transfer");
        telemetry::pipeline::action(PipelineAction::Hold);
        Ok(ActionResult::Held)
    }

    /// Delete the holds whose transfer is in error or gone from Put.io, since nothing
    /// will arrive to hold (feature 039, R9). The list is the shared snapshot; a stale one
    /// (Put.io unreachable) is not trusted to say a transfer is gone. Returns the transfer
    /// ids released.
    pub async fn release_stale_holds(&self, max_age: Duration) -> DownloadResult<Vec<i64>> {
        let Some(db) = self.database.as_ref() else {
            return Ok(Vec::new());
        };
        let snapshot = self.transfers_snapshot(max_age).await?;
        if snapshot.stale {
            return Ok(Vec::new());
        }
        let mut released = Vec::new();
        for hold in db.holds().list().await.map_err(internal)? {
            let alive = snapshot
                .transfers
                .iter()
                .any(|t| t.id == hold.transfer_id && !t.status.eq_ignore_ascii_case("ERROR"));
            if !alive
                && db
                    .holds()
                    .remove(hold.transfer_id)
                    .await
                    .map_err(internal)?
            {
                info!(
                    transfer_id = hold.transfer_id,
                    name = %hold.name,
                    "Released the hold of a transfer that failed or is gone"
                );
                released.push(hold.transfer_id);
            }
        }
        Ok(released)
    }

    /// Resume items of the Active page by key (feature 039, FR-015): a hold is released and
    /// the item's paused files go back in the queue in their old place
    pub async fn resume_items(
        &self,
        keys: &[String],
        stalls: &StallTracker,
        retention_hours: u64,
    ) -> DownloadResult<Vec<ItemResult>> {
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for resume".to_string())
        })?;
        let (pipeline, queued) = self.action_context(stalls, retention_hours).await?;
        let items: HashMap<&str, &PipelineItem> =
            pipeline.items.iter().map(|i| (i.key.as_str(), i)).collect();

        let mut results = Vec::with_capacity(keys.len());
        for key in keys {
            let result = match items.get(key.as_str()) {
                None => ActionResult::NotFound,
                Some(item) => {
                    let paused: Vec<i64> = queued
                        .get(key)
                        .into_iter()
                        .flatten()
                        .filter(|(_, status)| status == "paused")
                        .map(|(id, _)| *id)
                        .collect();
                    let released = match item.transfer_id {
                        Some(id) => db.holds().remove(id).await.map_err(internal)?,
                        None => false,
                    };
                    let resumed = self.resume_files(&paused).await?;
                    if released || !resumed.is_empty() {
                        info!(
                            transfer_id = item.transfer_id,
                            name = %item.name,
                            files = resumed.len(),
                            "Resumed transfer"
                        );
                        telemetry::pipeline::action(PipelineAction::Resume);
                        ActionResult::Resumed
                    } else {
                        ActionResult::Already
                    }
                }
            };
            results.push(ItemResult {
                key: key.clone(),
                result,
            });
        }
        Ok(results)
    }

    /// What a pause or resume reads: the pipeline, and each item's files in the queue as
    /// `(file_id, status)` by item key. With Put.io unreachable and no list yet, the
    /// pipeline holds NAS items only.
    async fn action_context(
        &self,
        stalls: &StallTracker,
        retention_hours: u64,
    ) -> DownloadResult<(Pipeline, HashMap<String, Vec<(i64, String)>>)> {
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for the pipeline".to_string())
        })?;
        let pipeline = match self.pipeline(ACTION_MAX_AGE, stalls, retention_hours).await {
            Err(DownloadError::ClientError(e)) => {
                warn!(error = %error_chain(&e), "Put.io is unreachable; acting on NAS items only");
                self.pipeline_cached(stalls, retention_hours).await?
            }
            other => other?,
        };
        let transfer_ids: Vec<i64> = pipeline
            .items
            .iter()
            .filter_map(|i| i.transfer_id)
            .collect();
        let mut queued: HashMap<String, Vec<(i64, String)>> = HashMap::new();
        for file in db
            .pipeline_files(&transfer_ids, retention_hours)
            .await
            .map_err(internal)?
        {
            if let FileState::Queued(status) = &file.state {
                queued
                    .entry(file_key(&file))
                    .or_default()
                    .push((file.file_id, status.clone()));
            }
        }
        Ok((pipeline, queued))
    }

    /// In-memory notes per file, such as "Started again: the partial download was gone"
    pub fn file_notes(&self) -> HashMap<i64, String> {
        locked(&self.notes).clone()
    }

    /// Register a stop signal for `file_id` until the guard is dropped
    fn register_stop(&self, file_id: i64) -> StopGuard {
        let signal = Arc::new(StopSignal::default());
        self.stop_signals
            .lock()
            .unwrap()
            .insert(file_id, signal.clone());
        StopGuard {
            signals: self.stop_signals.clone(),
            file_id,
            signal,
        }
    }

    /// The stop signal of a running `file_id`, if any
    fn stop_signal(&self, file_id: i64) -> Option<Arc<StopSignal>> {
        locked(&self.stop_signals).get(&file_id).cloned()
    }

    /// Get statistics for all active workers
    ///
    /// Returns a list of worker stats including bandwidth usage and limits.
    pub async fn get_worker_stats(&self) -> Vec<WorkerStats> {
        let workers = self.workers.read().await;
        workers.values().map(|w| w.stats.clone()).collect()
    }

    /// Get statistics for a specific worker by ID
    pub async fn get_worker_stat(&self, worker_id: usize) -> Option<WorkerStats> {
        let workers = self.workers.read().await;
        workers.get(&worker_id).map(|w| w.stats.clone())
    }
}

/// How old the Put.io list may be for a pause or resume: the page's own age (R3)
const ACTION_MAX_AGE: Duration = Duration::from_secs(10);

fn internal(e: putio_sync_db::DbError) -> DownloadError {
    DownloadError::InternalError(e.to_string())
}

/// The transfer whose file or folder holds `file_id`, walking up the listed parents
/// What a plugin is told about a file of the sync (feature 028)
pub(crate) fn hook_file(
    id: i64,
    name: &str,
    size: u64,
    folder: &str,
    decision: &Decision,
) -> HookFile {
    HookFile {
        id,
        name: name.to_string(),
        size,
        extension: extension(name).unwrap_or_default(),
        folder: folder.to_string(),
        decision: match decision {
            Decision::Download => "download",
            Decision::Stream { .. } => "stream",
            Decision::Blocked { .. } => "blocked",
            Decision::Skipped { .. } => "skipped",
        }
        .to_string(),
        reason: decision.reason(),
    }
}

/// The built-in decision, or a plugin filter's answer when one gives it
async fn filtered(
    hooks: &dyn SyncHooks,
    file: &FileEntry,
    folder: &str,
    decision: Decision,
) -> Decision {
    let asked = hook_file(
        file.id,
        &file.name,
        file.size.max(0) as u64,
        folder,
        &decision,
    );
    let Some((answer, plugin)) = hooks.filter(&asked).await else {
        return decision;
    };
    debug!(file_id = file.id, plugin = %plugin, answer = ?answer, "Plugin filter answered");
    match answer {
        FilterAnswer::Download => Decision::Download,
        FilterAnswer::Stream => Decision::Stream { rule: None },
        FilterAnswer::Skip => Decision::Blocked {
            reason: Reason::Plugin { name: plugin },
            rule: None,
        },
    }
}

fn matching_transfer<'t>(
    file_id: i64,
    parents: &HashMap<i64, Option<i64>>,
    transfers: &'t HashMap<i64, Transfer>,
) -> Option<&'t Transfer> {
    // Guards against a parent cycle in a malformed listing
    const MAX_DEPTH: usize = 64;
    let mut current = Some(file_id);
    for _ in 0..MAX_DEPTH {
        let id = current?;
        if let Some(transfer) = transfers.get(&id) {
            return Some(transfer);
        }
        current = parents.get(&id).copied().flatten();
    }
    None
}

/// Where a queued file came from: its transfer's type, tracker host and privacy, how
/// the transfer was created, and the top-level Put.io folder (feature 006, R11)
async fn download_source(
    db: &Database,
    transfer: Option<&Transfer>,
    relative_path: &str,
) -> DownloadSource {
    let putio_folder = relative_path
        .split('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .unwrap_or("(root)")
        .to_string();
    let Some(transfer) = transfer else {
        return DownloadSource {
            transfer_id: None,
            source_type: None,
            tracker_host: None,
            is_private: false,
            origin: Origin::Unknown,
            watch_folder: None,
            putio_folder: Some(putio_folder),
        };
    };

    let created = match db.sources().transfer_origin(transfer.id).await {
        Ok(created) => created,
        Err(e) => {
            warn!(transfer_id = transfer.id, error = %e, "Failed to read transfer origin");
            None
        }
    };
    let source_type = SourceType::from_putio(transfer.transfer_type.as_deref());
    let torrent = source_type == SourceType::Torrent;
    DownloadSource {
        transfer_id: Some(transfer.id),
        source_type: Some(source_type),
        tracker_host: transfer.any_tracker_host().filter(|_| torrent),
        is_private: torrent && transfer.is_private,
        origin: created.as_ref().map_or(Origin::Putio, |c| c.origin),
        watch_folder: created.and_then(|c| c.watch_folder),
        putio_folder: Some(putio_folder),
    }
}

/// A file's path under the download folder for events, `/`-separated
/// `cleanup.deleted`: delete-after-download removed these from Put.io (feature 057)
fn cleanup_deleted(file_ids: &[i64]) {
    events::emit(Payload::CleanupDeleted(events::CleanupDeleted {
        file_ids: file_ids.to_vec(),
        count: file_ids.len() as u64,
        reason: "downloaded".to_string(),
    }));
}

fn event_path(relative_dir: &str, name: &str) -> String {
    let dir = relative_dir.replace('\\', "/");
    let dir = dir.trim_matches('/');
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", dir, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_space_floor_decision() {
        const GB: u64 = 1 << 30;
        // Off
        assert!(!below_free_space_floor(0, 0, 100 * GB));
        // Plenty of room
        assert!(!below_free_space_floor(50 * GB, 5 * GB, 0));
        assert!(!below_free_space_floor(50 * GB, 5 * GB, 10 * GB));
        // Under the floor already
        assert!(below_free_space_floor(4 * GB, 5 * GB, 0));
        // Exactly at the floor is fine; one byte under is not
        assert!(!below_free_space_floor(5 * GB, 5 * GB, 0));
        assert!(below_free_space_floor(5 * GB - 1, 5 * GB, 0));
        // The next file would take it under (and a file bigger than the disk can't wrap)
        assert!(below_free_space_floor(8 * GB, 5 * GB, 4 * GB));
        assert!(!below_free_space_floor(8 * GB, 5 * GB, 3 * GB));
        assert!(below_free_space_floor(GB, 5 * GB, 100 * GB));
    }

    #[test]
    fn reservations_stop_concurrent_overshoot_and_release_on_drop() {
        const GB: u64 = 1 << 30;
        let space = SpaceReservations::default();
        // 10 GB free, 5 GB floor: room for 5 GB in all
        let a = space.try_reserve(10 * GB, 5 * GB, 3 * GB).expect("fits");
        assert_eq!(space.reserved(), 3 * GB);
        // Each would pass alone, but not with the first one's bytes still to come
        assert!(space.try_reserve(10 * GB, 5 * GB, 3 * GB).is_none());
        assert_eq!(space.reserved(), 3 * GB, "a refusal reserves nothing");
        let b = space.try_reserve(10 * GB, 5 * GB, 2 * GB).expect("fits");
        assert!(space.try_reserve(10 * GB, 5 * GB, 1).is_none());
        drop(a);
        assert_eq!(space.reserved(), 2 * GB);
        assert!(space.try_reserve(10 * GB, 5 * GB, 3 * GB).is_some());
        drop(b);
        // Floor off: always reserves
        assert!(space.try_reserve(0, 0, 100 * GB).is_some());
    }

    #[test]
    fn reservations_hold_under_threads() {
        const GB: u64 = 1 << 30;
        let space = SpaceReservations::default();
        let granted: Vec<_> = (0..16)
            .map(|_| {
                let space = space.clone();
                std::thread::spawn(move || space.try_reserve(10 * GB, 5 * GB, GB))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|h| h.join().unwrap())
            .collect();
        assert_eq!(granted.len(), 5, "exactly the room above the floor");
        assert_eq!(space.reserved(), 5 * GB);
    }

    #[test]
    fn free_bytes_walks_up_to_an_existing_parent() {
        let dir = tempfile::tempdir().unwrap();
        let warned = Mutex::new(None);
        assert!(free_bytes_of(&dir.path().join("not/yet/made"), &warned).is_some());
        assert!(warned.lock().unwrap().is_none(), "no warning for that");
    }

    #[test]
    fn test_download_file_struct() {
        let file = DownloadFile {
            file_id: 123,
            filename: "test.txt".to_string(),
            file_size: 1024,
            download_url: "https://example.com/test.txt".to_string(),
            relative_path: "./".to_string(),
            crc32: None,
            save_as: None,
            fixed_path: false,
            overwrite: false,
        };

        assert_eq!(file.file_id, 123);
        assert_eq!(file.filename, "test.txt");
        assert_eq!(file.file_size, 1024);
    }

    #[test]
    fn test_download_operation_result() {
        let result = DownloadOperationResult {
            file_id: 123,
            success: true,
            error: None,
            bytes_downloaded: 1024,
            file_path: Some(PathBuf::from("/tmp/test.txt")),
            transient: false,
        };

        assert_eq!(result.file_id, 123);
        assert!(result.success);
        assert!(result.error.is_none());
        assert_eq!(result.bytes_downloaded, 1024);
    }
}
