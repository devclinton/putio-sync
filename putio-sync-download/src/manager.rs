//! Download manager for orchestrating downloads

use crate::error::{DownloadError, DownloadResult};
use crate::files::{
    check_resume, cleanup_partial, crc32_file, crc32_prefix, crc_equal, create_temp_path,
    needs_checksum, next_free_name, plan_existing, valid_crc32, Crc32, ExistingAction, LocalState,
};
use crate::progress::{DownloadProgress, OverallProgress, ProgressCallback, ProgressTracker};
use crate::rate_limiter::RateLimiter;
use crate::stream::{finish_download, stream_to_temp, StopSignal};
use crate::sync_status::{SyncOutcome, SyncRequestError, SyncStatus};
use crate::transfer_events::{self, TransferChange, TransferClass, TransferEvents};
use crate::utils::retry_with_stall_detection;
use putio_sync_client::models::Transfer;
use putio_sync_client::{error_chain, FileEntry, PutioApiClient};
use putio_sync_core::bandwidth::{
    Override, OverrideMode, Policy, ScheduleEntry, Status as BandwidthStatus, DEFAULT_SLOW_LIMIT,
};
use putio_sync_core::config::{ExistingFile, ExistingOutcome};
use putio_sync_core::events::{self, Payload};
use putio_sync_core::redact;
use putio_sync_core::rules::{extension, Decision, Effective, RuleSet};
use putio_sync_core::stall::StallTracker;
use putio_sync_db::models::{DownloadSource, DownloadStatus, Origin, QueueStatus, SourceType};
use putio_sync_db::pipeline::TRANSFER_EVENTS_SEEN;
use putio_sync_db::Database;
use putio_sync_telemetry::{self as telemetry, RuleDecision, SyncKind, SyncResult};
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
    /// Number of concurrent download workers
    pub workers: usize,
    /// Delete files from Put.io after successful download
    pub delete_after_download: bool,
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
            workers: 1,
            delete_after_download: false,
            rules: RuleSet::default(),
            verify: VerifySettings::default(),
        }
    }
}

impl From<&putio_sync_core::config::DownloadConfig> for DownloadConfig {
    /// The one mapping from `[download]` to the manager, used by `daemon`, `server` and
    /// the settings API. `[download]` fields with no manager counterpart (`chunk_size_mb`, `progress_style`, sorting, glob patterns, `delete_empty_folders`,
    /// `max_concurrent`) are read by the one-off `download` command only.
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
            workers: settings.workers,
            delete_after_download: settings.delete_after_download,
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
        let mut signals = self.signals.lock().unwrap();
        // A later registration for the same file keeps its own signal
        if signals
            .get(&self.file_id)
            .is_some_and(|s| Arc::ptr_eq(s, &self.signal))
        {
            signals.remove(&self.file_id);
        }
    }
}

/// Note shown when a stopped file had to start again from 0 (feature 039, R8)
const RESTARTED_NOTE: &str = "Started again: the partial download was gone";

