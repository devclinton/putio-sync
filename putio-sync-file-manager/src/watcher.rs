//! Filesystem watching for .torrent and .magnet files
//!
//! This module provides functionality for watching directories and processing
//! torrent and magnet files. It supports both:
//! - Continuous daemon mode (TorrentWatcher)
//! - Periodic check mode (scan_once)

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use putio_sync_client::{error_chain, PutioApiClient, Transfer};
use putio_sync_db::models::{Origin, TransferOrigin};
use putio_sync_db::{CheckState, Database, NewCheck, NewReport, PendingCheck, ReportKind};
use putio_sync_precheck::magnet::from_magnet;
use putio_sync_precheck::metainfo::info_hash;
use putio_sync_precheck::policy::Key;
use putio_sync_precheck::putio::{self as putio_check, OnPutio};
use putio_sync_precheck::{
    Lookup, Metainfo, Outcome, Policy, PrecheckSource, Prechecker, Reason, Stage, Traits,
};
use putio_sync_telemetry::watch as watch_metrics;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock as StdRwLock};
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Statistics for a watch operation
#[derive(Debug, Clone, Default)]
pub struct WatchStats {
    /// Number of directories scanned
    pub directories_scanned: usize,
    /// Number of .torrent/.magnet files found
    pub files_found: usize,
    /// Number of files successfully processed
    pub files_processed: usize,
    /// Number of files that failed to process
    pub files_failed: usize,
    /// Number of files skipped (already processed)
    pub files_skipped: usize,
    /// Number of files the release check rejected (042 US4)
    pub files_rejected: usize,
    /// Number of already-uploaded files removed or archived by this scan
    pub cleaned_up: usize,
}

/// What `process_torrent_file` did with a watch file
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessOutcome {
    /// Uploaded to Put.io now (or Put.io already had the transfer)
    Uploaded,
    /// Uploaded by an earlier scan or run; only the post-upload action is left
    AlreadyUploaded,
    /// The file disappeared before it could be processed
    Gone,
    /// The file content cannot be uploaded
    Invalid(String),
    /// Not a .torrent or .magnet file
    Unsupported,
    /// Failed the release check: not uploaded, renamed `.rejected` (042 US4)
    Rejected(String),
}

/// Folders above an upload folder read for its path, so a loop can't hang a scan
const MAX_FOLDER_DEPTH: usize = 32;

/// A watch file's check before upload, for its `release_checks` row
struct WatchCheck {
    hash: Option<String>,
    name: String,
    source: PrecheckSource,
    watch_folder: String,
    lookup: Option<&'static str>,
    cache: Option<String>,
    policy: Policy,
}

/// What a post-upload action did
enum PostUploadAction {
    Archived(PathBuf),
    Deleted,
}

/// Runtime statistics for the file watcher
#[derive(Debug, Clone)]
pub struct WatcherRuntimeStats {
    /// Total files processed since watcher started
    pub total_processed: usize,
    /// Current watch directories
    pub watch_directories: Vec<PathBuf>,
    /// Parent folder ID for uploads
    pub parent_folder_id: Option<i64>,
    /// Check interval in seconds
    pub interval_seconds: u64,
    /// Archive directory (if configured)
    pub archive_directory: Option<PathBuf>,
    /// Delete after upload setting
    pub delete_after_upload: bool,
}

impl WatchStats {
    /// Create new empty stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Get total files encountered
    pub fn total_files(&self) -> usize {
        self.files_processed
            + self.files_failed
            + self.files_skipped
            + self.files_rejected
            + self.cleaned_up
    }

    /// Get success rate as percentage
    pub fn success_rate(&self) -> f64 {
        if self.files_found == 0 {
            return 100.0;
        }
        (self.files_processed as f64 / self.files_found as f64) * 100.0
    }
}

/// Core torrent watcher that can be used in different modes
pub struct TorrentWatchCore {
    /// Files that have been processed to avoid duplicates
    processed_files: Arc<Mutex<HashSet<PathBuf>>>,
    /// Paths already warned about, so a stuck file is reported once, not every scan
    warned: Mutex<HashSet<PathBuf>>,
    /// The release check (042 US4); files go up unchecked without it
    precheck: Option<Arc<Prechecker>>,
    /// Put.io folder paths by id, for the check's policy
    folder_paths: Mutex<HashMap<i64, String>>,
}

impl TorrentWatchCore {
    /// Create a new torrent watch core
    pub fn new() -> Self {
        Self {
            processed_files: Arc::new(Mutex::new(HashSet::new())),
            warned: Mutex::new(HashSet::new()),
            precheck: None,
            folder_paths: Mutex::new(HashMap::new()),
        }
    }

    /// Create a new torrent watch core with existing processed files
    pub fn with_processed_files(processed_files: Arc<Mutex<HashSet<PathBuf>>>) -> Self {
        Self {
            processed_files,
            warned: Mutex::new(HashSet::new()),
            precheck: None,
            folder_paths: Mutex::new(HashMap::new()),
        }
    }

    /// Check watch files before upload, and magnets that missed the caches once Put.io
    /// has them (042 US4)
    pub fn with_precheck(mut self, precheck: Arc<Prechecker>) -> Self {
        self.precheck = Some(precheck);
        self
    }

