//! Download manager for orchestrating downloads

use super::chunk::{
    calculate_chunks, download_chunk, supports_range_requests, ResumeInfo,
    DEFAULT_CHUNK_SIZE, MIN_CHUNK_FILE_SIZE,
};
use super::progress::{DownloadProgress, OverallProgress, ProgressCallback, ProgressTracker};
use super::stream::{get_original_path, is_partial_download, stream_download};
use crate::api::ApiClient;
use crate::config::DownloadConfig;
use crate::db::models::{ChunkStatus, DownloadStatus};
use crate::db::Database;
use crate::error::{CoreError, CoreResult};
use crate::fs::sanitize::sanitize_filename;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::fs::OpenOptions;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// File information for download
#[derive(Debug, Clone)]
pub struct DownloadFile {
    /// File ID from Put.io
    pub file_id: i64,
    /// Original filename
    pub filename: String,
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

        // Notify callback that file download is starting
        if let Some(ref callback) = progress_callback {
            callback.on_file_start(file_id, &filename, file.file_size);
        }

        // Check if file already exists and skip if configured
        let target_path = self.config.target_directory.join(&filename);
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

        // Download the file
        let result = if resume_info.is_some() {
            self.download_with_resume(file, resume_info.unwrap(), progress_callback.clone())
                .await
        } else if file.file_size >= MIN_CHUNK_FILE_SIZE {
            // Use chunked download for large files
            self.download_chunked(file, progress_callback.clone()).await
        } else {
            // Use streaming download for small files
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

    /// Download using streaming (for small files)
    async fn download_streaming(
        &self,
        file: DownloadFile,
        progress_callback: Option<Arc<dyn ProgressCallback>>,
    ) -> CoreResult<DownloadOperationResult> {
        let filename = sanitize_filename(&file.filename);
        let target_path = self.config.target_directory.join(&filename);

        // Create progress tracker
        self.progress_trackers
            .write()
            .await
            .insert(file.file_id, ProgressTracker::new(0));

        // Start download
        let start_time = Instant::now();

        match stream_download(
            &self.client,
            &file.download_url,
            &target_path,
            Some(file.file_size),
            self.config.buffer_size_mb,
            self.config.bandwidth_limit_bytes_per_sec,
        )
        .await
        {
            Ok(result) => {
                let duration = start_time.elapsed();

                // Update progress
                if let Some(ref callback) = progress_callback {
                    let mut progress = DownloadProgress::new(
                        file.file_id,
                        filename.clone(),
                        file.file_size,
                    );
                    progress.bytes_downloaded = result.bytes_downloaded;
                    progress.complete = true;
                    callback.on_file_progress(&progress);
                }

                // Save to database
                let repo = self.database.downloads();
                repo.insert(
                    file.file_id,
                    &filename,
                    file.file_size as i64,
                    target_path.to_str().unwrap_or(""),
                    DownloadStatus::Completed,
                )
                .await?;

                info!(
                    "Download completed: {} ({} bytes in {:.2}s)",
                    filename,
                    result.bytes_downloaded,
                    duration.as_secs_f64()
                );

                Ok(DownloadOperationResult {
                    file_id: file.file_id,
                    success: true,
                    error: None,
                    bytes_downloaded: result.bytes_downloaded,
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

    /// Download using chunks (for large files)
    async fn download_chunked(
        &self,
        file: DownloadFile,
        progress_callback: Option<Arc<dyn ProgressCallback>>,
    ) -> CoreResult<DownloadOperationResult> {
        let filename = sanitize_filename(&file.filename);
        let target_path = self.config.target_directory.join(&filename);

        // Check if server supports range requests
        if !supports_range_requests(&self.client, &file.download_url).await {
            debug!(
                "Server doesn't support range requests, falling back to streaming for {}",
                filename
            );
            return self.download_streaming(file, progress_callback).await;
        }

        debug!(
            "Using chunked download for {} ({} bytes)",
            filename, file.file_size
        );

        // Calculate chunks
        let chunks = calculate_chunks(file.file_size, DEFAULT_CHUNK_SIZE);
        let total_chunks = chunks.len();

        info!(
            "Downloading {} in {} chunks",
            filename,
            total_chunks
        );

        // Create temp file path
        let temp_path = self.create_temp_path(&target_path)?;

        // Pre-allocate file
        self.preallocate_file(&temp_path, file.file_size).await?;

        // Save download to database
        let repo = self.database.downloads();
        let download_id = repo
            .insert(
                file.file_id,
                &filename,
                file.file_size as i64,
                target_path.to_str().unwrap_or(""),
                DownloadStatus::InProgress,
            )
            .await?;

        // Save chunks to database
        for chunk in &chunks {
            repo.insert_chunk(download_id, chunk.number as i64, chunk.start as i64, chunk.end as i64)
                .await?;
        }

        // Download chunks
        let mut bytes_downloaded = 0u64;
        let start_time = Instant::now();

        for chunk in chunks {
            match download_chunk(
                &self.client,
                &file.download_url,
                &chunk,
                &temp_path,
                self.config.buffer_size_mb * 1024 * 1024,
            )
            .await
            {
                Ok(bytes) => {
                    bytes_downloaded += bytes;

                    // Update chunk status in database
                    repo.update_chunk_status_by_number(
                        download_id,
                        chunk.number as i64,
                        ChunkStatus::Completed,
                    )
                    .await?;

                    // Update progress
                    if let Some(ref callback) = progress_callback {
                        let mut progress = DownloadProgress::new(
                            file.file_id,
                            filename.clone(),
                            file.file_size,
                        );
                        progress.bytes_downloaded = bytes_downloaded;

                        // Calculate speed/ETA
                        let mut trackers = self.progress_trackers.write().await;
                        let tracker = trackers
                            .entry(file.file_id)
                            .or_insert_with(|| ProgressTracker::new(0));
                        let (speed, eta) = tracker.update(bytes_downloaded, file.file_size);
                        progress.speed_bps = speed;
                        progress.eta_seconds = eta;

                        callback.on_file_progress(&progress);
                    }
                }
                Err(e) => {
                    error!("Failed to download chunk {}: {}", chunk.number, e);

                    // Check if it's a 404 error
                    if let CoreError::ApiError(ref msg) = e {
                        if msg.contains("404") {
                            warn!("File no longer available (404): {}", filename);
                            // Clean up partial download
                            self.cleanup_partial(&temp_path).await;
                            repo.update_status(download_id, DownloadStatus::Failed)
                                .await?;

                            return Ok(DownloadOperationResult {
                                file_id: file.file_id,
                                success: false,
                                error: Some("File no longer available (404)".to_string()),
                                bytes_downloaded: 0,
                                file_path: None,
                            });
                        }
                    }

                    // Mark download as failed but keep partial file for resume
                    repo.update_status(download_id, DownloadStatus::Failed)
                        .await?;

                    return Ok(DownloadOperationResult {
                        file_id: file.file_id,
                        success: false,
                        error: Some(e.to_string()),
                        bytes_downloaded,
                        file_path: None,
                    });
                }
            }
        }

        // Rename temp file to final name
        tokio::fs::rename(&temp_path, &target_path)
            .await
            .map_err(|e| {
                CoreError::IoError(std::io::Error::new(
                    e.kind(),
                    format!("Failed to rename downloaded file: {}", e),
                ))
            })?;

        // Update database
        repo.update_status(download_id, DownloadStatus::Completed)
            .await?;

        let duration = start_time.elapsed();
        info!(
            "Chunked download completed: {} ({} bytes in {:.2}s)",
            filename,
            bytes_downloaded,
            duration.as_secs_f64()
        );

        Ok(DownloadOperationResult {
            file_id: file.file_id,
            success: true,
            error: None,
            bytes_downloaded,
            file_path: Some(target_path),
        })
    }

    /// Download with resume support
    async fn download_with_resume(
        &self,
        file: DownloadFile,
        resume_info: ResumeInfo,
        progress_callback: Option<Arc<dyn ProgressCallback>>,
    ) -> CoreResult<DownloadOperationResult> {
        info!(
            "Resuming download: {} ({:.1}% complete)",
            file.filename,
            resume_info.percentage()
        );

        // Get remaining chunks
        let all_chunks = calculate_chunks(file.file_size, DEFAULT_CHUNK_SIZE);
        let mut bytes_downloaded = resume_info.bytes_downloaded;

        // Get download from database
        let repo = self.database.downloads();
        let download = repo
            .find_by_file_id(file.file_id)
            .await?
            .ok_or_else(|| CoreError::InternalError("Download not found in database".to_string()))?;

        // Download remaining chunks
        let start_time = Instant::now();

        for chunk in all_chunks {
            if resume_info.completed_chunks.contains(&chunk.number) {
                continue; // Skip already completed chunks
            }

            match download_chunk(
                &self.client,
                &file.download_url,
                &chunk,
                &resume_info.file_path,
                self.config.buffer_size_mb * 1024 * 1024,
            )
            .await
            {
                Ok(bytes) => {
                    bytes_downloaded += bytes;

                    // Update chunk in database
                    repo.update_chunk_status_by_number(download.id, chunk.number as i64, ChunkStatus::Completed)
                        .await?;

                    // Update progress
                    if let Some(ref callback) = progress_callback {
                        let mut progress = DownloadProgress::new(
                            file.file_id,
                            file.filename.clone(),
                            file.file_size,
                        );
                        progress.bytes_downloaded = bytes_downloaded;

                        // Calculate speed/ETA
                        let mut trackers = self.progress_trackers.write().await;
                        let tracker = trackers
                            .entry(file.file_id)
                            .or_insert_with(|| ProgressTracker::new(resume_info.bytes_downloaded));
                        let (speed, eta) = tracker.update(bytes_downloaded, file.file_size);
                        progress.speed_bps = speed;
                        progress.eta_seconds = eta;

                        callback.on_file_progress(&progress);
                    }
                }
                Err(e) => {
                    error!("Failed to download chunk {}: {}", chunk.number, e);
                    repo.update_status(download.id, DownloadStatus::Failed)
                        .await?;

                    return Ok(DownloadOperationResult {
                        file_id: file.file_id,
                        success: false,
                        error: Some(e.to_string()),
                        bytes_downloaded,
                        file_path: None,
                    });
                }
            }
        }

        // Get original path and rename
        let target_path = get_original_path(&resume_info.file_path)
            .ok_or_else(|| CoreError::InternalError("Invalid partial file path".to_string()))?;

        tokio::fs::rename(&resume_info.file_path, &target_path)
            .await
            .map_err(|e| {
                CoreError::IoError(std::io::Error::new(
                    e.kind(),
                    format!("Failed to rename resumed file: {}", e),
                ))
            })?;

        // Update database
        repo.update_status(download.id, DownloadStatus::Completed)
            .await?;

        let duration = start_time.elapsed();
        info!(
            "Resume completed: {} ({} bytes in {:.2}s)",
            file.filename,
            bytes_downloaded - resume_info.bytes_downloaded,
            duration.as_secs_f64()
        );

        Ok(DownloadOperationResult {
            file_id: file.file_id,
            success: true,
            error: None,
            bytes_downloaded,
            file_path: Some(target_path),
        })
    }

    /// Check if a download can be resumed
    async fn check_resume(
        &self,
        file: &DownloadFile,
        target_path: &Path,
    ) -> CoreResult<Option<ResumeInfo>> {
        // Check for partial download file
        let temp_path = self.create_temp_path(target_path)?;
        if !temp_path.exists() || !is_partial_download(&temp_path) {
            return Ok(None);
        }

        // Check database for download record
        let repo = self.database.downloads();
        let download = match repo.find_by_file_id(file.file_id).await? {
            Some(d) => d,
            None => return Ok(None),
        };

        // Get completed chunks
        let chunks = repo.get_chunks(download.id).await?;
        let completed_chunks: Vec<u64> = chunks
            .iter()
            .filter(|c| c.status == "completed")
            .map(|c| c.chunk_index as u64)
            .collect();

        let total_chunks = calculate_chunks(file.file_size, DEFAULT_CHUNK_SIZE).len() as u64;
        let bytes_downloaded = completed_chunks
            .iter()
            .map(|&num| {
                chunks
                    .iter()
                    .find(|c| c.chunk_index as u64 == num)
                    .map(|c| (c.end_byte - c.start_byte + 1) as u64)
                    .unwrap_or(0)
            })
            .sum();

        Ok(Some(ResumeInfo {
            file_path: temp_path,
            completed_chunks,
            total_chunks,
            bytes_downloaded,
            total_bytes: file.file_size,
        }))
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

    /// Pre-allocate a file to the given size
    async fn preallocate_file(&self, path: &Path, size: u64) -> CoreResult<()> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)
            .await
            .map_err(|e| {
                CoreError::IoError(std::io::Error::new(
                    e.kind(),
                    format!("Failed to create file: {}", e),
                ))
            })?;

        file.set_len(size).await.map_err(|e| {
            CoreError::IoError(std::io::Error::new(
                e.kind(),
                format!("Failed to pre-allocate file: {}", e),
            ))
        })?;

        Ok(())
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