/// Sanitize a filename to remove invalid characters
pub(crate) fn sanitize_filename(filename: &str) -> String {
    filename
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect()
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
    /// Blocked files that would be deleted from Put.io
    pub would_delete: usize,
    /// Blocked and skipped files, the first `PREVIEW_ITEMS`
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
    /// The last Put.io transfer list, shared by the Active page, Status and the daemon
    /// loop (feature 039, R3). The async lock makes concurrent callers share one fetch.
    transfers_cache: Arc<tokio::sync::Mutex<Option<CachedTransfers>>>,
    /// The transfer classes behind `transfer.*` events; `None` until read from the
    /// database (feature 027)
    transfer_events: Arc<tokio::sync::Mutex<Option<TransferEvents>>>,
    /// The one bandwidth limiter every download shares (feature 032)
    pub(crate) bandwidth: RateLimiter,
    /// The "verify everything" run (feature 034)
    pub(crate) verifier: crate::verify::VerifyRunner,
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
            overall_progress,
            workers: Arc::new(RwLock::new(HashMap::new())),
            next_worker_id: Arc::new(RwLock::new(0)),
            decided: Arc::new(Mutex::new(HashSet::new())),
            sync_status: Arc::new(RwLock::new(SyncStatus::default())),
            sync_notify: Arc::new(Notify::new()),
            manual_sync: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            stop_signals: Arc::new(Mutex::new(HashMap::new())),
            stopped_partial: Arc::new(Mutex::new(HashSet::new())),
            notes: Arc::new(Mutex::new(HashMap::new())),
            transfers_cache: Arc::new(tokio::sync::Mutex::new(None)),
            transfer_events: Arc::new(tokio::sync::Mutex::new(None)),
            paths_in_use: Arc::new(Mutex::new(HashSet::new())),
            checking: Arc::new(Mutex::new(HashMap::new())),
            verifier: crate::verify::VerifyRunner::default(),
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
            overall_progress,
            workers: Arc::new(RwLock::new(HashMap::new())),
            next_worker_id: Arc::new(RwLock::new(0)),
            decided: Arc::new(Mutex::new(HashSet::new())),
            sync_status: Arc::new(RwLock::new(SyncStatus::default())),
            sync_notify: Arc::new(Notify::new()),
            manual_sync: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            stop_signals: Arc::new(Mutex::new(HashMap::new())),
            stopped_partial: Arc::new(Mutex::new(HashSet::new())),
            notes: Arc::new(Mutex::new(HashMap::new())),
            transfers_cache: Arc::new(tokio::sync::Mutex::new(None)),
            transfer_events: Arc::new(tokio::sync::Mutex::new(None)),
            paths_in_use: Arc::new(Mutex::new(HashSet::new())),
            checking: Arc::new(Mutex::new(HashMap::new())),
            verifier: crate::verify::VerifyRunner::default(),
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
                        // Queue empty, sleep briefly and retry
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
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
        self.paths_in_use
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(path)
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

        // Construct the folder, preserving directory structure
        let dir = if file.relative_path.is_empty() {
            config.target_directory.clone()
        } else {
            config.target_directory.join(&file.relative_path)
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
                });
            }
            ExistingAction::Refuse => {
                existing.record(&target_path, ExistingOutcome::Refused, None);
                let e = DownloadError::InvalidInput(format!(
                    "target path is a folder: {}",
                    target_path.display()
                ));
                telemetry::download::failed(e.fail_reason());
                if let Some(ref callback) = progress_callback {
                    callback.on_file_complete(file_id, &filename, false);
                }
                return Ok(DownloadOperationResult {
                    file_id,
                    success: false,
                    error: Some(e.to_string()),
                    bytes_downloaded: 0,
                    file_path: None,
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
        if resumed_from.is_none() && self.stopped_partial.lock().unwrap().remove(&file_id) {
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
                self.notes.lock().unwrap().remove(&file_id);
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
                })
            }
            Err(DownloadError::Stopped) => {
                info!(file_id, name = %filename, "Download stopped; partial file kept");
                if tokio::fs::metadata(&temp_path)
                    .await
                    .is_ok_and(|m| m.len() > 0)
                {
                    self.stopped_partial.lock().unwrap().insert(file_id);
                }
                Err(DownloadError::Stopped)
            }
            Err(e) => {
                telemetry::download::failed(e.fail_reason());
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
                })
            }
        }
    }

    /// Wait until no other worker writes `path`, then hold it until the claim drops
    pub async fn claim_path(&self, path: &std::path::Path) -> PathClaim {
        loop {
            {
                let mut in_use = self.paths_in_use.lock().unwrap_or_else(|e| e.into_inner());
                if in_use.insert(path.to_path_buf()) {
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
        let result = retry_with_stall_detection(
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
            &format!("Download {}", filename),
        )
        .await;
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
        let result = self.sync_downloads_inner().await;
        self.sync_status.write().await.finish(match &result {
            Ok(stats) => SyncOutcome::from(stats),
            Err(e) => SyncOutcome::Failure {
                error: error_chain(e),
            },
        });
        let (outcome, stats) = match &result {
            Ok(stats) => (SyncResult::Success, stats.clone()),
            Err(_) => (SyncResult::Failure, SyncStats::default()),
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
        self.enqueue_tree(0).await
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
    pub async fn enqueue_tree_report(
        &self,
        root_id: i64,
        fixed_path: bool,
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
        // Transfers whose release check waits for Put.io (feature 042, research R8)
        let gated = self.gated_transfers(db).await;
        if !gated.is_empty() {
            transfers = Some(self.transfers_by_file().await);
        }

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
            let decision = rules.decide(&file.name);
            if decision != Decision::Download {
                stats.files_blocked += 1;
                decide(match decision {
                    Decision::Skipped { .. } => TreeDecision::Skipped,
                    _ => TreeDecision::Blocked(extension(&file.name).unwrap_or_default()),
                });
                if matches!(decision, Decision::Blocked { .. }) && rules.delete_blocked.value {
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
            match downloads_repo.find_by_file_id(file.id).await {
                Ok(Some(record)) if record.status == "completed" => {
                    debug!(
                        "Skipping already downloaded file: {} (ID: {})",
                        file.name, file.id
                    );
                    stats.files_skipped += 1;
                    decide(TreeDecision::Known);
                    continue;
                }
                Ok(_) => {
                    // Not downloaded or incomplete - continue to queue check
                }
                Err(e) => {
                    warn!(
                        "Database error checking downloads for file {}: {}",
                        file.id, e
                    );
                    // Continue anyway - will try to add to queue
                }
            }

            // Check if already in queue (pending or downloading)
            match queue_repo.get_by_file_id(file.id).await {
                Ok(Some(queue_item)) => {
                    // File is already in queue - skip to avoid duplicates
                    debug!(
                        "Skipping file already in queue: {} (ID: {}, status: {})",
                        file.name, file.id, queue_item.status
                    );
                    stats.files_skipped += 1;
                    decide(TreeDecision::Known);
                    continue;
                }
                Ok(None) => {
                    // Not in queue - safe to add
                }
                Err(e) => {
                    warn!("Database error checking queue for file {}: {}", file.id, e);
                    // Continue anyway - queue.add has ON CONFLICT handling
                }
            }

            // Add to queue
            debug!(
                "Adding to queue: {} ({} bytes) at path '{}'",
                file.name, file.size, relative_path
            );

            let download_url = self.client.get_download_url(file.id);

            match queue_repo
                .add_with_path_kept(
                    file.id,
                    &file.name,
                    file.size,
                    &download_url,
                    &relative_path,
                    0,
                    QueueStatus::Pending,
                    fixed_path,
                )
                .await
            {
                Ok(_) => {
                    stats.files_downloaded += 1; // Renamed but means "added to queue"
                    decide(TreeDecision::Queued);
                    debug!("Added to queue: {}", file.name);
                    self.forget_decision(db, file.id).await;
                    if transfers.is_none() {
                        transfers = Some(self.transfers_by_file().await);
                    }
                    let transfer =
                        matching_transfer(file.id, &parents, transfers.as_ref().unwrap());
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

        stats.files_deleted = self.delete_blocked(db, to_delete).await;

        info!(
            "Sync complete - Queued: {}, Skipped: {}, Blocked: {}, Deleted: {}, Failed: {}",
            stats.files_downloaded,
            stats.files_skipped,
            stats.files_blocked,
            stats.files_deleted,
            stats.files_failed
        );

        Ok(TreeReport {
            stats,
            files: report_files,
            root_name,
        })
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

        // Get config snapshot
        let config = self.config.read().await;

        // Require database for queue-based downloads
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for queue processing".to_string())
        })?;

        let queue_repo = db.queue();

        // Atomically claim next pending item (prevents race conditions between workers)
        let queue_item = queue_repo.claim_next_pending().await.map_err(|e| {
            DownloadError::InternalError(format!("Failed to claim next pending item: {}", e))
        })?;

        let queue_item = match queue_item {
            Some(item) => item,
            None => {
                debug!("Queue is empty");
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

        info!(
            "Processing queue item: {} (ID: {})",
            queue_item.filename, file_id
        );

        // Verify file still exists on Put.io and get current info
        match self.client.get_file_details(file_id).await {
            Ok(file_response) => {
                let file_info = file_response.file;
                info!(
                    "File verified on Put.io: {} ({} bytes)",
                    file_info.name, file_info.size
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
                        info!("Successfully downloaded: {}", file_info.name);

                        // Delete from Put.io if configured
                        if config
                            .effective(&queue_item.relative_path)
                            .delete_after_download
                            .value
                        {
                            info!(
                                "Deleting file from Put.io: {} (ID: {})",
                                file_info.name, file_id
                            );
                            match self.client.delete_files(vec![file_id]).await {
                                Ok(_) => {
                                    info!("Successfully deleted from Put.io: {}", file_info.name);
                                }
                                Err(e) => {
                                    warn!(
                                        "Failed to delete file {} from Put.io: {}",
                                        file_info.name, e
                                    );
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

                        Ok(true)
                    }
                    // Stopped on request: keep the partial and the row as the caller set it
                    Err(DownloadError::Stopped) => Ok(true),
                    Ok(_) | Err(_) => {
                        error!("Failed to download: {}", file_info.name);

                        // Update queue status to failed
                        if let Err(e) = queue_repo.update_status(file_id, QueueStatus::Failed).await
                        {
                            warn!("Failed to update queue status to failed: {}", e);
                        }

                        Ok(true) // Still processed, even if failed
                    }
                }
            }
            Err(e) => {
                warn!(
                    "File {} no longer exists on Put.io or is inaccessible: {}",
                    file_id, e
                );

                // Remove from queue since file doesn't exist
                if let Err(e) = queue_repo.remove(file_id).await {
                    warn!("Failed to remove stale item from queue: {}", e);
                }

                Ok(true) // Processed (removed stale item)
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
        info!(
            "Starting Put.io sync loop with interval: {}s",
            interval_secs
        );
        let interval = std::time::Duration::from_secs(interval_secs);
        self.sync_status.write().await.schedule(interval);
        let mut cycle = 0;

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
            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = self.sync_notify.notified() => {
                    debug!("Sync requested");
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

    /// List all files recursively from Put.io
    /// Returns a list of (FileEntry, relative_path) tuples, with paths under `base_path`
    async fn list_all_files(
        &self,
        parent_id: i64,
        base_path: String,
    ) -> DownloadResult<Vec<(FileEntry, String)>> {
        let mut all_files = Vec::new();
        // Track (folder_id, relative_path) for each folder to process
        let mut folders_to_process = vec![(parent_id, base_path)];
        // Map folder_id -> folder_name for building paths
        let mut folder_names = std::collections::HashMap::new();

        while let Some((current_folder, current_path)) = folders_to_process.pop() {
            debug!(
                "Fetching files from folder: {} (path: {})",
                current_folder, current_path
            );

            let response = self
                .client
                .list_files(current_folder)
                .await
                .map_err(DownloadError::ClientError)?;

            for file in response.files {
                if file.file_type == "FOLDER" {
                    // Build the folder's relative path
                    let folder_path = join_path(&current_path, &sanitize_filename(&file.name));

                    folder_names.insert(file.id, file.name.clone());
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

        // Get all items with 'downloading' status
        let all_items = queue_repo.get_all().await.map_err(|e| {
            DownloadError::InternalError(format!("Failed to get queue items: {}", e))
        })?;

        let downloading_items: Vec<_> = all_items
            .into_iter()
            .filter(|item| item.status == "downloading")
            .collect();

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
        let signals = self.stop_signals.lock().unwrap();
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
        let mut cache = self.transfers_cache.lock().await;
        if let Some(cached) = cache.as_ref() {
            if cached.attempted.elapsed() < max_age {
                return Ok(cached.snapshot());
            }
        }
        match self.client.list_transfers().await {
            Ok(response) => {
                let snapshot = cache
                    .insert(CachedTransfers {
                        attempted: Instant::now(),
                        fetched_at: SystemTime::now(),
                        transfers: Arc::new(response.transfers),
                        stale: false,
                    })
                    .snapshot();
                drop(cache);
                self.track_transfers(&snapshot.transfers).await;
                Ok(snapshot)
            }
            Err(e) => match cache.as_mut() {
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
        let snapshot = self
            .transfers_cache
            .lock()
            .await
            .as_ref()
            .map(|c| c.snapshot());
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
        let internal = |e: putio_sync_db::DbError| DownloadError::InternalError(e.to_string());

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
            last_sync_start,
            next_sync_at: sync.next_due_at,
            now: SystemTime::now(),
            putio_as_of: snapshot.as_ref().map(|s| s.fetched_at),
            putio_stale: snapshot.is_some_and(|s| s.stale),
        }))
    }

    /// In-memory notes per file, such as "Started again: the partial download was gone"
    pub fn file_notes(&self) -> HashMap<i64, String> {
        self.notes.lock().unwrap().clone()
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
        self.stop_signals.lock().unwrap().get(&file_id).cloned()
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

/// The transfer whose file or folder holds `file_id`, walking up the listed parents
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
        };

        assert_eq!(result.file_id, 123);
        assert!(result.success);
        assert!(result.error.is_none());
        assert_eq!(result.bytes_downloaded, 1024);
    }
}