    /// Perform a single scan of the specified directories
    ///
    /// This method scans all directories once, processes any .torrent/.magnet files found,
    /// and returns statistics about the operation. It's designed for periodic checks.
    pub async fn scan_directories(
        &self,
        directories: &[PathBuf],
        client: &PutioApiClient,
        parent_id: Option<i64>,
        archive_dir: Option<&PathBuf>,
        delete_after_upload: bool,
        database: Option<&Database>,
    ) -> Result<WatchStats> {
        let mut stats = WatchStats::new();
        watch_metrics::scan();
        if let Some(db) = database {
            self.check_pending_on_putio(client, db).await;
        }

        debug!("Starting scan of {} directories", directories.len());

        for dir in directories {
            if !dir.exists() {
                debug!("Directory does not exist: {}", dir.display());
                continue;
            }

            if !dir.is_dir() {
                debug!("Path is not a directory: {}", dir.display());
                continue;
            }

            debug!("Scanning directory: {}", dir.display());
            stats.directories_scanned += 1;

            // Find torrent files in this directory
            let torrent_files = self.find_torrent_files(dir)?;
            stats.files_found += torrent_files.len();

            if !torrent_files.is_empty() {
                debug!(
                    "Found {} .torrent/.magnet file(s) in {}",
                    torrent_files.len(),
                    dir.display()
                );
            }

            // Process each torrent file
            for file_path in torrent_files {
                match self
                    .process_torrent_file(&file_path, client, parent_id, database)
                    .await
                {
                    Ok(ProcessOutcome::Uploaded) => {
                        stats.files_processed += 1;
                        watch_metrics::uploaded();
                        info!("Successfully processed: {}", file_path.display());

                        match self
                            .handle_post_upload_action(&file_path, archive_dir, delete_after_upload)
                            .await
                        {
                            Ok(Some(PostUploadAction::Archived(to))) => {
                                watch_metrics::cleaned_up();
                                info!(path = %file_path.display(), to = %to.display(), "Archived uploaded watch file");
                            }
                            Ok(Some(PostUploadAction::Deleted)) => {
                                watch_metrics::cleaned_up();
                                info!(path = %file_path.display(), "Deleted uploaded watch file");
                            }
                            Ok(None) => {}
                            Err(e) => {
                                watch_metrics::cleanup_failed();
                                self.warn_once(&file_path, |p| {
                                    warn!(path = %p.display(), error = %format!("{:#}", e), "Post-upload action failed");
                                })
                            }
                        }
                    }
                    Ok(ProcessOutcome::AlreadyUploaded) => {
                        if archive_dir.is_none() && !delete_after_upload {
                            stats.files_skipped += 1;
                            debug!("Skipped (already processed): {}", file_path.display());
                            continue;
                        }
                        match self
                            .handle_post_upload_action(&file_path, archive_dir, delete_after_upload)
                            .await
                        {
                            Ok(Some(PostUploadAction::Archived(to))) => {
                                stats.cleaned_up += 1;
                                watch_metrics::cleaned_up();
                                info!(path = %file_path.display(), to = %to.display(), "Archived already-uploaded watch file");
                            }
                            Ok(Some(PostUploadAction::Deleted)) => {
                                stats.cleaned_up += 1;
                                watch_metrics::cleaned_up();
                                info!(path = %file_path.display(), "Removed already-uploaded watch file");
                            }
                            Ok(None) => stats.files_skipped += 1,
                            Err(e) => {
                                stats.files_failed += 1;
                                watch_metrics::cleanup_failed();
                                self.warn_once(&file_path, |p| {
                                    warn!(path = %p.display(), error = %format!("{:#}", e), "Failed to clean up already-uploaded watch file");
                                });
                            }
                        }
                    }
                    // Renamed `.rejected`: no archive or delete step
                    Ok(ProcessOutcome::Rejected(_)) => stats.files_rejected += 1,
                    Ok(ProcessOutcome::Gone) | Ok(ProcessOutcome::Unsupported) => {
                        stats.files_skipped += 1;
                        debug!("Skipped: {}", file_path.display());
                    }
                    Ok(ProcessOutcome::Invalid(reason)) => {
                        stats.files_failed += 1;
                        self.warn_once(&file_path, |p| {
                            watch_metrics::invalid();
                            warn!(path = %p.display(), reason = %reason, "Invalid watch file");
                        });
                    }
                    Err(e) => {
                        stats.files_failed += 1;
                        error!("Failed to process {}: {}", file_path.display(), e);
                    }
                }
            }
        }

        let summary = format!(
            "Scan completed - directories: {}, files found: {}, processed: {}, cleaned up: {}, skipped: {}, rejected: {}, failed: {}",
            stats.directories_scanned,
            stats.files_found,
            stats.files_processed,
            stats.cleaned_up,
            stats.files_skipped,
            stats.files_rejected,
            stats.files_failed
        );
        if stats.files_processed + stats.cleaned_up + stats.files_rejected + stats.files_failed > 0
        {
            info!("{}", summary);
        } else {
            debug!("{}", summary);
        }

        Ok(stats)
    }

