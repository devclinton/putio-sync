//! Streaming download functionality with buffering

use crate::error::{DownloadError, DownloadResult};
use crate::files::create_temp_path;
use crate::rate_limiter::RateLimiter;
use putio_sync_client::PutioApiClient;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncWriteExt, BufWriter};
use tracing::{debug, error, info, warn};

/// Default buffer size for streaming downloads (128MB)
pub const DEFAULT_BUFFER_SIZE: usize = 64 * 1024 * 1024;

/// Size thresholds for progress logging in bytes
const SIZE_256MB: u64 = 256 * 1024 * 1024;
const SIZE_512MB: u64 = 512 * 1024 * 1024;
const SIZE_1GB: u64 = 1024 * 1024 * 1024;
const SIZE_2GB: u64 = 2 * 1024 * 1024 * 1024;
const SIZE_4GB: u64 = 4 * 1024 * 1024 * 1024;
const SIZE_8GB: u64 = 8 * 1024 * 1024 * 1024;
const SIZE_16GB: u64 = 16 * 1024 * 1024 * 1024;
const SIZE_30GB: u64 = 30 * 1024 * 1024 * 1024;
const SIZE_40GB: u64 = 40 * 1024 * 1024 * 1024;
const SIZE_50GB: u64 = 50 * 1024 * 1024 * 1024;

/// Helper struct to track size-based progress logging
#[derive(Debug)]
struct ProgressLogger {
    progress_interval: f64,
    last_logged_percentage: f64,
    filename: String,
    last_bytes: u64,
    last_time: Instant,
}

impl ProgressLogger {
    /// Create a new progress logger based on file size
    fn new(file_size: u64, filename: String) -> Self {
        let progress_interval = match file_size {
            0..SIZE_256MB => 0.0,           // No progress logging for files under 256MB
            SIZE_256MB..SIZE_512MB => 50.0, // Log every 50% for files 256MB-512MB
            SIZE_512MB..SIZE_1GB => 40.0,   // Log every 40% for files 512MB-1GB
            SIZE_1GB..SIZE_2GB => 35.0,     // Log every 35% for files 1GB-2GB
            SIZE_2GB..SIZE_4GB => 25.0,     // Log every 25% for files 2GB-4GB
            SIZE_4GB..SIZE_8GB => 15.0,     // Log every 15% for files 4GB-8GB
            SIZE_8GB..SIZE_16GB => 7.0,     // Log every 7% for files 8GB-16GB
            SIZE_16GB..SIZE_30GB => 3.0,    // Log every 3% for files 16GB-30GB
            SIZE_30GB..SIZE_40GB => 2.0,    // Log every 2% for files 30GB-40GB
            SIZE_40GB..SIZE_50GB => 1.5,    // Log every 1.5% for files 40GB-50GB
            SIZE_50GB.. => 1.25,            // Log every 1.25% for files over 50GB
        };

        Self {
            progress_interval,
            last_logged_percentage: 0.0,
            filename,
            last_bytes: 0,
            last_time: Instant::now(),
        }
    }

    /// Check if progress should be logged and log if appropriate
    /// Returns true if progress was logged
    fn maybe_log_progress(&mut self, bytes_downloaded: u64, total_bytes: u64) -> bool {
        if self.progress_interval == 0.0 {
            return false; // No progress logging for small files
        }

        let current_percentage = (bytes_downloaded as f64 / total_bytes as f64) * 100.0;

        // Calculate which threshold we should be at
        let current_threshold =
            (current_percentage / self.progress_interval).floor() * self.progress_interval;

        // Only log if we've crossed a new threshold and haven't logged it yet
        if current_threshold > self.last_logged_percentage
            && current_percentage >= current_threshold
        {
            // Calculate transfer speed
            let now = Instant::now();
            let elapsed = now.duration_since(self.last_time).as_secs_f64();
            let bytes_diff = bytes_downloaded.saturating_sub(self.last_bytes) as f64;
            let speed_bps = if elapsed > 0.0 {
                bytes_diff / elapsed
            } else {
                0.0
            };

            info!(
                "📥 Download progress: {} - {:.1}% complete ({} / {}) @ {}",
                self.filename,
                current_threshold,
                format_bytes(bytes_downloaded),
                format_bytes(total_bytes),
                format_speed(speed_bps)
            );

            self.last_logged_percentage = current_threshold;
            self.last_bytes = bytes_downloaded;
            self.last_time = now;
            return true;
        }

        false
    }
}

