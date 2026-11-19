//! Streaming download functionality with buffering

use crate::error::{DownloadError, DownloadResult};
use crate::files::{create_temp_path, Crc32};
use crate::rate_limiter::RateLimiter;
use putio_sync_client::PutioApiClient;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Notify;
use tracing::{debug, error, info, warn};

/// The writer collects chunks into batches of this size before a disk write
const WRITE_BATCH: usize = 2 * 1024 * 1024;

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

/// Asks one running download to stop (feature 039, R7)
///
/// The chunk loop checks it between chunks and while it waits for the network or the rate
/// limiter, so a stop takes effect within one chunk.
#[derive(Debug, Default)]
pub struct StopSignal {
    flag: AtomicBool,
    notify: Notify,
}

impl StopSignal {
    /// Ask the download to stop
    pub fn stop(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_one();
    }

    /// Whether a stop was asked for
    pub fn is_stopped(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Resolves once a stop is asked for
    pub(crate) async fn stopped(&self) {
        while !self.is_stopped() {
            self.notify.notified().await;
        }
    }
}

/// Run `fut`, unless `stop` asks to stop first
pub(crate) async fn unless_stopped<F: Future>(
    stop: Option<&StopSignal>,
    fut: F,
) -> DownloadResult<F::Output> {
    match stop {
        Some(stop) => tokio::select! {
            biased;
            _ = stop.stopped() => Err(DownloadError::Stopped),
            out = fut => Ok(out),
        },
        None => Ok(fut.await),
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
    /// CRC32 of the whole file when a digest was passed in, as 8 lower-case hex digits
    pub crc32: Option<String>,
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
/// * `stop` - Optional stop signal; a stop ends the download with
///   [`DownloadError::Stopped`], keeping the partial temp file
///
/// # Returns
///
/// A `StreamDownloadResult` containing download statistics
/// No bytes for this long ends a download as stalled. Only the network read counts:
/// time spent held back by the bandwidth limit or a pause is not a stall.
const STALL_TIMEOUT: Duration = if cfg!(test) {
    Duration::from_secs(1)
} else {
    Duration::from_secs(60)
};

#[allow(clippy::too_many_arguments)]
pub async fn stream_download(
    client: &PutioApiClient,
    url: &str,
    target_path: &Path,
    filename: &str,
    expected_size: Option<u64>,
    buffer_size_mb: usize,
    limiter: Option<&RateLimiter>,
    resume_from_byte: Option<u64>,
    stop: Option<&StopSignal>,
) -> DownloadResult<StreamDownloadResult> {
    let result = stream_to_temp(
        client,
        url,
        target_path,
        filename,
        expected_size,
        buffer_size_mb,
        limiter,
        resume_from_byte,
        stop,
        None,
    )
    .await?;
    finish_download(&result.final_path, target_path).await?;
    Ok(StreamDownloadResult {
        final_path: target_path.to_path_buf(),
        ..result
    })
}

/// Atomically rename a verified hidden `.downloading` file to its final path, durably:
/// the file's data is flushed to disk first and the folder entry after, so a power cut
/// right after cannot leave a complete-looking file with missing bytes, or no file at
/// all, once the caller deletes the original from Put.io.
pub async fn finish_download(temp_path: &Path, target_path: &Path) -> DownloadResult<()> {
    let temp = temp_path.to_path_buf();
    let target = target_path.to_path_buf();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        // Opened for writing: Windows refuses to flush a read-only handle
        std::fs::OpenOptions::new()
            .write(true)
            .open(&temp)
            .and_then(|file| file.sync_all())
            .map_err(|e| {
                std::io::Error::new(e.kind(), format!("Failed to flush download file: {}", e))
            })?;
        std::fs::rename(&temp, &target).map_err(|e| {
            std::io::Error::new(e.kind(), format!("Failed to rename download file: {}", e))
        })?;
        sync_parent_dir(&target);
        Ok(())
    })
    .await
    .map_err(|e| DownloadError::InternalError(format!("Finishing the download panicked: {e}")))?
    .map_err(DownloadError::IoError)?;
    info!("Download saved to {}", target_path.display());
    Ok(())
}

/// Flush the folder entry of `path` to disk (Unix). Best effort: some network shares
/// refuse to sync a folder, and the file itself is already on disk.
fn sync_parent_dir(path: &Path) {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::File::open(parent).and_then(|dir| dir.sync_all()) {
            debug!("Could not sync folder {}: {}", parent.display(), e);
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// [`stream_download`] without the final rename: the file stays at its hidden
/// `.downloading` path, returned as `final_path`, so the caller can check it first.
///
/// `digest` is the CRC32 of the bytes already on disk (empty for a fresh download);
/// every byte written is added to it, in write order, and the final value is
/// returned in `crc32`.
#[allow(clippy::too_many_arguments)]
pub async fn stream_to_temp(
    client: &PutioApiClient,
    url: &str,
    target_path: &Path,
    filename: &str,
    expected_size: Option<u64>,
    buffer_size_mb: usize,
    limiter: Option<&RateLimiter>,
    resume_from_byte: Option<u64>,
    stop: Option<&StopSignal>,
    digest: Option<Crc32>,
) -> DownloadResult<StreamDownloadResult> {
    // Config validation caps it; a caller that skipped it still cannot overflow
    let buffer_size = buffer_size_mb.min(putio_sync_core::config::MAX_BUFFER_SIZE_MB) * 1024 * 1024;

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

    // Make the authenticated request on the shared download client (connect and read
    // timeouts, no total one); a stop ends the wait for the server's answer
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

    let response = unless_stopped(stop, client.download_client().execute(request))
        .await?
        .map_err(|e| {
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

    // The server cannot serve the range asked for (the file changed, or it no longer
    // allows ranges): the partial file is no use, so the next attempt starts over
    if status.as_u16() == 416 && resume_from_byte.is_some() {
        warn!("Server answered 416 to a resume of {}; starting over", url);
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(DownloadError::ApiError(
            "Resume refused by the server (416); the partial download was discarded".to_string(),
        ));
    }

    // Check for valid response status: 200 (OK) for full download, 206 (Partial Content) for range requests
    if !status.is_success() {
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

    // Where the body starts. A resume needs a 206 whose Content-Range begins at the
    // byte asked for; a plain 200 means the server ignored the Range header and sends
    // the whole file, so it replaces the partial one. Appending that to the partial
    // file would corrupt it.
    let requested_from = resume_from_byte.filter(|from| *from > 0);
    let from = match requested_from {
        Some(want) if status.as_u16() == 206 => match content_range_start(response.headers()) {
            Some(start) if start == want => want,
            other => {
                warn!(
                    "Resume asked for byte {} but the server's range starts at {:?}; starting over",
                    want, other
                );
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(DownloadError::ApiError(format!(
                    "Resume refused: the server's range does not start at byte {want}; \
                     the partial download was discarded"
                )));
            }
        },
        Some(_) => {
            warn!(
                "The server ignored the Range header (status {}); downloading from the start",
                status
            );
            0
        }
        None => 0,
    };
    // What the server says it is sending, to tell a cut connection from a short file
    let body_length = match content_range(response.headers()) {
        Some((start, end)) if from > 0 && end >= start => Some(end - start + 1),
        _ => response.content_length(),
    };
    // A restart from 0 hashes from scratch, not from the partial file's prefix
    let digest = if requested_from.is_some() && from == 0 {
        digest.map(|_| Crc32::new())
    } else {
        digest
    };

    // Open file for writing
    let file = if from > 0 {
        let mut file = OpenOptions::new()
            .write(true)
            .open(&temp_path)
            .await
            .map_err(|e| {
                DownloadError::IoError(std::io::Error::new(
                    e.kind(),
                    format!("Failed to open download file for resume: {}", e),
                ))
            })?;
        let len = file.metadata().await?.len();
        if len < from {
            drop(file);
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(DownloadError::ApiError(format!(
                "The partial download is shorter ({len} bytes) than the resume point ({from}); \
                 it was discarded"
            )));
        }
        if len > from {
            file.set_len(from).await?;
        }
        file.seek(std::io::SeekFrom::Start(from)).await?;
        file
    } else {
        // For new downloads, truncate the file
        let file = OpenOptions::new()
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
            })?;
        match expected_size {
            Some(size) if size > 0 => preallocate(file, size).await,
            _ => file,
        }
    };

    // Create channel for continuous disk writing
    // Channel size determines how much data can be buffered in memory
    let channel_capacity = (buffer_size / (128 * 1024)).max(2); // Number of ~128KB chunks that fit in buffer
    let (tx, mut rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(channel_capacity);

    let mut bytes_downloaded = 0u64;

    // Create progress logger based on file size
    let mut progress_logger =
        expected_size.map(|size| ProgressLogger::new(size, filename.to_string()));

    // Task-locals don't reach the writer task, so take the worker id here
    let worker = crate::manager::current_worker();
    let written_before = from;

    // Spawn disk writer task that runs concurrently
    let writer_handle = tokio::spawn({
        let temp_path = temp_path.clone();
        async move {
            // The channel is the read-ahead buffer; the file gets batches of a few MiB,
            // so there is no second buffer of `buffer_size` to copy through
            let mut file = file;
            let mut batch: Vec<u8> = Vec::with_capacity(WRITE_BATCH + 256 * 1024);
            let mut total_written = 0u64;
            let mut digest = digest;
            let write_error = |e: std::io::Error| {
                let message = if matches!(
                    e.kind(),
                    std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded
                ) {
                    format!(
                        "The disk is full: no space left to write {}",
                        temp_path.display()
                    )
                } else {
                    format!("Failed to write to download file: {}", e)
                };
                DownloadError::IoError(std::io::Error::new(e.kind(), message))
            };

            // Continuously read from channel and write to disk
            while let Some(chunk) = rx.recv().await {
                if let Some(digest) = digest.as_mut() {
                    digest.update(&chunk);
                }
                putio_sync_telemetry::download::bytes(chunk.len() as u64);
                total_written += chunk.len() as u64;
                if let Some(id) = worker {
                    putio_sync_telemetry::download::worker_inflight(
                        id,
                        written_before + total_written,
                    );
                }
                if batch.is_empty() && chunk.len() >= WRITE_BATCH {
                    file.write_all(&chunk).await.map_err(write_error)?;
                } else {
                    batch.extend_from_slice(&chunk);
                    if batch.len() >= WRITE_BATCH {
                        file.write_all(&batch).await.map_err(write_error)?;
                        batch.clear();
                    }
                }
            }

            // Write the last, smaller batch
            if !batch.is_empty() {
                file.write_all(&batch).await.map_err(write_error)?;
            }

            // Hand everything to the OS; `finish_download` syncs it to disk before the rename
            file.flush().await.map_err(write_error)?;

            debug!(
                "Disk writer finished: wrote {} bytes to {}",
                total_written,
                temp_path.display()
            );

            Ok::<(u64, Option<String>), DownloadError>((total_written, digest.map(|d| d.hex())))
        }
    });

    // Stream the response body with stall detection
    let mut stream = response.bytes_stream();
    let stall_timeout = STALL_TIMEOUT;

    use futures_util::StreamExt;

    // Network download loop - never blocks on disk I/O
    let network_result: Result<(), DownloadError> = async {
        loop {
            if stop.is_some_and(StopSignal::is_stopped) {
                return Err(DownloadError::Stopped);
            }
            match unless_stopped(stop, tokio::time::timeout(stall_timeout, stream.next())).await? {
                Ok(Some(Ok(chunk))) => {
                    // The shared limit; time held back here is outside the stall timeout
                    if let Some(limiter) = limiter {
                        unless_stopped(stop, limiter.consume(chunk.len())).await?;
                    }

                    bytes_downloaded += chunk.len() as u64;

                    // Send to disk writer (async, non-blocking if channel has space).
                    // A closed channel means the writer failed; its own error is
                    // read below, which says why (a full disk, say).
                    if tx.send(chunk).await.is_err() {
                        return Err(DownloadError::IoError(std::io::Error::new(
                            std::io::ErrorKind::BrokenPipe,
                            "Disk writer task failed",
                        )));
                    }

                    // Use size-based progress logging for large files
                    if let Some(ref mut logger) = progress_logger {
                        if let Some(size) = expected_size {
                            logger.maybe_log_progress(from + bytes_downloaded, size);
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

    // A disk error comes first: when the writer fails, the network loop only sees its
    // channel close, and the writer's error is the real cause
    let (_, crc32) = match disk_result {
        Ok(done) => done,
        Err(e) => {
            if e.is_storage_full() {
                error!("Disk full while writing {}", temp_path.display());
            }
            return Err(e);
        }
    };

    // Propagate any network errors
    network_result?;

    debug!(
        "Downloaded {} bytes to {}",
        bytes_downloaded,
        temp_path.display()
    );

    // Validate file size if expected size was provided
    if let Some(expected) = expected_size {
        // For resumed downloads, add the existing bytes to what we just downloaded
        let total_bytes_written = from + bytes_downloaded;

        let cut_short = body_length.is_none_or(|len| bytes_downloaded < len);
        if total_bytes_written < expected && cut_short {
            // The connection ended early without an error. What arrived is valid, so the
            // partial file stays and the next attempt resumes it.
            warn!(
                "Download ended early: {} of {} bytes",
                total_bytes_written, expected
            );
            return Err(DownloadError::ApiError(format!(
                "Incomplete download: the connection ended after {} of {} bytes",
                total_bytes_written, expected
            )));
        }
        if total_bytes_written != expected {
            warn!(
                "Total file size ({}) doesn't match expected size ({})",
                total_bytes_written, expected
            );
            // Clean up the file: it is not what Put.io described
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
        crc32,
    })
}

/// The first byte of a `Content-Range: bytes 100-199/200` header
fn content_range_start(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    content_range(headers).map(|(start, _)| start)
}

/// The first and last byte of a `Content-Range: bytes a-b/total` header
fn content_range(headers: &reqwest::header::HeaderMap) -> Option<(u64, u64)> {
    let value = headers.get(reqwest::header::CONTENT_RANGE)?.to_str().ok()?;
    let range = value.trim().strip_prefix("bytes")?.trim_start();
    let (span, _total) = range.split_once('/')?;
    let (start, end) = span.split_once('-')?;
    Some((start.trim().parse().ok()?, end.trim().parse().ok()?))
}

/// Reserve disk blocks for the whole file where the OS can (Linux), so the file is
/// not fragmented and a full disk shows up early. The length stays 0 until bytes are
/// written (`FALLOC_FL_KEEP_SIZE`), so an interrupted download still looks like a
/// partial file. Best effort: errors are ignored.
#[cfg(target_os = "linux")]
async fn preallocate(file: tokio::fs::File, len: u64) -> tokio::fs::File {
    let std_file = file.into_std().await;
    let std_file = tokio::task::spawn_blocking(move || {
        use std::os::fd::AsRawFd;
        // SAFETY: the descriptor belongs to `std_file`, which outlives the call
        let _ = unsafe {
            libc::fallocate(
                std_file.as_raw_fd(),
                libc::FALLOC_FL_KEEP_SIZE,
                0,
                len as libc::off_t,
            )
        };
        std_file
    })
    .await
    .expect("fallocate does not panic");
    tokio::fs::File::from_std(std_file)
}

#[cfg(not(target_os = "linux"))]
async fn preallocate(file: tokio::fs::File, _len: u64) -> tokio::fs::File {
    file
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
            None,
        )
        .await;

        assert!(result.is_err());
        assert!(!target_path.exists()); // Should clean up on failure
    }

    /// A 10-byte file whose first 5 bytes are already in the partial file
    async fn resume_setup(
        server: &mut mockito::ServerGuard,
        response: impl FnOnce(mockito::Mock) -> mockito::Mock,
    ) -> (
        TempDir,
        PathBuf,
        PathBuf,
        PutioApiClient,
        String,
        mockito::Mock,
    ) {
        let mock = response(
            server
                .mock("GET", "/resume.bin")
                .match_header("range", "bytes=5-9"),
        )
        .create_async()
        .await;
        let temp_dir = TempDir::new().unwrap();
        let target_path = temp_dir.path().join("resume.bin");
        let temp_path = create_temp_path(&target_path).unwrap();
        tokio::fs::write(&temp_path, b"01234").await.unwrap();
        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url());
        let url = format!("{}/resume.bin", server.url());
        (temp_dir, target_path, temp_path, client, url, mock)
    }

    async fn resume(
        client: &PutioApiClient,
        url: &str,
        target_path: &Path,
    ) -> DownloadResult<StreamDownloadResult> {
        let mut digest = Crc32::new();
        digest.update(b"01234");
        stream_to_temp(
            client,
            url,
            target_path,
            "resume.bin",
            Some(10),
            1,
            None,
            Some(5),
            None,
            Some(digest),
        )
        .await
    }

    #[tokio::test]
    async fn resume_appends_a_206_that_starts_where_asked() {
        let mut server = Server::new_async().await;
        let (_dir, target, temp, client, url, mock) = resume_setup(&mut server, |m| {
            m.with_status(206)
                .with_header("content-range", "bytes 5-9/10")
                .with_body("56789")
        })
        .await;

        let result = resume(&client, &url, &target).await.unwrap();

        assert_eq!(result.bytes_downloaded, 5);
        assert_eq!(tokio::fs::read(&temp).await.unwrap(), b"0123456789");
        let mut whole = Crc32::new();
        whole.update(b"0123456789");
        assert_eq!(result.crc32, Some(whole.hex()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn resume_answered_with_200_replaces_the_partial_file() {
        let mut server = Server::new_async().await;
        // The server ignores Range and sends the whole file
        let (_dir, target, temp, client, url, mock) =
            resume_setup(&mut server, |m| m.with_status(200).with_body("0123456789")).await;

        let result = resume(&client, &url, &target).await.unwrap();

        assert_eq!(result.bytes_downloaded, 10);
        assert_eq!(tokio::fs::read(&temp).await.unwrap(), b"0123456789");
        // The CRC covers the new bytes only, not the old prefix twice
        let mut whole = Crc32::new();
        whole.update(b"0123456789");
        assert_eq!(result.crc32, Some(whole.hex()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn resume_with_a_misplaced_range_discards_the_partial_file() {
        let mut server = Server::new_async().await;
        let (_dir, target, temp, client, url, mock) = resume_setup(&mut server, |m| {
            m.with_status(206)
                .with_header("content-range", "bytes 0-9/10")
                .with_body("0123456789")
        })
        .await;

        let err = resume(&client, &url, &target).await.unwrap_err();

        assert!(err.is_transient(), "{err}");
        assert!(!temp.exists());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn a_short_body_keeps_the_partial_file_for_resume() {
        let mut server = Server::new_async().await;
        let (_dir, target, temp, client, url, mock) = resume_setup(&mut server, |m| {
            m.with_status(206)
                .with_header("content-range", "bytes 5-9/10")
                .with_body("56")
        })
        .await;

        let err = resume(&client, &url, &target).await.unwrap_err();

        assert!(err.to_string().contains("Incomplete download"), "{err}");
        assert!(err.is_transient());
        assert_eq!(tokio::fs::read(&temp).await.unwrap(), b"0123456");
        assert!(!target.exists());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn a_whole_response_smaller_than_put_io_says_is_a_size_mismatch() {
        let mut server = Server::new_async().await;
        let (_dir, target, temp, client, url, mock) = resume_setup(&mut server, |m| {
            m.with_status(206)
                .with_header("content-range", "bytes 5-6/7")
                .with_body("56")
        })
        .await;

        let err = resume(&client, &url, &target).await.unwrap_err();

        assert!(err.to_string().contains("Size mismatch"), "{err}");
        assert!(!err.is_transient());
        assert!(!temp.exists());
        mock.assert_async().await;
    }

    #[test]
    fn content_range_start_reads_the_first_byte() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(content_range_start(&headers), None);
        headers.insert("content-range", "bytes 100-199/200".parse().unwrap());
        assert_eq!(content_range_start(&headers), Some(100));
        headers.insert("content-range", "bytes */200".parse().unwrap());
        assert_eq!(content_range_start(&headers), None);
    }

    #[tokio::test]
    async fn finish_download_moves_the_file_into_place() {
        let dir = TempDir::new().unwrap();
        let temp = dir.path().join(".done.bin.downloading");
        let target = dir.path().join("done.bin");
        tokio::fs::write(&temp, b"payload").await.unwrap();

        finish_download(&temp, &target).await.unwrap();

        assert!(!temp.exists());
        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"payload");
        // A missing temp file is an error, not a silent success
        assert!(finish_download(&temp, &target).await.is_err());
    }

    /// `/dev/full` fails every write with ENOSPC, like a full disk
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_full_disk_is_reported_as_such_and_not_as_a_broken_pipe() {
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", "/big.bin")
            .with_status(200)
            .with_body("x".repeat(64 * 1024))
            .create_async()
            .await;
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("big.bin");
        std::os::unix::fs::symlink("/dev/full", create_temp_path(&target).unwrap()).unwrap();
        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url());
        let url = format!("{}/big.bin", server.url());

        let err = stream_to_temp(
            &client,
            &url,
            &target,
            "big.bin",
            Some(64 * 1024),
            1,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap_err();

        assert!(err.is_storage_full(), "{err}");
        assert!(!err.is_transient());
        assert!(err.to_string().contains("disk is full"), "{err}");
    }

    #[tokio::test]
    async fn a_stop_ends_the_wait_for_a_server_that_never_answers() {
        // Accepts the connection and never replies
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/hang.bin", listener.local_addr().unwrap());
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("hang.bin");
        let client = PutioApiClient::with_base_url("test_token".to_string(), url.clone());
        let stop = std::sync::Arc::new(StopSignal::default());
        let stopper = stop.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            stopper.stop();
        });

        let result = tokio::time::timeout(
            Duration::from_secs(5),
            stream_to_temp(
                &client,
                &url,
                &target,
                "hang.bin",
                None,
                1,
                None,
                None,
                Some(&stop),
                None,
            ),
        )
        .await
        .expect("the stop must end the wait");

        assert!(matches!(result, Err(DownloadError::Stopped)));
        drop(listener);
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

        let result = stream_download(
            &client,
            &url,
            &target_path,
            "test.txt",
            None,
            1,
            None,
            None,
            None,
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rate_limiter() {
        let limiter = RateLimiter::new(1024); // 1KB/sec

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
            Some(&RateLimiter::new(1024)),
            None,
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

    #[tokio::test]
    async fn time_held_by_a_pause_is_not_a_stall() {
        use putio_sync_core::bandwidth::{Override, OverrideMode};

        let temp_dir = TempDir::new().unwrap();
        let target_path = temp_dir.path().join("paused.txt");
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/paused.txt")
            .with_status(200)
            .with_body("x".repeat(64))
            .create_async()
            .await;
        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url());
        let url = format!("{}/paused.txt", server.url());

        // Paused for longer than the (test) stall timeout of 1 s
        let limiter = RateLimiter::unlimited();
        let now = chrono::Utc::now();
        limiter.set_override(Some(
            Override::for_minutes(OverrideMode::Pause, 60, now).unwrap(),
        ));
        let resume = tokio::spawn({
            let limiter = limiter.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(2500)).await;
                limiter.set_override(None);
            }
        });
        let start = Instant::now();
        let result = stream_download(
            &client,
            &url,
            &target_path,
            "paused.txt",
            Some(64),
            1,
            Some(&limiter),
            None,
            None,
        )
        .await
        .unwrap();
        resume.await.unwrap();

        assert!(result.success);
        assert!(start.elapsed() >= Duration::from_secs(2));
        mock.assert_async().await;
    }
}