    /// Process a single .torrent or .magnet file
    ///
    /// Returns what happened to the file, or Err if the upload to Put.io failed.
    pub async fn process_torrent_file(
        &self,
        path: &Path,
        client: &PutioApiClient,
        parent_id: Option<i64>,
        database: Option<&Database>,
    ) -> Result<ProcessOutcome> {
        // Check if already processed using checksum if database is available
        if let Some(db) = database {
            let checksum = if self.is_magnet_file(path) {
                // For magnet files, calculate checksum of the magnet link content
                match tokio::fs::read_to_string(path).await {
                    Ok(magnet_content) => {
                        let trimmed = magnet_content.trim();
                        if trimmed.starts_with("magnet:") {
                            calculate_string_checksum(trimmed)
                        } else {
                            debug!(
                                "Invalid magnet file content in {}: doesn't start with 'magnet:'",
                                path.display()
                            );
                            return Ok(ProcessOutcome::Invalid(
                                "magnet file content doesn't start with 'magnet:'".to_string(),
                            ));
                        }
                    }
                    Err(e) => {
                        debug!("Failed to read magnet file {}: {}", path.display(), e);
                        return Ok(ProcessOutcome::Invalid(format!(
                            "failed to read magnet file: {}",
                            e
                        )));
                    }
                }
            } else {
                // For torrent files, calculate checksum of the binary content
                match calculate_file_checksum(path).await {
                    Ok(checksum) => checksum,
                    Err(e) => {
                        debug!("Failed to calculate checksum for {}: {}", path.display(), e);
                        // Fall back to path-based check
                        String::new()
                    }
                }
            };

            if !checksum.is_empty() {
                let repo = db.processed_torrents();
                if repo.is_processed(&checksum).await.unwrap_or(false) {
                    debug!(
                        "File already processed (checksum match): {}",
                        path.display()
                    );
                    // Also add to in-memory set for consistency
                    let mut processed = self.processed_files.lock().unwrap();
                    processed.insert(path.to_path_buf());
                    return Ok(ProcessOutcome::AlreadyUploaded);
                }
            }
        }

        // Check if already processed (in-memory fallback)
        {
            let processed = self.processed_files.lock().unwrap();
            if processed.contains(path) {
                debug!("File already processed: {}", path.display());
                return Ok(ProcessOutcome::AlreadyUploaded);
            }
        }

        // Verify file still exists and is a torrent/magnet
        if !path.exists() {
            debug!("File no longer exists: {}", path.display());
            return Ok(ProcessOutcome::Gone);
        }

        if !self.is_torrent_file(path) && !self.is_magnet_file(path) {
            debug!("Not a .torrent or .magnet file: {}", path.display());
            return Ok(ProcessOutcome::Unsupported);
        }

        let file_type = if self.is_magnet_file(path) {
            "magnet"
        } else {
            "torrent"
        };
        info!("🔼 Processing new .{} file: {}", file_type, path.display());

        let check = match self.precheck_file(path, client, parent_id).await? {
            Some((check, Outcome::Rejected(meta, reason))) => {
                return Ok(self.reject_file(path, &check, meta, reason, database).await);
            }
            other => other,
        };

        // Process based on file type
        let transfer_result = if self.is_magnet_file(path) {
            // Read magnet link from file and add as URL
            match tokio::fs::read_to_string(path).await {
                Ok(magnet_content) => {
                    let magnet_link = magnet_content.trim();
                    if magnet_link.starts_with("magnet:") {
                        client.add_transfer_url(magnet_link, parent_id, None).await
                    } else {
                        return Ok(ProcessOutcome::Invalid(
                            "magnet file content doesn't start with 'magnet:'".to_string(),
                        ));
                    }
                }
                Err(e) => {
                    return Ok(ProcessOutcome::Invalid(format!(
                        "failed to read magnet file: {}",
                        e
                    )));
                }
            }
        } else {
            // Upload torrent file
            client.add_transfer_file(path, parent_id, None).await
        };

        match transfer_result {
            Ok(transfer) => {
                info!(
                    "✅ {} uploaded successfully: {} (ID: {})",
                    if self.is_magnet_file(path) {
                        "Magnet link"
                    } else {
                        "Torrent"
                    },
                    transfer.name,
                    transfer.id
                );
                info!("   File: {}", path.display());
                info!("   Status: {}", transfer.status);
                info!("   Size: {}", self.format_bytes(transfer.size.unwrap_or(0)));
                if let Some(parent) = parent_id {
                    info!("   Parent folder ID: {}", parent);
                }

                // Mark as processed
                {
                    let mut processed = self.processed_files.lock().unwrap();
                    processed.insert(path.to_path_buf());
                }

                // Record in database if available
                if let Some(db) = database {
                    // Remember the transfer came from this watch folder, for the Stats
                    // page's origin breakdown (feature 006). Watching isn't recursive,
                    // so the file's directory is the configured watch folder.
                    let origin = TransferOrigin {
                        origin: Origin::Watch,
                        watch_folder: path.parent().map(|dir| dir.display().to_string()),
                    };
                    if let Err(e) = db
                        .sources()
                        .record_transfer_origin(transfer.id, &origin)
                        .await
                    {
                        warn!(transfer_id = transfer.id, error = %e, "Failed to record transfer origin");
                    }
                    if let Some((check, outcome)) = &check {
                        record_upload_check(db, check, outcome, transfer.id).await;
                    }

                    let (checksum, magnet_link) = if self.is_magnet_file(path) {
                        // For magnet files, record the magnet link and use content checksum
                        match tokio::fs::read_to_string(path).await {
                            Ok(content) => {
                                let magnet = content.trim().to_string();
                                let checksum = calculate_string_checksum(&magnet);
                                (checksum, Some(magnet))
                            }
                            Err(e) => {
                                debug!(
                                    "Failed to re-read magnet file for database recording: {}",
                                    e
                                );
                                return Ok(ProcessOutcome::Uploaded); // Still consider it processed
                            }
                        }
                    } else {
                        // For torrent files, calculate file checksum and no magnet link
                        match calculate_file_checksum(path).await {
                            Ok(checksum) => (checksum, None),
                            Err(e) => {
                                debug!(
                                    "Failed to calculate checksum for database recording: {}",
                                    e
                                );
                                return Ok(ProcessOutcome::Uploaded); // Still consider it processed
                            }
                        }
                    };

                    let repo = db.processed_torrents();
                    let file_size = path.metadata().map(|m| m.len()).unwrap_or(0) as i64;

                    if let Err(e) = repo
                        .record_processed(
                            path.to_str().unwrap_or("unknown"),
                            &checksum,
                            file_size,
                            magnet_link.as_deref(),
                        )
                        .await
                    {
                        debug!(
                            "Failed to record processed {} in database: {}",
                            file_type, e
                        );
                    } else {
                        debug!("Recorded {} in database: checksum {}", file_type, checksum);
                    }
                }

                Ok(ProcessOutcome::Uploaded)
            }
            Err(e) => {
                // Check if this is a "TRANSFER_ALREADY_ADDED" error
                let error_str = format!("{}", e);
                if error_str.contains("TRANSFER_ALREADY_ADDED") {
                    warn!("⚠️  Transfer already exists for {}, treating as success for archive/delete", path.display());

                    // Mark as processed
                    {
                        let mut processed = self.processed_files.lock().unwrap();
                        processed.insert(path.to_path_buf());
                    }

                    // Record in database if available (same logic as success case)
                    if let Some(db) = database {
                        let (checksum, magnet_link) = if self.is_magnet_file(path) {
                            match tokio::fs::read_to_string(path).await {
                                Ok(content) => {
                                    let magnet = content.trim().to_string();
                                    let checksum = calculate_string_checksum(&magnet);
                                    (checksum, Some(magnet))
                                }
                                Err(e) => {
                                    debug!(
                                        "Failed to re-read magnet file for database recording: {}",
                                        e
                                    );
                                    return Ok(ProcessOutcome::Uploaded); // Still consider it processed
                                }
                            }
                        } else {
                            match calculate_file_checksum(path).await {
                                Ok(checksum) => (checksum, None),
                                Err(e) => {
                                    debug!(
                                        "Failed to calculate checksum for database recording: {}",
                                        e
                                    );
                                    return Ok(ProcessOutcome::Uploaded); // Still consider it processed
                                }
                            }
                        };

                        let repo = db.processed_torrents();
                        let file_size = path.metadata().map(|m| m.len()).unwrap_or(0) as i64;

                        if let Err(e) = repo
                            .record_processed(
                                path.to_str().unwrap_or("unknown"),
                                &checksum,
                                file_size,
                                magnet_link.as_deref(),
                            )
                            .await
                        {
                            debug!(
                                "Failed to record processed {} in database: {}",
                                file_type, e
                            );
                        } else {
                            debug!("Recorded {} in database: checksum {}", file_type, checksum);
                        }
                    }

                    Ok(ProcessOutcome::Uploaded) // Treat as success for archive/delete
                } else {
                    error!("❌ Failed to upload {}: {}", path.display(), e);
                    Err(e.into())
                }
            }
        }
    }