/// Format bytes as human-readable string
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[unit_idx])
    } else {
        format!("{:.2} {}", size, UNITS[unit_idx])
    }
}

/// Format transfer speed as human-readable string
fn format_speed(bytes_per_sec: f64) -> String {
    const UNITS: &[&str] = &["B/s", "KB/s", "MB/s", "GB/s"];
    let mut speed = bytes_per_sec;
    let mut unit_idx = 0;

    while speed >= 1024.0 && unit_idx < UNITS.len() - 1 {
        speed /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{:.0} {}", speed, UNITS[unit_idx])
    } else {
        format!("{:.2} {}", speed, UNITS[unit_idx])
    }
}

/// Result of a streaming download operation
#[derive(Debug)]
pub struct StreamDownloadResult {
    /// Total bytes downloaded
    pub bytes_downloaded: u64,
    /// Final file path
    pub final_path: PathBuf,
    /// Whether the download was successful
    pub success: bool,
}

/// Download a file using streaming with buffering
///
/// This function streams the file directly to disk with a configurable buffer
/// to minimize memory usage. During download, the file is saved with a
/// `.downloading` extension and a `.` prefix. On successful completion,
/// it's renamed to the final name.
///
/// # Arguments
///
/// * `client` - API client for making authenticated requests
/// * `url` - URL to download from
/// * `target_path` - Final destination path
/// * `filename` - Filename for progress logging
/// * `expected_size` - Expected file size in bytes (for validation)
/// * `buffer_size_mb` - Buffer size in megabytes
/// * `bandwidth_limit_bytes_per_sec` - Optional bandwidth limit in bytes per second
/// * `resume_from_byte` - Optional byte offset to resume from (for range requests)
///
/// # Returns
///
/// A `StreamDownloadResult` containing download statistics
#[allow(clippy::too_many_arguments)]
pub async fn stream_download(
    client: &PutioApiClient,
    url: &str,
    target_path: &Path,
    filename: &str,
    expected_size: Option<u64>,
    buffer_size_mb: usize,
    bandwidth_limit_bytes_per_sec: Option<u64>,
    resume_from_byte: Option<u64>,
) -> DownloadResult<StreamDownloadResult> {
    let result = stream_to_temp(
        client,
        url,
        target_path,
        filename,
        expected_size,
        buffer_size_mb,
        bandwidth_limit_bytes_per_sec,
        resume_from_byte,
    )
    .await?;
    finish_download(&result.final_path, target_path).await?;
    Ok(StreamDownloadResult {
        final_path: target_path.to_path_buf(),
        ..result
    })
}

/// Atomically rename a verified hidden `.downloading` file to its final path
pub async fn finish_download(temp_path: &Path, target_path: &Path) -> DownloadResult<()> {
    tokio::fs::rename(temp_path, target_path)
        .await
        .map_err(|e| {
            DownloadError::IoError(std::io::Error::new(
                e.kind(),
                format!("Failed to rename download file: {}", e),
            ))
        })?;
    info!("Download saved to {}", target_path.display());
    Ok(())
}

