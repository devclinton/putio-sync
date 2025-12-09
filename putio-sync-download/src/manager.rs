//! Download manager for orchestrating downloads

use crate::error::{DownloadError, DownloadResult};
use crate::files::{check_resume, cleanup_partial, create_temp_path};
use crate::progress::{DownloadProgress, OverallProgress, ProgressCallback, ProgressTracker};
use crate::stream::stream_download;
use crate::utils::retry_with_stall_detection;
use putio_sync_client::{FileEntry, PutioApiClient};
use putio_sync_db::models::DownloadStatus;
use putio_sync_db::Database;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Configuration for download operations
#[derive(Debug, Clone)]
pub struct DownloadConfig {
    /// Target directory for downloads
    pub target_directory: PathBuf,
    /// Skip files that already exist
    pub skip_existing: bool,
    /// Buffer size in megabytes
    pub buffer_size_mb: usize,
    /// Bandwidth limit in bytes per second (None = unlimited)
    pub bandwidth_limit_bytes_per_sec: Option<u64>,
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay for retries in seconds
    pub retry_initial_delay_secs: u64,
    /// Number of concurrent download workers
    pub workers: usize,
    /// Delete files from Put.io after successful download
    pub delete_after_download: bool,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            target_directory: PathBuf::from("."),
            skip_existing: false,
            buffer_size_mb: 64,
            bandwidth_limit_bytes_per_sec: None,
            max_retries: 3,
            retry_initial_delay_secs: 2,
            workers: 1,
            delete_after_download: false,
        }
    }
}

/// Sanitize a filename to remove invalid characters
fn sanitize_filename(filename: &str) -> String {
    filename
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect()
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
}