    /// Check a watch file before upload (042 US4). `None` when the check is off for its
    /// folder, or a magnet has no info hash to look up.
    async fn precheck_file(
        &self,
        path: &Path,
        client: &PutioApiClient,
        parent_id: Option<i64>,
    ) -> Result<Option<(WatchCheck, Outcome)>> {
        let Some(precheck) = &self.precheck else {
            return Ok(None);
        };
        // Watching isn't recursive, so the file's directory is the `[watch].paths` entry
        let dir = path.parent().unwrap_or(Path::new(""));
        let folder = self.folder_path(client, parent_id).await?;
        let policy = precheck.policy(&folder, Key::Watch(dir));
        if !policy.enabled {
            return Ok(None);
        }
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let watch_folder = dir.display().to_string();

        if self.is_magnet_file(path) {
            let content = tokio::fs::read_to_string(path).await.unwrap_or_default();
            let Some((hash, name)) = from_magnet(content.trim()) else {
                debug!(path = %path.display(), "Magnet has no info hash; not checked");
                return Ok(None);
            };
            let source = PrecheckSource::WatchMagnet;
            let name = name.unwrap_or(stem);
            let limit = Duration::from_secs(precheck.config().lookup_timeout_secs);
            let found = precheck.lookup(&hash, limit).await;
            let lookup = Some(found.as_str());
            let (outcome, cache) = match found {
                Lookup::Found { meta, cache } => {
                    (precheck.check_meta(meta, &policy, source), Some(cache))
                }
                Lookup::Missed(missed) => {
                    precheck.unknown(&hash, &name, source, missed.as_str());
                    let outcome = Outcome::Unknown {
                        hash: hash.clone(),
                        name: Some(name.clone()),
                    };
                    (outcome, None)
                }
            };
            let check = WatchCheck {
                hash: Some(hash),
                name,
                source,
                watch_folder,
                lookup,
                cache,
                policy,
            };
            return Ok(Some((check, outcome)));
        }

        let bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("read {}", path.display()))?;
        let source = PrecheckSource::WatchTorrent;
        let (hash, name) = info_hash(&bytes).map_or((None, None), |(h, n)| (Some(h), n));
        let outcome = precheck.check(&bytes, &policy, source);
        let check = WatchCheck {
            hash,
            name: name.filter(|n| !n.trim().is_empty()).unwrap_or(stem),
            source,
            watch_folder,
            lookup: None,
            cache: None,
            policy,
        };
        Ok(Some((check, outcome)))
    }

    /// A watch file that failed the check: not uploaded, marked processed, recorded, and
    /// renamed `<file>.rejected` for the operator to see
    async fn reject_file(
        &self,
        path: &Path,
        check: &WatchCheck,
        meta: Option<Metainfo>,
        reason: Reason,
        database: Option<&Database>,
    ) -> ProcessOutcome {
        let message = reason.message(Stage::Before);
        let hash = check
            .hash
            .as_deref()
            .or(meta.as_ref().map(|m| m.hash.as_str()))
            .unwrap_or("");
        warn!(path = %path.display(), hash, reason = reason.key(), "Watch file rejected: {}", message);

        if let Some(db) = database {
            if let Some((checksum, magnet)) = self.content_checksum(path).await {
                let size = path.metadata().map(|m| m.len()).unwrap_or(0) as i64;
                if let Err(e) = db
                    .processed_torrents()
                    .record_processed(
                        path.to_str().unwrap_or("unknown"),
                        &checksum,
                        size,
                        magnet.as_deref(),
                    )
                    .await
                {
                    debug!("Failed to record rejected watch file in database: {}", e);
                }
            }
            record_rejection(db, check, meta.as_ref(), &reason).await;
        }

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let to = path.with_file_name(format!("{}.rejected", name));
        if let Err(e) = tokio::fs::rename(path, &to).await {
            warn!(path = %path.display(), error = %e, "Failed to rename rejected watch file");
            // Keep it from being checked again this run
            self.processed_files
                .lock()
                .unwrap()
                .insert(path.to_path_buf());
        }
        ProcessOutcome::Rejected(message)
    }

    /// The checksum a watch file is remembered by, and its magnet link
    async fn content_checksum(&self, path: &Path) -> Option<(String, Option<String>)> {
        if self.is_magnet_file(path) {
            let magnet = tokio::fs::read_to_string(path)
                .await
                .ok()?
                .trim()
                .to_string();
            Some((calculate_string_checksum(&magnet), Some(magnet)))
        } else {
            calculate_file_checksum(path).await.ok().map(|c| (c, None))
        }
    }

    /// The Put.io path of an upload folder, as 009 rules name folders. The root, and an
    /// unset `parent_id` (the account's default folder), are `""`.
    async fn folder_path(&self, client: &PutioApiClient, parent_id: Option<i64>) -> Result<String> {
        let Some(id) = parent_id.filter(|id| *id > 0) else {
            return Ok(String::new());
        };
        if let Some(path) = self.folder_paths.lock().unwrap().get(&id) {
            return Ok(path.clone());
        }
        let mut names = Vec::new();
        let mut next = Some(id);
        while let Some(current) = next.filter(|id| *id > 0) {
            if names.len() >= MAX_FOLDER_DEPTH {
                break;
            }
            let entry = client
                .get_file_details(current)
                .await
                .context("read the upload folder on Put.io")?
                .file;
            names.push(entry.name);
            next = entry.parent_id;
        }
        names.reverse();
        let path = names.join("/");
        self.folder_paths.lock().unwrap().insert(id, path.clone());
        Ok(path)
    }

    /// Check the watch magnets that reached Put.io unchecked, once their transfer
    /// finished (042 US4). Runs before the scan; the sync holds their files until then.
    async fn check_pending_on_putio(&self, client: &PutioApiClient, db: &Database) {
        let Some(precheck) = &self.precheck else {
            return;
        };
        let releases = db.releases();
        let pending = match releases.has_pending().await {
            Ok(false) => return,
            Ok(true) => releases.pending_checks().await,
            Err(e) => Err(e),
        };
        let pending: Vec<PendingCheck> = match pending {
            Ok(checks) => checks
                .into_iter()
                .filter(|c| c.source == PrecheckSource::WatchMagnet.as_str())
                .collect(),
            Err(e) => {
                warn!(error = %e, "Failed to list pending release checks");
                return;
            }
        };
        if pending.is_empty() {
            return;
        }
        let transfers: HashMap<i64, Transfer> = match client.list_transfers().await {
            Ok(listing) => listing.transfers.into_iter().map(|t| (t.id, t)).collect(),
            Err(e) => {
                warn!(error = %error_chain(&e), "Failed to list Put.io transfers for release checks");
                return;
            }
        };

        for check in &pending {
            match check.transfer_id.and_then(|id| transfers.get(&id)) {
                Some(transfer) if !putio_check::is_gone(transfer) => {
                    if let Some(file_id) = transfer
                        .file_id
                        .filter(|_| putio_check::is_finished(transfer))
                    {
                        self.check_transfer(precheck, client, db, check, transfer, file_id)
                            .await;
                    }
                }
                // Gone, in `ERROR` or cancelled: no stat counts it as fake
                _ => {
                    let gone = Reason::Gone.key();
                    match releases
                        .resolve_check(check.id, CheckState::Rejected, Some(gone), 0)
                        .await
                    {
                        Ok(_) => info!(
                            hash = ?check.hash,
                            name = %check.name,
                            "Release check on Put.io dropped: the transfer is gone"
                        ),
                        Err(e) => warn!(error = %e, "Failed to resolve release check"),
                    }
                }
            }
        }
    }

    /// Judge one finished watch magnet on Put.io, removing it there when it fails
    async fn check_transfer(
        &self,
        precheck: &Prechecker,
        client: &PutioApiClient,
        db: &Database,
        check: &PendingCheck,
        transfer: &Transfer,
        file_id: i64,
    ) {
        let releases = db.releases();
        let bytes = putio_check::putio_bytes(transfer);
        let folder = match self
            .folder_path(client, Some(transfer.save_parent_id))
            .await
        {
            Ok(folder) => folder,
            Err(e) => {
                debug!(error = %format!("{:#}", e), "Upload folder unreadable; checking next pass");
                return;
            }
        };
        let dir = Path::new(check.watch_folder.as_deref().unwrap_or(""));
        let policy = precheck.policy(&folder, Key::Watch(dir));
        let hash = check.hash.as_deref().unwrap_or("");
        let source = PrecheckSource::WatchMagnet;
        let judged = if policy.enabled {
            precheck
                .judge_on_putio(
                    client,
                    hash,
                    &check.name,
                    transfer,
                    file_id,
                    &policy,
                    source,
                )
                .await
        } else {
            // The check was turned off since the upload: let it through
            OnPutio::Passed
        };
        let resolved = match judged {
            OnPutio::Retry => return,
            OnPutio::Passed => {
                releases
                    .resolve_check(check.id, CheckState::Passed, None, bytes)
                    .await
            }
            OnPutio::Rejected { meta, reason } => {
                let resolved = releases
                    .resolve_check(check.id, CheckState::Rejected, Some(reason.key()), bytes)
                    .await;
                let total = meta.as_ref().map_or(0, Metainfo::total_size) as i64;
                let traits = meta.as_ref().map(|m| Traits::of(m, &policy));
                let types = traits.as_ref().map_or("[]".to_string(), Traits::types_json);
                let report = NewReport {
                    kind: ReportKind::Rejected,
                    check_id: Some(check.id),
                    file_id: Some(file_id),
                    transfer_id: Some(transfer.id),
                    source: Some(source.as_str()),
                    watch_folder: check.watch_folder.as_deref(),
                    stage: Some(Stage::Putio.as_str()),
                    name: &check.name,
                    hash: Some(hash),
                    total_size: Some(total),
                    file_count: traits.as_ref().map(|t| t.file_count as i64),
                    types: &types,
                    largest_wanted: traits.as_ref().map(|t| t.largest_wanted as i64),
                    min_size: Some(policy.min_size as i64),
                    reason: Some(reason.key()),
                    ..NewReport::default()
                };
                if let Err(e) = releases.insert_report(&report).await {
                    warn!(hash, error = %e, "Failed to record release report");
                }
                resolved
            }
        };
        if let Err(e) = resolved {
            warn!(hash, error = %e, "Failed to resolve release check");
        }
    }

    /// Find all .torrent and .magnet files in a directory (non-recursive)
    pub fn find_torrent_files(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        let mut torrent_files = Vec::new();

        debug!(
            "Searching for .torrent and .magnet files in: {}",
            dir.display()
        );

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && (self.is_torrent_file(&path) || self.is_magnet_file(&path)) {
                debug!("Found torrent/magnet file: {}", path.display());
                torrent_files.push(path);
            }
        }

        debug!(
            "Found {} torrent/magnet files in {}",
            torrent_files.len(),
            dir.display()
        );
        Ok(torrent_files)
    }

    /// Check if a path is a .torrent file
    pub fn is_torrent_file(&self, path: &Path) -> bool {
        path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("torrent")
    }

    /// Check if a path is a .magnet file
    pub fn is_magnet_file(&self, path: &Path) -> bool {
        path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("magnet")
    }

    /// Get access to processed files set
    pub fn processed_files(&self) -> Arc<Mutex<HashSet<PathBuf>>> {
        self.processed_files.clone()
    }

    /// Clear the processed files set
    pub fn clear_processed_files(&self) {
        let mut processed = self.processed_files.lock().unwrap();
        processed.clear()
    }

    /// Get count of processed files
    pub fn processed_files_count(&self) -> usize {
        let processed = self.processed_files.lock().unwrap();
        processed.len()
    }

    /// Format bytes in human readable format
    fn format_bytes(&self, bytes: i64) -> String {
        if bytes < 0 {
            return "Unknown".to_string();
        }

        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut unit_index = 0;

        while size >= 1024.0 && unit_index < UNITS.len() - 1 {
            size /= 1024.0;
            unit_index += 1;
        }

        if unit_index == 0 {
            format!("{} {}", bytes, UNITS[unit_index])
        } else {
            format!("{:.2} {}", size, UNITS[unit_index])
        }
    }

    /// Run `log` only the first time `path` is reported
    fn warn_once(&self, path: &Path, log: impl FnOnce(&Path)) {
        let first = self
            .warned
            .lock()
            .map(|mut warned| warned.insert(path.to_path_buf()))
            .unwrap_or(true);
        if first {
            log(path);
        }
    }

    /// Handle post-upload actions (archive or delete)
    ///
    /// Returns None when neither action is configured.
    async fn handle_post_upload_action(
        &self,
        file_path: &Path,
        archive_dir: Option<&PathBuf>,
        delete_after_upload: bool,
    ) -> Result<Option<PostUploadAction>> {
        if let Some(archive_path) = archive_dir {
            let filename = file_path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Invalid filename"))?;
            let archive_file_path = archive_path.join(filename);

            if let Some(parent) = archive_file_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("create {}", parent.display()))?;
            }
            tokio::fs::rename(file_path, &archive_file_path)
                .await
                .with_context(|| format!("move to {}", archive_file_path.display()))?;
            Ok(Some(PostUploadAction::Archived(archive_file_path)))
        } else if delete_after_upload {
            tokio::fs::remove_file(file_path).await.context("delete")?;
            Ok(Some(PostUploadAction::Deleted))
        } else {
            Ok(None)
        }
    }
}