/// [`stream_download`] without the final rename: the file stays at its hidden
/// `.downloading` path, returned as `final_path`, so the caller can check it first.
#[allow(clippy::too_many_arguments)]
pub async fn stream_to_temp(
    client: &PutioApiClient,
    url: &str,
    target_path: &Path,
    filename: &str,
    expected_size: Option<u64>,
    buffer_size_mb: usize,
    bandwidth_limit_bytes_per_sec: Option<u64>,
    resume_from_byte: Option<u64>,
) -> DownloadResult<StreamDownloadResult> {
    let buffer_size = buffer_size_mb * 1024 * 1024;

    if let Some(resume_from) = resume_from_byte {
        debug!(
            "Starting stream download with resume: {} -> {} (buffer: {}MB, resume from byte: {})",
            url,
            target_path.display(),
            buffer_size_mb,
            resume_from
        );
    } else {
        debug!(
            "Starting stream download: {} -> {} (buffer: {}MB)",
            url,
            target_path.display(),
            buffer_size_mb
        );
    }

    // Create temporary download path with .downloading extension and . prefix
    let temp_path = create_temp_path(target_path)?;

    // Ensure parent directory exists
    if let Some(parent) = temp_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            DownloadError::IoError(std::io::Error::new(
                e.kind(),
                format!("Failed to create download directory: {}", e),
            ))
        })?;
    }

    // Make authenticated HTTP request with extended timeout for downloads
    debug!("Making download request to: {}", url);
    let mut request_builder = client.authenticated_download_request(url)?;

    // Add user agent and other headers that Put.io might expect
    request_builder = request_builder
        .header("User-Agent", "putio-sync/0.0.1")
        .header("Accept", "*/*");

    // Add Range header for resuming if specified
    if let Some(resume_from) = resume_from_byte {
        if let Some(expected) = expected_size {
            let range_header = format!("bytes={}-{}", resume_from, expected - 1);
            debug!("Adding Range header: {}", range_header);
            request_builder = request_builder.header("Range", range_header);
        } else {
            let range_header = format!("bytes={}-", resume_from);
            debug!("Adding Range header: {}", range_header);
            request_builder = request_builder.header("Range", range_header);
        }
    }

    let request = request_builder.build().map_err(DownloadError::HttpError)?;

    debug!("Request URL: {}", request.url());
    debug!("Request method: {}", request.method());
    debug!("Request headers: {:?}", request.headers());

    let response = reqwest::Client::new().execute(request).await.map_err(|e| {
        error!("HTTP request failed: {}", e);
        DownloadError::HttpError(e)
    })?;

    let status = response.status();
    debug!(
        "Response status: {} {}",
        status.as_u16(),
        status.canonical_reason().unwrap_or("Unknown")
    );
    debug!("Response headers: {:?}", response.headers());

    // Log specific headers that might be relevant for debugging
    if let Some(content_type) = response.headers().get("content-type") {
        debug!("Content-Type: {:?}", content_type);
    }
    if let Some(content_length) = response.headers().get("content-length") {
        debug!("Content-Length: {:?}", content_length);
    }
    if let Some(accept_ranges) = response.headers().get("accept-ranges") {
        debug!("Accept-Ranges: {:?}", accept_ranges);
    }
    if let Some(content_range) = response.headers().get("content-range") {
        debug!("Content-Range: {:?}", content_range);
    }

    // Check for valid response status: 200 (OK) for full download, 206 (Partial Content) for range requests
    let is_valid_response =
        status.is_success() || (resume_from_byte.is_some() && status.as_u16() == 206);

    if !is_valid_response {
        // Try to get response body for more detailed error info
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read error response".to_string());

        // Enhanced error logging for 400 Bad Request
        if status.as_u16() == 400 {
            error!("=== 400 BAD REQUEST DETAILS ===");
            error!("Request URL: {}", url);
            error!("Request method: GET");
            if let Some(resume_from) = resume_from_byte {
                error!("Resume requested from byte: {}", resume_from);
                if let Some(expected) = expected_size {
                    error!(
                        "Range header would be: bytes={}-{}",
                        resume_from,
                        expected - 1
                    );
                } else {
                    error!("Range header would be: bytes={}-", resume_from);
                }
            } else {
                error!("No range request (full download)");
            }
            error!(
                "Response status: {} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown")
            );
            error!("Error response body: {}", error_body);
            error!("=== END 400 BAD REQUEST DETAILS ===");
        }

        let error_msg = format!(
            "HTTP error {}: {} - Response: {}",
            status,
            status.canonical_reason().unwrap_or("Unknown"),
            error_body
        );
        warn!("Download failed for URL {}: {}", url, error_msg);
        return Err(DownloadError::ApiError(error_msg));
    }

    // Open file for writing with buffering
    let file = if resume_from_byte.is_some() {
        // For resume, open in append mode
        OpenOptions::new()
            .create(true)
            .write(true)
            .append(true)
            .open(&temp_path)
            .await
            .map_err(|e| {
                DownloadError::IoError(std::io::Error::new(
                    e.kind(),
                    format!("Failed to open download file for resume: {}", e),
                ))
            })?
    } else {
        // For new downloads, truncate the file
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temp_path)
            .await
            .map_err(|e| {
                DownloadError::IoError(std::io::Error::new(
                    e.kind(),
                    format!("Failed to create download file: {}", e),
                ))
            })?
    };

    // Create channel for continuous disk writing
    // Channel size determines how much data can be buffered in memory
    let channel_capacity = buffer_size / (128 * 1024); // Number of ~128KB chunks that fit in buffer
    let (tx, mut rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(channel_capacity);

    let mut bytes_downloaded = 0u64;

    // Create rate limiter if bandwidth limit is set
    let mut rate_limiter = bandwidth_limit_bytes_per_sec.map(RateLimiter::new);

    // Create progress logger based on file size
    let mut progress_logger =
        expected_size.map(|size| ProgressLogger::new(size, filename.to_string()));

    // Task-locals don't reach the writer task, so take the worker id here
    let worker = crate::manager::current_worker();
    let written_before = resume_from_byte.unwrap_or(0);

    // Spawn disk writer task that runs concurrently
    let writer_handle = tokio::spawn({
        let temp_path = temp_path.clone();
        async move {
            let mut writer = BufWriter::with_capacity(buffer_size, file);
            let mut total_written = 0u64;

            // Continuously read from channel and write to disk
            while let Some(chunk) = rx.recv().await {
                writer.write_all(&chunk).await.map_err(|e| {
                    DownloadError::IoError(std::io::Error::new(
                        e.kind(),
                        format!("Failed to write to download file: {}", e),
                    ))
                })?;
                putio_sync_telemetry::download::bytes(chunk.len() as u64);
                total_written += chunk.len() as u64;
                if let Some(id) = worker {
                    putio_sync_telemetry::download::worker_inflight(
                        id,
                        written_before + total_written,
                    );
                }
            }

            // Flush any remaining buffered data
            writer.flush().await.map_err(|e| {
                DownloadError::IoError(std::io::Error::new(
                    e.kind(),
                    format!("Failed to flush download file: {}", e),
                ))
            })?;

            // Ensure all data is written to disk
            writer.shutdown().await.map_err(|e| {
                DownloadError::IoError(std::io::Error::new(
                    e.kind(),
                    format!("Failed to close download file: {}", e),
                ))
            })?;

            debug!(
                "Disk writer finished: wrote {} bytes to {}",
                total_written,
                temp_path.display()
            );

            Ok::<u64, DownloadError>(total_written)
        }
    });

    // Stream the response body with stall detection
    let mut stream = response.bytes_stream();
    let stall_timeout = Duration::from_secs(60); // 60 seconds timeout for stalled downloads

    use futures_util::StreamExt;

    // Network download loop - never blocks on disk I/O
    let network_result: Result<(), DownloadError> = async {
        loop {
            match tokio::time::timeout(stall_timeout, stream.next()).await {
                Ok(Some(Ok(chunk))) => {
                    // Apply rate limiting if enabled
                    if let Some(ref mut limiter) = rate_limiter {
                        limiter.consume(chunk.len()).await;
                    }

                    bytes_downloaded += chunk.len() as u64;

                    // Send to disk writer (async, non-blocking if channel has space)
                    tx.send(chunk).await.map_err(|_| {
                        DownloadError::IoError(std::io::Error::new(
                            std::io::ErrorKind::BrokenPipe,
                            "Disk writer task failed",
                        ))
                    })?;

                    // Use size-based progress logging for large files
                    if let Some(ref mut logger) = progress_logger {
                        if let Some(size) = expected_size {
                            let total_downloaded = if let Some(resume_from) = resume_from_byte {
                                resume_from + bytes_downloaded
                            } else {
                                bytes_downloaded
                            };
                            logger.maybe_log_progress(total_downloaded, size);
                        }
                    } else if bytes_downloaded.is_multiple_of(10 * 1024 * 1024) {
                        // Fallback debug logging for files without expected_size
                        debug!("Downloaded {} bytes so far...", bytes_downloaded);
                    }
                }
                Ok(Some(Err(e))) => {
                    return Err(DownloadError::HttpError(e));
                }
                Ok(None) => {
                    // Stream ended normally
                    debug!("Download stream ended normally");
                    break;
                }
                Err(_) => {
                    // Timeout occurred - download has stalled
                    error!("Download stalled - no data received for {} seconds. Downloaded {} bytes so far.",
                           stall_timeout.as_secs(), bytes_downloaded);

                    return Err(DownloadError::ApiError(format!(
                        "Download stalled - no data received for {} seconds (downloaded {} bytes)",
                        stall_timeout.as_secs(),
                        bytes_downloaded
                    )));
                }
            }
        }
        Ok(())
    }
    .await;

    // Drop sender to signal disk writer to finish
    drop(tx);

    // Wait for disk writer to finish and check for errors
    let disk_result = writer_handle.await.map_err(|e| {
        DownloadError::IoError(std::io::Error::other(format!(
            "Disk writer task panicked: {}",
            e
        )))
    })?;

    // Propagate any network errors
    network_result?;

    // Propagate any disk errors
    disk_result?;

    debug!(
        "Downloaded {} bytes to {}",
        bytes_downloaded,
        temp_path.display()
    );

    // Validate file size if expected size was provided
    if let Some(expected) = expected_size {
        let total_bytes_written = if let Some(resume_from) = resume_from_byte {
            // For resumed downloads, add the existing bytes to what we just downloaded
            resume_from + bytes_downloaded
        } else {
            // For new downloads, just use what we downloaded
            bytes_downloaded
        };

        if total_bytes_written != expected {
            warn!(
                "Total file size ({}) doesn't match expected size ({})",
                total_bytes_written, expected
            );
            // Clean up incomplete file
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(DownloadError::ApiError(format!(
                "Size mismatch: total file size {} bytes, expected {} bytes",
                total_bytes_written, expected
            )));
        }
    }

    info!(
        "Successfully downloaded {} bytes to {}",
        bytes_downloaded,
        temp_path.display()
    );

    Ok(StreamDownloadResult {
        bytes_downloaded,
        final_path: temp_path,
        success: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use std::time::Instant;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_stream_download_mock() {
        use mockito::Server;
        use tempfile::TempDir;

        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/test-file.txt")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_body("Hello, World!")
            .create_async()
            .await;

        let temp_dir = TempDir::new().unwrap();
        let target_path = temp_dir.path().join("downloaded.txt");

        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url());
        let url = format!("{}/test-file.txt", server.url());

        let result = stream_download(
            &client,
            &url,
            &target_path,
            "test.txt",
            Some(13),
            1,
            None,
            None,
        )
        .await
        .unwrap();

        assert!(result.success);
        assert_eq!(result.bytes_downloaded, 13);
        assert_eq!(result.final_path, target_path);
        assert!(target_path.exists());

        let content = tokio::fs::read_to_string(&target_path).await.unwrap();
        assert_eq!(content, "Hello, World!");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_stream_download_size_mismatch() {
        use mockito::Server;
        use tempfile::TempDir;

        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/test-file.txt")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_body("Hello")
            .create_async()
            .await;

        let temp_dir = TempDir::new().unwrap();
        let target_path = temp_dir.path().join("downloaded.txt");

        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url());
        let url = format!("{}/test-file.txt", server.url());

        // Expect 10 bytes but will only get 5
        let result = stream_download(
            &client,
            &url,
            &target_path,
            "test.txt",
            Some(10),
            1,
            None,
            None,
        )
        .await;

        assert!(result.is_err());
        assert!(!target_path.exists()); // Should clean up on failure
    }

    #[tokio::test]
    async fn test_stream_download_http_error() {
        use mockito::Server;
        use tempfile::TempDir;

        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/not-found.txt")
            .match_header("authorization", "Bearer test_token")
            .with_status(404)
            .create_async()
            .await;

        let temp_dir = TempDir::new().unwrap();
        let target_path = temp_dir.path().join("downloaded.txt");

        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url());
        let url = format!("{}/not-found.txt", server.url());

        let result =
            stream_download(&client, &url, &target_path, "test.txt", None, 1, None, None).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rate_limiter() {
        let mut limiter = RateLimiter::new(1024); // 1KB/sec

        let start = Instant::now();

        // Consume 2048 bytes (2 seconds worth at 1024 bytes/sec)
        limiter.consume(2048).await;
        let elapsed = start.elapsed();

        // Should take approximately 2 seconds
        // Allow some tolerance for overhead
        assert!(elapsed >= Duration::from_millis(1800));
        assert!(elapsed < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn test_stream_download_with_bandwidth_limit() {
        let temp_dir = TempDir::new().unwrap();
        let target_path = temp_dir.path().join("limited-file.txt");

        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/limited-file.txt")
            .with_status(200)
            .with_body("x".repeat(2048)) // 2KB file
            .create_async()
            .await;

        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url());
        let url = format!("{}/limited-file.txt", server.url());

        let start = Instant::now();

        // Set bandwidth limit to 1024 bytes/sec
        let result = stream_download(
            &client,
            &url,
            &target_path,
            "test.txt",
            Some(2048),
            1,
            Some(1024),
            None,
        )
        .await
        .unwrap();

        let elapsed = start.elapsed();

        assert!(result.success);
        assert_eq!(result.bytes_downloaded, 2048);

        // Download should take approximately 2 seconds (2048 bytes at 1024 bytes/sec)
        // Allow some tolerance for overhead
        assert!(elapsed >= Duration::from_millis(1500));
        assert!(elapsed < Duration::from_secs(4));

        mock.assert_async().await;
    }
}
