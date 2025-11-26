//! Download manager for orchestrating downloads

use super::chunk::ResumeInfo;
use super::progress::{DownloadProgress, OverallProgress, ProgressCallback, ProgressTracker};
use super::stream::{is_partial_download, stream_download};
use crate::api::ApiClient;
use crate::config::DownloadConfig;
use crate::db::models::DownloadStatus;
use crate::db::Database;
use crate::error::{CoreError, CoreResult};
use crate::fs::sanitize::sanitize_filename;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

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
    client: Arc<ApiClient>,
    /// Database
    database: Arc<Database>,
    /// Download configuration
    config: DownloadConfig,
    /// Progress trackers for active downloads
    progress_trackers: Arc<RwLock<HashMap<i64, ProgressTracker>>>,
    /// Overall progress
    overall_progress: Arc<RwLock<OverallProgress>>,
}

/// Result of retry operation with attempt count
struct RetryResult<T> {
    value: T,
    attempts: u32,
}


/// Retry a future with special handling for stalled downloads
///
/// This function is similar to retry_with_backoff but adds special handling
/// for stalled downloads with shorter delays to restart quickly.
///
/// # Arguments
///
/// * `operation` - The async operation to retry
/// * `max_retries` - Maximum number of retry attempts
/// * `initial_delay` - Initial delay in seconds
/// * `operation_name` - Name of the operation for logging
///
/// # Returns
///
/// Returns the result along with the number of attempts made
async fn retry_with_stall_detection<F, Fut, T>(
    mut operation: F,
    max_retries: u32,
    initial_delay: u64,
    operation_name: &str,
) -> CoreResult<RetryResult<T>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = CoreResult<T>>,
{
    let mut attempt = 0;

    loop {
        match operation().await {
            Ok(result) => {
                if attempt > 0 {
                    info!(
                        "{} succeeded after {} retry attempt(s)",
                        operation_name, attempt
                    );
                }
                return Ok(RetryResult {
                    value: result,
                    attempts: attempt,
                });
            }
            Err(e) => {
                attempt += 1;
                if attempt > max_retries {
                    error!(
                        "{} failed after {} attempts: {}",
                        operation_name, max_retries, e
                    );
                    return Err(e);
                }

                // Check if this is a stalled download error
                let is_stall_error = e.to_string().contains("Download stalled");
                
                // Use shorter delay for stalled downloads, longer for other errors
                let delay = if is_stall_error {
                    Duration::from_secs(3) // 3 second delay for stalled downloads
                } else {
                    Duration::from_secs(initial_delay * (1 << (attempt - 1).min(4))) // Exponential backoff for other errors
                };

                if is_stall_error {
                    warn!(
                        "{} stalled (attempt {}/{}): {}. Restarting download in {}s...",
                        operation_name,
                        attempt,
                        max_retries,
                        e,
                        delay.as_secs()
                    );
                } else {
                    warn!(
                        "{} failed (attempt {}/{}): {}. Retrying in {:.1}s...",
                        operation_name,
                        attempt,
                        max_retries,
                        e,
                        delay.as_secs_f64()
                    );
                }

                tokio::time::sleep(delay).await;
            }
        }
    }
}

impl DownloadManager {
    /// Create a new download manager
    ///
    /// # Arguments
    ///
    /// * `client` - Put.io API client
    /// * `database` - Database for tracking downloads
    /// * `config` - Download configuration
    pub fn new(client: Arc<ApiClient>, database: Arc<Database>, config: DownloadConfig) -> Self {
        let overall_progress = Arc::new(RwLock::new(OverallProgress::new(0, 0)));

        Self {
            client,
            database,
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
    ) -> CoreResult<DownloadOperationResult> {
        let file_id = file.file_id;
        let filename = sanitize_filename(&file.filename);

        info!("Starting download: {} (ID: {})", filename, file_id);
        info!("Download config target_directory: {}", self.config.target_directory.display());
        info!("File relative_path: '{}'", file.relative_path);

        // Notify callback that file download is starting
        if let Some(ref callback) = progress_callback {
            callback.on_file_start(file_id, &filename, file.file_size);
        }

        // Construct full path preserving directory structure
        let target_path = if file.relative_path.is_empty() {
            self.config.target_directory.join(&filename)
        } else {
            self.config.target_directory.join(&file.relative_path).join(&filename)
        };
        info!("Constructed target_path: {}", target_path.display());
        
        // Ensure parent directory exists
        if let Some(parent) = target_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    CoreError::IoError(e)
                })?;
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
        let resume_info = self.check_resume(&file, &target_path).await?;