impl Default for TorrentWatchCore {
    fn default() -> Self {
        Self::new()
    }
}

/// Calculate SHA-256 checksum of file content
pub async fn calculate_file_checksum(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};

    let content = tokio::fs::read(path).await?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}

/// Calculate SHA-256 checksum of a string
pub fn calculate_string_checksum(content: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}

/// Write the check row of an uploaded watch file: passed, or pending its check on Put.io
/// when a magnet missed the caches. A failure is logged: the upload stands.
async fn record_upload_check(
    db: &Database,
    check: &WatchCheck,
    outcome: &Outcome,
    transfer_id: i64,
) {
    let (state, total) = match outcome {
        Outcome::Passed(meta) => (
            Some(CheckState::Passed),
            meta.as_ref().map_or(0, Metainfo::total_size),
        ),
        _ => (None, 0),
    };
    let row = NewCheck {
        hash: check.hash.as_deref(),
        name: &check.name,
        source: check.source.as_str(),
        watch_folder: Some(&check.watch_folder),
        stage: Stage::Before.as_str(),
        state,
        lookup: check.lookup,
        lookup_cache: check.cache.as_deref(),
        transfer_id: Some(transfer_id),
        total_size: total as i64,
        ..NewCheck::default()
    };
    if let Err(e) = db.releases().insert_check(&row).await {
        warn!(hash = ?check.hash, error = %e, "Failed to record release check");
    }
}

