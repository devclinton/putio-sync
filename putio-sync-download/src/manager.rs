//! Download manager for orchestrating downloads

use crate::error::{DownloadError, DownloadResult};
use crate::files::{check_resume, cleanup_partial, create_temp_path};
use crate::progress::{DownloadProgress, OverallProgress, ProgressCallback, ProgressTracker};
use crate::stream::stream_download;
use crate::utils::retry_with_stall_detection;
use putio_sync_client::PutioApiClient;
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

/// Download manager
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