        // Download the file using streaming with range requests for resuming if available
        let result = if let Some(ref resume) = resume_info {
            // Try to resume using range requests
            info!("Attempting to resume download from byte {} of {}", resume.bytes_downloaded, resume.total_bytes);
            self.download_streaming_with_resume(file, progress_callback.clone(), resume.bytes_downloaded).await
        } else {
            // Start new download
            info!("Starting new streaming download");
            self.download_streaming(file, progress_callback.clone()).await
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
    ) -> CoreResult<DownloadOperationResult> {
        let filename = sanitize_filename(&file.filename);
        let target_path = if file.relative_path.is_empty() {
            self.config.target_directory.join(&filename)
        } else {
            self.config.target_directory.join(&file.relative_path).join(&filename)
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
                    let mut progress = DownloadProgress::new(
                        file.file_id,
                        filename.clone(),
                        file.file_size,
                    );
                    progress.bytes_downloaded = total_bytes_downloaded;
                    progress.complete = true;
                    callback.on_file_progress(&progress);
                }

                // Save to database with retry count
                let repo = self.database.downloads();
                repo.insert_with_retry_count(
                    file.file_id,
                    &filename,
                    file.file_size as i64,
                    target_path.to_str().unwrap_or(""),
                    DownloadStatus::Completed,
                    retry_count as i64,
                )
                .await?;

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
                if let CoreError::ApiError(ref msg) = e {
                    if msg.contains("404") {
                        warn!("File no longer available (404): {}", filename);
                        // Clean up any partial download
                        let temp_path = self.create_temp_path(&target_path)?;
                        self.cleanup_partial(&temp_path).await;
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
    ) -> CoreResult<DownloadOperationResult> {
        let filename = sanitize_filename(&file.filename);
        let target_path = if file.relative_path.is_empty() {
            self.config.target_directory.join(&filename)
        } else {
            self.config.target_directory.join(&file.relative_path).join(&filename)
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
                    let mut progress = DownloadProgress::new(
                        file.file_id,
                        filename.clone(),
                        file.file_size,
                    );
                    progress.bytes_downloaded = download_result.bytes_downloaded;
                    progress.complete = true;
                    callback.on_file_progress(&progress);
                }

                // Save to database with retry count
                let repo = self.database.downloads();
                repo.insert_with_retry_count(
                    file.file_id,
                    &filename,
                    file.file_size as i64,
                    target_path.to_str().unwrap_or(""),
                    DownloadStatus::Completed,
                    retry_count as i64,
                )
                .await?;

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
                if let CoreError::ApiError(ref msg) = e {
                    if msg.contains("404") {
                        warn!("File no longer available (404): {}", filename);
                        // Clean up any partial download
                        self.cleanup_partial(&target_path).await;
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



    /// Check if a download can be resumed using range requests
    async fn check_resume(
        &self,
        file: &DownloadFile,
        target_path: &Path,
    ) -> CoreResult<Option<ResumeInfo>> {
        // Check for partial download file
        let temp_path = self.create_temp_path(target_path)?;
        if temp_path.exists() && is_partial_download(&temp_path) {
            // Get the current size of the partial file
            match tokio::fs::metadata(&temp_path).await {
                Ok(metadata) => {
                    let partial_size = metadata.len();
                    if partial_size > 0 && partial_size < file.file_size {
                        info!("Found partial download: {} bytes of {} bytes", partial_size, file.file_size);
                        Ok(Some(ResumeInfo {
                            file_path: temp_path,
                            completed_chunks: Vec::new(), // Not used for streaming resume
                            total_chunks: 0,              // Not used for streaming resume
                            bytes_downloaded: partial_size,
                            total_bytes: file.file_size,
                        }))
                    } else if partial_size >= file.file_size {
                        // File appears to be complete, clean it up and start fresh
                        info!("Partial file appears complete, cleaning up and starting fresh");
                        let _ = tokio::fs::remove_file(&temp_path).await;
                        Ok(None)
                    } else {
                        // Empty partial file, clean it up
                        debug!("Empty partial file found, cleaning up");
                        let _ = tokio::fs::remove_file(&temp_path).await;
                        Ok(None)
                    }
                }
                Err(_) => {
                    // Can't read metadata, clean up and start fresh
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Create temporary download path
    fn create_temp_path(&self, target_path: &Path) -> CoreResult<PathBuf> {
        let parent = target_path.parent();
        let filename = target_path
            .file_name()
            .ok_or_else(|| CoreError::InvalidInput("Invalid file path".to_string()))?
            .to_str()
            .ok_or_else(|| CoreError::InvalidInput("Invalid filename encoding".to_string()))?;

        let temp_filename = format!(".{}.downloading", filename);

        let temp_path = if let Some(parent) = parent {
            parent.join(temp_filename)
        } else {
            PathBuf::from(temp_filename)
        };

        Ok(temp_path)
    }


    /// Clean up a partial download
    async fn cleanup_partial(&self, path: &Path) {
        if path.exists() {
            if let Err(e) = tokio::fs::remove_file(path).await {
                warn!("Failed to clean up partial download {}: {}", path.display(), e);
            } else {
                debug!("Cleaned up partial download: {}", path.display());
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
            relative_path: "./".to_string()
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