/// Write the check row and report of a rejected watch file
async fn record_rejection(
    db: &Database,
    check: &WatchCheck,
    meta: Option<&Metainfo>,
    reason: &Reason,
) {
    let releases = db.releases();
    let hash = check.hash.as_deref().or(meta.map(|m| m.hash.as_str()));
    let total = meta.map_or(0, Metainfo::total_size) as i64;
    let row = NewCheck {
        hash,
        name: &check.name,
        source: check.source.as_str(),
        watch_folder: Some(&check.watch_folder),
        stage: Stage::Before.as_str(),
        state: Some(CheckState::Rejected),
        lookup: check.lookup,
        lookup_cache: check.cache.as_deref(),
        total_size: total,
        reason: Some(reason.key()),
        ..NewCheck::default()
    };
    let check_id = match releases.insert_check(&row).await {
        Ok(id) => Some(id),
        Err(e) => {
            warn!(hash = ?hash, error = %e, "Failed to record release check");
            None
        }
    };
    let traits = meta.map(|m| Traits::of(m, &check.policy));
    let types = traits.as_ref().map_or("[]".to_string(), Traits::types_json);
    let hosts = traits
        .as_ref()
        .map_or("[]".to_string(), Traits::tracker_hosts_json);
    let report = NewReport {
        kind: ReportKind::Rejected,
        check_id,
        source: Some(check.source.as_str()),
        watch_folder: Some(&check.watch_folder),
        stage: Some(Stage::Before.as_str()),
        name: &check.name,
        hash,
        total_size: Some(total),
        file_count: traits.as_ref().map(|t| t.file_count as i64),
        types: &types,
        largest_wanted: traits.as_ref().map(|t| t.largest_wanted as i64),
        min_size: Some(check.policy.min_size as i64),
        tracker_hosts: &hosts,
        is_private: meta.is_some_and(|m| m.private),
        reason: Some(reason.key()),
        ..NewReport::default()
    };
    if let Err(e) = releases.insert_report(&report).await {
        warn!(hash = ?hash, error = %e, "Failed to record release report");
    }
}