/// Download manager
#[derive(Clone)]
pub struct DownloadManager {
    /// Put.io API client
    client: Arc<PutioApiClient>,
    /// Database (optional for fallback mode)
    database: Option<Arc<Database>>,
    /// Download configuration
    config: DownloadConfig,
    /// Progress trackers for active downloads
    progress_trackers: Arc<RwLock<HashMap<i64, ProgressTracker>>>,
    /// Overall progress
    overall_progress: Arc<RwLock<OverallProgress>>,
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
            config,
            progress_trackers: Arc::new(RwLock::new(HashMap::new())),
            overall_progress,
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
            config,
            progress_trackers: Arc::new(RwLock::new(HashMap::new())),
            overall_progress,
        }
    }

    /// Download a single file
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
        let file_id = file.file_id;
        let filename = sanitize_filename(&file.filename);

        debug!("Starting download: {} (ID: {})", filename, file_id);
        debug!(
            "Download config target_directory: {}",
            self.config.target_directory.display()
        );
        debug!("File relative_path: '{}'", file.relative_path);

        // Notify callback that file download is starting
        if let Some(ref callback) = progress_callback {
            callback.on_file_start(file_id, &filename, file.file_size);
        }

        // Construct full path preserving directory structure
        let target_path = if file.relative_path.is_empty() {
            self.config.target_directory.join(&filename)
        } else {
            self.config
                .target_directory
                .join(&file.relative_path)
                .join(&filename)
        };
        debug!("Constructed target_path: {}", target_path.display());

        // Ensure parent directory exists
        if let Some(parent) = target_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(DownloadError::IoError)?;
            }
        }

        if self.config.skip_existing && target_path.exists() {
            info!("Skipping existing file: {}", filename);
            return Ok(DownloadOperationResult {
                file_id,
                success: true,
                error: None,
                bytes_downloaded: 0,
                file_path: Some(target_path),
            });
        }

        // Check for resume capability
        // Create temporary download path
        let temp_path = create_temp_path(&target_path)?;

        let resume_bytes = check_resume(&temp_path, file.file_size).await?;

        // Download the file using streaming with range requests for resuming if available
        let result = if let Some(resume_from) = resume_bytes {
            // Try to resume using range requests
            info!(
                "Attempting to resume download from byte {} of {}",
                resume_from, file.file_size
            );
            self.download_streaming_with_resume(file, progress_callback.clone(), resume_from)
                .await
        } else {
            // Start new download
            debug!("Starting new streaming download");
            self.download_streaming(file, progress_callback.clone())
                .await
        };

        // Handle result
        match result {
            Ok(result) => {
                if let Some(ref callback) = progress_callback {
                    callback.on_file_complete(file_id, &filename, result.success);
                }
                Ok(result)
            }
            Err(e) => {
                error!("Download failed for {}: {}", filename, e);
                if let Some(ref callback) = progress_callback {
                    callback.on_file_complete(file_id, &filename, false);
                }
                Err(e)
            }
        }
    }

    /// Download using streaming with resume support
    async fn download_streaming_with_resume(
        &self,
        file: DownloadFile,
        progress_callback: Option<Arc<dyn ProgressCallback>>,
        resume_from_byte: u64,
    ) -> DownloadResult<DownloadOperationResult> {
        let filename = sanitize_filename(&file.filename);
        let target_path = if file.relative_path.is_empty() {
            self.config.target_directory.join(&filename)
        } else {
            self.config
                .target_directory
                .join(&file.relative_path)
                .join(&filename)
        };

        // Create progress tracker
        self.progress_trackers
            .write()
            .await
            .insert(file.file_id, ProgressTracker::new(resume_from_byte));

        // Start download with retry logic
        let start_time = Instant::now();

        let client = &self.client;
        let download_url = &file.download_url;
        let file_size = file.file_size;
        let buffer_size_mb = self.config.buffer_size_mb;
        let bandwidth_limit = self.config.bandwidth_limit_bytes_per_sec;
        let max_retries = self.config.max_retries;
        let initial_delay = self.config.retry_initial_delay_secs;

        let result = retry_with_stall_detection(
            || async {
                stream_download(
                    client,
                    download_url,
                    &target_path,
                    &filename,
                    Some(file_size),
                    buffer_size_mb,
                    bandwidth_limit,
                    Some(resume_from_byte), // Resume from the specified byte
                )
                .await
            },
            max_retries,
            initial_delay,
            &format!("Resume download {}", filename),
        )
        .await;

        match result {
            Ok(retry_result) => {
                let duration = start_time.elapsed();
                let download_result = retry_result.value;
                let retry_count = retry_result.attempts;

                // Calculate total bytes (existing + newly downloaded)
                let total_bytes_downloaded = resume_from_byte + download_result.bytes_downloaded;

                // Update progress
                if let Some(ref callback) = progress_callback {
                    let mut progress =
                        DownloadProgress::new(file.file_id, filename.clone(), file.file_size);
                    progress.bytes_downloaded = total_bytes_downloaded;
                    progress.complete = true;
                    callback.on_file_progress(&progress);
                }

                // Save to database with retry count (if database available)
                if let Some(database) = &self.database {
                    let repo = database.downloads();
                    if let Err(e) = repo
                        .insert_with_retry_count(
                            file.file_id,
                            &filename,
                            file.file_size as i64,
                            target_path.to_str().unwrap_or(""),
                            DownloadStatus::Completed,
                            retry_count as i64,
                        )
                        .await
                    {
                        warn!("Database error (non-fatal): Failed to record download completion for {}: {}. Download succeeded.", filename, e);
                    }
                } else {
                    debug!(
                        "Database not available - skipping download record for {}",
                        filename
                    );
                }

                info!(
                    "Resume download completed: {} (total {} bytes in {:.2}s)",
                    filename,
                    total_bytes_downloaded,
                    duration.as_secs_f64()
                );

                Ok(DownloadOperationResult {
                    file_id: file.file_id,
                    success: true,
                    error: None,
                    bytes_downloaded: total_bytes_downloaded,
                    file_path: Some(target_path),
                })
            }
            Err(e) => {
                // Check if it's a 404 error
                if let DownloadError::ApiError(ref msg) = e {
                    if msg.contains("404") {
                        warn!("File no longer available (404): {}", filename);
                        // Clean up any partial download
                        let temp_path = create_temp_path(&target_path)?;
                        cleanup_partial(&temp_path).await;
                    }
                }

                Ok(DownloadOperationResult {
                    file_id: file.file_id,
                    success: false,
                    error: Some(e.to_string()),
                    bytes_downloaded: resume_from_byte, // Return at least the existing bytes
                    file_path: None,
                })
            }
        }
    }

    /// Download using streaming (for small files)
    async fn download_streaming(
        &self,
        file: DownloadFile,
        progress_callback: Option<Arc<dyn ProgressCallback>>,
    ) -> DownloadResult<DownloadOperationResult> {
        let filename = sanitize_filename(&file.filename);
        let target_path = if file.relative_path.is_empty() {
            self.config.target_directory.join(&filename)
        } else {
            self.config
                .target_directory
                .join(&file.relative_path)
                .join(&filename)
        };

        // Create progress tracker
        self.progress_trackers
            .write()
            .await
            .insert(file.file_id, ProgressTracker::new(0));

        // Start download with retry logic
        let start_time = Instant::now();

        let client = &self.client;
        let download_url = &file.download_url;
        let file_size = file.file_size;
        let buffer_size_mb = self.config.buffer_size_mb;
        let bandwidth_limit = self.config.bandwidth_limit_bytes_per_sec;
        let max_retries = self.config.max_retries;
        let initial_delay = self.config.retry_initial_delay_secs;

        let result = retry_with_stall_detection(
            || async {
                stream_download(
                    client,
                    download_url,
                    &target_path,
                    &filename,
                    Some(file_size),
                    buffer_size_mb,
                    bandwidth_limit,
                    None, // No resume for new downloads
                )
                .await
            },
            max_retries,
            initial_delay,
            &format!("Download {}", filename),
        )
        .await;

        match result {
            Ok(retry_result) => {
                let duration = start_time.elapsed();
                let download_result = retry_result.value;
                let retry_count = retry_result.attempts;

                // Update progress
                if let Some(ref callback) = progress_callback {
                    let mut progress =
                        DownloadProgress::new(file.file_id, filename.clone(), file.file_size);
                    progress.bytes_downloaded = download_result.bytes_downloaded;
                    progress.complete = true;
                    callback.on_file_progress(&progress);
                }

                // Save to database with retry count (if database available)
                if let Some(database) = &self.database {
                    let repo = database.downloads();
                    if let Err(e) = repo
                        .insert_with_retry_count(
                            file.file_id,
                            &filename,
                            file.file_size as i64,
                            target_path.to_str().unwrap_or(""),
                            DownloadStatus::Completed,
                            retry_count as i64,
                        )
                        .await
                    {
                        warn!("Database error (non-fatal): Failed to record download completion for {}: {}. Download succeeded.", filename, e);
                    }
                } else {
                    debug!(
                        "Database not available - skipping download record for {}",
                        filename
                    );
                }

                info!(
                    "Download completed: {} ({} bytes in {:.2}s)",
                    filename,
                    download_result.bytes_downloaded,
                    duration.as_secs_f64()
                );

                Ok(DownloadOperationResult {
                    file_id: file.file_id,
                    success: true,
                    error: None,
                    bytes_downloaded: download_result.bytes_downloaded,
                    file_path: Some(target_path),
                })
            }
            Err(e) => {
                // Check if it's a 404 error
                if let DownloadError::ApiError(ref msg) = e {
                    if msg.contains("404") {
                        warn!("File no longer available (404): {}", filename);
                        // Clean up any partial download
                        cleanup_partial(&target_path).await;
                    }
                }

                Ok(DownloadOperationResult {
                    file_id: file.file_id,
                    success: false,
                    error: Some(e.to_string()),
                    bytes_downloaded: 0,
                    file_path: None,
                })
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
        let mut stats = SyncStats::default();

        info!("Starting download sync...");

        // Require database for queue-based downloads
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for sync downloads".to_string())
        })?;

        // List all files from Put.io (starting from root folder 0)
        let files_with_paths = match self.list_all_files(0).await {
            Ok(files) => files,
            Err(e) => {
                error!("Failed to list files from Put.io: {}", e);
                return Err(e);
            }
        };

        // Filter to only actual files (not folders)
        let downloadable_files: Vec<_> = files_with_paths
            .into_iter()
            .filter(|(f, _)| f.file_type != "FOLDER")
            .collect();

        stats.total_files = downloadable_files.len();

        if stats.total_files == 0 {
            debug!("No files found on Put.io");
            return Ok(stats);
        }

        info!("Found {} files on Put.io", stats.total_files);

        let downloads_repo = db.downloads();
        let queue_repo = db.queue();

        // Check which files need to be added to queue
        for (file, relative_path) in downloadable_files {
            // Check if already downloaded
            match downloads_repo.find_by_file_id(file.id).await {
                Ok(Some(record)) if record.status == "completed" => {
                    debug!("Skipping already downloaded file: {} (ID: {})", file.name, file.id);
                    stats.files_skipped += 1;
                    continue;
                }
                Ok(_) => {
                    // Not downloaded or incomplete - continue to queue check
                }
                Err(e) => {
                    warn!("Database error checking downloads for file {}: {}", file.id, e);
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
            debug!("Adding to queue: {} ({} bytes) at path '{}'", file.name, file.size, relative_path);

            let download_url = format!("https://api.put.io/v2/files/{}/download", file.id);

            match queue_repo.add(file.id, &file.name, file.size, &download_url, &relative_path, 0).await {
                Ok(_) => {
                    stats.files_downloaded += 1; // Renamed but means "added to queue"
                    debug!("Added to queue: {}", file.name);
                }
                Err(e) => {
                    stats.files_failed += 1;
                    error!("Failed to add {} to queue: {}", file.name, e);
                }
            }
        }

        info!(
            "Sync complete - Queued: {}, Skipped: {}, Failed: {}",
            stats.files_downloaded, stats.files_skipped, stats.files_failed
        );

        Ok(stats)
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
        // Require database for queue-based downloads
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for queue processing".to_string())
        })?;

        let queue_repo = db.queue();

        // Atomically claim next pending item (prevents race conditions between workers)
        let queue_item = queue_repo.claim_next_pending().await
            .map_err(|e| DownloadError::InternalError(format!("Failed to claim next pending item: {}", e)))?;

        let queue_item = match queue_item {
            Some(item) => item,
            None => {
                debug!("Queue is empty");
                return Ok(false);
            }
        };

        let file_id = queue_item.file_id;

        info!("Processing queue item: {} (ID: {})", queue_item.filename, file_id);

        // Verify file still exists on Put.io and get current info
        match self.client.get_file_details(file_id).await {
            Ok(file_response) => {
                let file_info = file_response.file;
                info!("File verified on Put.io: {} ({} bytes)", file_info.name, file_info.size);

                // Download the file using the stored relative path
                let download_file = DownloadFile {
                    file_id,
                    filename: file_info.name.clone(),
                    relative_path: queue_item.relative_path.clone(),
                    file_size: file_info.size as u64,
                    download_url: format!("https://api.put.io/v2/files/{}/download", file_id),
                };

                match self.download_file(download_file, None).await {
                    Ok(result) if result.success => {
                        info!("Successfully downloaded: {}", file_info.name);

                        // Delete from Put.io if configured
                        if self.config.delete_after_download {
                            info!("Deleting file from Put.io: {} (ID: {})", file_info.name, file_id);
                            match self.client.delete_files(vec![file_id]).await {
                                Ok(_) => {
                                    info!("Successfully deleted from Put.io: {}", file_info.name);
                                }
                                Err(e) => {
                                    warn!("Failed to delete file {} from Put.io: {}", file_info.name, e);
                                }
                            }
                        }

                        // Update queue status to completed and remove
                        if let Err(e) = queue_repo.update_status(file_id, putio_sync_db::models::QueueStatus::Completed).await {
                            warn!("Failed to update queue status to completed: {}", e);
                        }

                        if let Err(e) = queue_repo.remove(file_id).await {
                            warn!("Failed to remove completed item from queue: {}", e);
                        }

                        Ok(true)
                    }
                    Ok(_) | Err(_) => {
                        error!("Failed to download: {}", file_info.name);

                        // Update queue status to failed
                        if let Err(e) = queue_repo.update_status(file_id, putio_sync_db::models::QueueStatus::Failed).await {
                            warn!("Failed to update queue status to failed: {}", e);
                        }

                        Ok(true) // Still processed, even if failed
                    }
                }
            }
            Err(e) => {
                warn!("File {} no longer exists on Put.io or is inaccessible: {}", file_id, e);

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
        info!("Starting Put.io sync loop with interval: {}s", interval_secs);
        let interval = std::time::Duration::from_secs(interval_secs);
        let mut cycle = 0;

        loop {
            cycle += 1;
            debug!("Sync loop cycle #{}", cycle);

            match self.sync_downloads().await {
                Ok(stats) => {
                    if stats.files_downloaded > 0 {
                        info!(
                            "Sync completed - Queued: {}, Skipped: {}, Failed: {}",
                            stats.files_downloaded, stats.files_skipped, stats.files_failed
                        );
                    } else {
                        debug!("Sync completed - no new files");
                    }
                }
                Err(e) => {
                    error!("Error syncing from Put.io: {}", e);
                }
            }

            tokio::time::sleep(interval).await;
        }
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
            let pending_items = queue_repo.get_pending().await
                .map_err(|e| DownloadError::InternalError(format!("Failed to check queue: {}", e)))?;

            if pending_items.is_empty() {
                debug!("Queue is empty, nothing to process");
                return Ok(0);
            }
        }

        // Spawn concurrent workers
        let num_workers = self.config.workers;
        info!("Starting {} concurrent download workers", num_workers);

        let mut handles = Vec::new();

        for worker_id in 0..num_workers {
            // Clone self for the worker task
            let client = self.client.clone();
            let database = self.database.clone();
            let config = self.config.clone();

            let handle = tokio::spawn(async move {
                let mut count = 0;

                // Create a DownloadManager for this worker
                let worker_dm = if let Some(db) = database {
                    DownloadManager::new(client, db, config)
                } else {
                    DownloadManager::new_without_database(client, config)
                };

                debug!("Worker {} starting", worker_id);

                // Process items until queue is empty
                loop {
                    match worker_dm.process_queue().await {
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
            info!("Worker cycle completed - {} workers processed {} items total", num_workers, total_processed);
        }

        Ok(total_processed)
    }

    /// List all files recursively from Put.io
    /// Returns a list of (FileEntry, relative_path) tuples
    async fn list_all_files(&self, parent_id: i64) -> DownloadResult<Vec<(FileEntry, String)>> {
        let mut all_files = Vec::new();
        // Track (folder_id, relative_path) for each folder to process
        let mut folders_to_process = vec![(parent_id, String::new())];
        // Map folder_id -> folder_name for building paths
        let mut folder_names = std::collections::HashMap::new();

        while let Some((current_folder, current_path)) = folders_to_process.pop() {
            debug!("Fetching files from folder: {} (path: {})", current_folder, current_path);

            let response = self
                .client
                .list_files(current_folder)
                .await
                .map_err(|e| DownloadError::ApiError(e.to_string()))?;

            for file in response.files {
                if file.file_type == "FOLDER" {
                    // Build the folder's relative path
                    let folder_path = if current_path.is_empty() {
                        sanitize_filename(&file.name)
                    } else {
                        format!("{}/{}", current_path, sanitize_filename(&file.name))
                    };

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