/// File watcher for .torrent and .magnet files (continuous daemon mode)
/// Configuration for file watcher (hot-reloadable)
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// Directories being watched
    pub watch_dirs: Vec<PathBuf>,
    /// Parent folder ID for transfers
    pub parent_id: Option<i64>,
    /// Check interval for polling mode
    pub interval: Duration,
    /// Archive directory for processed files
    pub archive_dir: Option<PathBuf>,
    /// Delete files after upload
    pub delete_after_upload: bool,
}

impl WatcherConfig {
    /// Create new watcher config
    pub fn new(watch_dirs: Vec<PathBuf>, parent_id: Option<i64>, interval_seconds: u64) -> Self {
        Self {
            watch_dirs,
            parent_id,
            interval: Duration::from_secs(interval_seconds),
            archive_dir: None,
            delete_after_upload: false,
        }
    }
}

pub struct TorrentWatcher {
    /// Shared configuration (hot-reloadable)
    config: Arc<StdRwLock<WatcherConfig>>,
    /// Core watch functionality
    core: TorrentWatchCore,
}

impl TorrentWatcher {
    /// Create a new torrent watcher
    ///
    /// # Arguments
    ///
    /// * `watch_dirs` - Directories to watch
    /// * `parent_id` - Optional parent folder ID for transfers
    /// * `interval_seconds` - Check interval in seconds
    pub fn new(watch_dirs: Vec<PathBuf>, parent_id: Option<i64>, interval_seconds: u64) -> Self {
        let config = WatcherConfig::new(watch_dirs, parent_id, interval_seconds);
        Self {
            config: Arc::new(StdRwLock::new(config)),
            core: TorrentWatchCore::new(),
        }
    }

    /// Create watcher from existing config
    pub fn from_config(config: WatcherConfig) -> Self {
        Self {
            config: Arc::new(StdRwLock::new(config)),
            core: TorrentWatchCore::new(),
        }
    }

    /// Check watch files with the release check (042 US4)
    pub fn with_precheck(mut self, precheck: Arc<Prechecker>) -> Self {
        self.core = self.core.with_precheck(precheck);
        self
    }

    /// Update watcher configuration dynamically (hot reload)
    ///
    /// This allows changing configuration without restarting the watcher.
    /// Changes take effect on the next scan cycle:
    /// - Watch directories (will scan new directories, stop watching removed ones)
    /// - Parent folder ID (new uploads use new parent)
    /// - Check interval (next sleep uses new interval)
    /// - Archive/delete settings (applied to next processed file)
    ///
    /// # Arguments
    ///
    /// * `new_config` - New configuration to apply
    pub fn update_config(&self, new_config: WatcherConfig) {
        let mut config = self.config.write().unwrap();
        *config = new_config;
        info!("File watcher configuration updated via hot reload");
        info!("  Watch directories: {:?}", config.watch_dirs);
        info!("  Parent folder ID: {:?}", config.parent_id);
        info!("  Check interval: {:?}", config.interval);
        info!("  Archive directory: {:?}", config.archive_dir);
        info!("  Delete after upload: {}", config.delete_after_upload);
    }

    /// Get current configuration (read-only snapshot)
    pub fn get_config(&self) -> WatcherConfig {
        self.config.read().unwrap().clone()
    }

    /// Get runtime statistics
    ///
    /// Returns current watcher statistics including processed files count
    /// and current configuration.
    pub fn get_stats(&self) -> WatcherRuntimeStats {
        let config = self.config.read().unwrap();

        WatcherRuntimeStats {
            total_processed: self.core.processed_files_count(),
            watch_directories: config.watch_dirs.clone(),
            parent_folder_id: config.parent_id,
            interval_seconds: config.interval.as_secs(),
            archive_directory: config.archive_dir.clone(),
            delete_after_upload: config.delete_after_upload,
        }
    }

    /// Perform a single scan check (for use by unified daemon)
    ///
    /// This is a non-blocking operation that scans all watch directories once
    /// and processes any new .torrent/.magnet files found.
    pub async fn scan_once(
        &self,
        client: &PutioApiClient,
        delete_after_upload: bool,
        database: Option<&Database>,
    ) -> Result<WatchStats> {
        // Snapshot config for consistency during scan
        let config = self.config.read().unwrap().clone();

        self.core
            .scan_directories(
                &config.watch_dirs,
                client,
                config.parent_id,
                config.archive_dir.as_ref(),
                delete_after_upload,
                database,
            )
            .await
    }

    /// Run the watcher in daemon mode
    ///
    /// Watches directories for new .torrent/.magnet files and automatically uploads them.
    /// Runs until interrupted with Ctrl+C.
    pub async fn run_daemon(&self, client: &PutioApiClient) -> Result<()> {
        // Read initial config for logging
        let initial_config = self.config.read().unwrap().clone();

        info!("Starting torrent watcher daemon");
        info!("Watching directories: {:?}", initial_config.watch_dirs);
        info!("Check interval: {:?}", initial_config.interval);

        // Validate watch directories
        for dir in &initial_config.watch_dirs {
            if !dir.exists() {
                warn!("Watch directory does not exist: {}", dir.display());
            } else if !dir.is_dir() {
                warn!("Path is not a directory: {}", dir.display());
            } else {
                info!("Watching: {}", dir.display());
            }
        }

        // Set up Ctrl+C handler
        let running = Arc::new(Mutex::new(true));
        let r = running.clone();

        ctrlc::set_handler(move || {
            info!("Received Ctrl+C, shutting down gracefully...");
            *r.lock().unwrap() = false;
        })
        .context("Failed to set Ctrl+C handler")?;

        // Create filesystem watcher
        let (tx, rx) = channel();
        let mut watcher = self.create_watcher(tx)?;

        // Watch all directories
        for dir in &initial_config.watch_dirs {
            if dir.exists() && dir.is_dir() {
                watcher
                    .watch(dir, RecursiveMode::NonRecursive)
                    .with_context(|| format!("Failed to watch directory: {}", dir.display()))?;
                debug!("Started watching: {}", dir.display());
            }
        }

        // Process initial files
        info!("Processing existing .torrent/.magnet files...");
        self.process_existing_files(client).await?;

        // Main watch loop
        let mut cycle = 0;
        while *running.lock().unwrap() {
            cycle += 1;
            debug!("Daemon cycle {}", cycle);

            // Process filesystem events
            self.process_events(&rx, client).await?;

            // Re-read config each cycle to pick up hot reload changes (especially interval)
            let current_config = self.config.read().unwrap().clone();

            // Sleep for the interval, but check running flag every second
            for _ in 0..current_config.interval.as_secs() {
                if !*running.lock().unwrap() {
                    break;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }

        info!("Torrent watcher daemon stopped");
        Ok(())
    }

    /// Create a filesystem watcher
    fn create_watcher(
        &self,
        tx: Sender<notify::Result<Event>>,
    ) -> Result<notify::RecommendedWatcher> {
        let watcher =
            notify::recommended_watcher(tx).context("Failed to create filesystem watcher")?;
        Ok(watcher)
    }

    /// Process filesystem events
    async fn process_events(
        &self,
        rx: &Receiver<notify::Result<Event>>,
        client: &PutioApiClient,
    ) -> Result<()> {
        // Process all available events (non-blocking)
        while let Ok(result) = rx.try_recv() {
            match result {
                Ok(event) => {
                    if let Err(e) = self.handle_event(event, client).await {
                        error!("Error handling event: {}", e);
                    }
                }
                Err(e) => {
                    error!("Filesystem watch error: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Handle a filesystem event
    async fn handle_event(&self, event: Event, client: &PutioApiClient) -> Result<()> {
        debug!("Filesystem event: {:?}", event.kind);

        // Only process Create and Modify events
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                for path in event.paths {
                    if self.is_torrent_file(&path) || self.is_magnet_file(&path) {
                        self.process_file(&path, client).await?;
                    }
                }
            }
            _ => {
                // Ignore other events (delete, access, etc.)
            }
        }

        Ok(())
    }

    /// Process existing .torrent and .magnet files in watch directories
    async fn process_existing_files(&self, client: &PutioApiClient) -> Result<()> {
        // Snapshot config for consistency during initial scan
        let config = self.config.read().unwrap().clone();

        let stats = self
            .core
            .scan_directories(
                &config.watch_dirs,
                client,
                config.parent_id,
                config.archive_dir.as_ref(),
                config.delete_after_upload,
                None,
            )
            .await?;

        info!(
            "Initial scan completed - {} files processed, {} skipped, {} failed",
            stats.files_processed, stats.files_skipped, stats.files_failed
        );

        Ok(())
    }

    /// Process a single .torrent or .magnet file
    async fn process_file(&self, path: &Path, client: &PutioApiClient) -> Result<()> {
        // Snapshot config for parent_id
        let config = self.config.read().unwrap().clone();

        match self
            .core
            .process_torrent_file(path, client, config.parent_id, None)
            .await
        {
            Ok(ProcessOutcome::Invalid(reason)) => {
                self.core.warn_once(path, |p| {
                    watch_metrics::invalid();
                    warn!(path = %p.display(), reason = %reason, "Invalid watch file");
                });
                Ok(())
            }
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Check if a path is a .torrent file
    fn is_torrent_file(&self, path: &Path) -> bool {
        self.core.is_torrent_file(path)
    }

    /// Check if a path is a .magnet file
    fn is_magnet_file(&self, path: &Path) -> bool {
        self.core.is_magnet_file(path)
    }
}
