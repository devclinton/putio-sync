//! Chunked download support for large files

use crate::api::ApiClient;
use crate::error::{CoreError, CoreResult};
use std::path::{Path, PathBuf};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};
use tracing::{debug, warn};

/// Default chunk size for large files (10MB)
pub const DEFAULT_CHUNK_SIZE: u64 = 10 * 1024 * 1024;

/// Minimum file size to trigger chunked download (50MB)
pub const MIN_CHUNK_FILE_SIZE: u64 = 50 * 1024 * 1024;

/// Represents a chunk of a file to download
#[derive(Debug, Clone)]
pub struct Chunk {
    /// Chunk number (0-indexed)
    pub number: u64,
    /// Start byte offset
    pub start: u64,
    /// End byte offset (inclusive)
    pub end: u64,
    /// Whether this chunk is complete
    pub complete: bool,
}

impl Chunk {
    /// Get the size of this chunk in bytes
    pub fn size(&self) -> u64 {
        self.end - self.start + 1
    }
}

/// Calculate chunks for a file
///
/// # Arguments
///
/// * `file_size` - Total size of the file in bytes
/// * `chunk_size` - Size of each chunk in bytes
///
/// # Returns
///
/// A vector of chunks to download
pub fn calculate_chunks(file_size: u64, chunk_size: u64) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut offset = 0u64;
    let mut chunk_number = 0u64;

    while offset < file_size {
        let end = std::cmp::min(offset + chunk_size - 1, file_size - 1);
        chunks.push(Chunk {
            number: chunk_number,
            start: offset,
            end,
            complete: false,
        });

        offset = end + 1;
        chunk_number += 1;
    }

    chunks
}

/// Download a single chunk to a file
///
/// # Arguments
///
/// * `client` - HTTP client
/// * `url` - URL to download from
/// * `chunk` - Chunk to download
/// * `file_path` - Path to write the chunk to
/// * `buffer_size` - Buffer size for writing
///
/// # Returns
///
/// Number of bytes downloaded
pub async fn download_chunk(
    client: &ApiClient,
    url: &str,
    chunk: &Chunk,
    file_path: &Path,
    buffer_size: usize,
) -> CoreResult<u64> {
    debug!(
        "Downloading chunk {} (bytes {}-{}, {} bytes)",
        chunk.number,
        chunk.start,
        chunk.end,
        chunk.size()
    );

    // Make authenticated HTTP request with Range header
    let range_header = format!("bytes={}-{}", chunk.start, chunk.end);
    let response = client
        .authenticated_request(url)?
        .header("Range", range_header)
        .send()
        .await
        .map_err(|e| CoreError::HttpError(e))?;

    // Check for 206 Partial Content response
    let status = response.status();
    if !status.is_success() && status.as_u16() != 206 {
        return Err(CoreError::ApiError(format!(
            "HTTP error {}: {}",
            status,
            status.canonical_reason().unwrap_or("Unknown")
        )));
    }

    // Open file for writing at the chunk offset
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(file_path)
        .await
        .map_err(|e| {
            CoreError::IoError(std::io::Error::new(
                e.kind(),
                format!("Failed to open file for chunk write: {}", e),
            ))
        })?;

    let mut writer = BufWriter::with_capacity(buffer_size, file);

    // Seek to the chunk offset
    writer
        .seek(std::io::SeekFrom::Start(chunk.start))
        .await
        .map_err(|e| {
            CoreError::IoError(std::io::Error::new(
                e.kind(),
                format!("Failed to seek to chunk offset: {}", e),
            ))
        })?;

    // Stream the chunk data
    let mut stream = response.bytes_stream();
    let mut bytes_downloaded = 0u64;

    use futures_util::StreamExt;

    while let Some(chunk_data) = stream.next().await {
        let chunk_data = chunk_data.map_err(|e| CoreError::HttpError(e))?;

        writer.write_all(&chunk_data).await.map_err(|e| {
            CoreError::IoError(std::io::Error::new(
                e.kind(),
                format!("Failed to write chunk data: {}", e),
            ))
        })?;

        bytes_downloaded += chunk_data.len() as u64;
    }

    // Flush and close
    writer.flush().await.map_err(|e| {
        CoreError::IoError(std::io::Error::new(
            e.kind(),
            format!("Failed to flush chunk data: {}", e),
        ))
    })?;

    writer.shutdown().await.map_err(|e| {
        CoreError::IoError(std::io::Error::new(
            e.kind(),
            format!("Failed to close chunk file: {}", e),
        ))
    })?;

    // Validate chunk size
    if bytes_downloaded != chunk.size() {
        warn!(
            "Chunk {} size mismatch: downloaded {}, expected {}",
            chunk.number,
            bytes_downloaded,
            chunk.size()
        );
        return Err(CoreError::ApiError(format!(
            "Chunk size mismatch: downloaded {} bytes, expected {} bytes",
            bytes_downloaded,
            chunk.size()
        )));
    }

    debug!("Chunk {} downloaded successfully", chunk.number);
    Ok(bytes_downloaded)
}

/// Check if server supports range requests
///
/// # Arguments
///
/// * `client` - HTTP client
/// * `url` - URL to check
///
/// # Returns
///
/// `true` if the server supports range requests
pub async fn supports_range_requests(client: &ApiClient, url: &str) -> bool {
    // Use authenticated HEAD request
    match client.authenticated_head_request(url) {
        Ok(request_builder) => {
            match request_builder.send().await {
                Ok(response) => {
                    // Check for Accept-Ranges header
                    if let Some(accept_ranges) = response.headers().get("Accept-Ranges") {
                        if let Ok(value) = accept_ranges.to_str() {
                            return value != "none";
                        }
                    }
                    false
                }
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

/// Resume information for a partial download
#[derive(Debug)]
pub struct ResumeInfo {
    /// Path to the partial download file
    pub file_path: PathBuf,
    /// Chunks that have been completed
    pub completed_chunks: Vec<u64>,
    /// Total number of chunks
    pub total_chunks: u64,
    /// Bytes downloaded so far
    pub bytes_downloaded: u64,
    /// Total file size
    pub total_bytes: u64,
}

impl ResumeInfo {
    /// Get the next chunk to download
    pub fn next_chunk(&self, all_chunks: &[Chunk]) -> Option<Chunk> {
        all_chunks
            .iter()
            .find(|c| !self.completed_chunks.contains(&c.number))
            .cloned()
    }

    /// Get progress as a percentage
    pub fn percentage(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.bytes_downloaded as f64 / self.total_bytes as f64) * 100.0
    }

    /// Check if download is complete
    pub fn is_complete(&self) -> bool {
        self.completed_chunks.len() as u64 >= self.total_chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_chunks() {
        let chunks = calculate_chunks(100, 30);
        assert_eq!(chunks.len(), 4);

        assert_eq!(chunks[0].start, 0);
        assert_eq!(chunks[0].end, 29);
        assert_eq!(chunks[0].size(), 30);

        assert_eq!(chunks[1].start, 30);
        assert_eq!(chunks[1].end, 59);
        assert_eq!(chunks[1].size(), 30);

        assert_eq!(chunks[2].start, 60);
        assert_eq!(chunks[2].end, 89);
        assert_eq!(chunks[2].size(), 30);

        assert_eq!(chunks[3].start, 90);
        assert_eq!(chunks[3].end, 99);
        assert_eq!(chunks[3].size(), 10);
    }

    #[test]
    fn test_calculate_chunks_exact_multiple() {
        let chunks = calculate_chunks(90, 30);
        assert_eq!(chunks.len(), 3);

        assert_eq!(chunks[0].size(), 30);
        assert_eq!(chunks[1].size(), 30);
        assert_eq!(chunks[2].size(), 30);
    }

    #[test]
    fn test_calculate_chunks_single_chunk() {
        let chunks = calculate_chunks(50, 100);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].start, 0);
        assert_eq!(chunks[0].end, 49);
        assert_eq!(chunks[0].size(), 50);
    }

    #[test]
    fn test_chunk_size() {
        let chunk = Chunk {
            number: 0,
            start: 0,
            end: 99,
            complete: false,
        };
        assert_eq!(chunk.size(), 100);

        let chunk2 = Chunk {
            number: 1,
            start: 100,
            end: 199,
            complete: false,
        };
        assert_eq!(chunk2.size(), 100);
    }

    #[test]
    fn test_resume_info_next_chunk() {
        let chunks = vec![
            Chunk {
                number: 0,
                start: 0,
                end: 99,
                complete: false,
            },
            Chunk {
                number: 1,
                start: 100,
                end: 199,
                complete: false,
            },
            Chunk {
                number: 2,
                start: 200,
                end: 299,
                complete: false,
            },
        ];

        let resume = ResumeInfo {
            file_path: PathBuf::from("test.txt"),
            completed_chunks: vec![0, 2],
            total_chunks: 3,
            bytes_downloaded: 200,
            total_bytes: 300,
        };

        let next = resume.next_chunk(&chunks).unwrap();
        assert_eq!(next.number, 1);
    }

    #[test]
    fn test_resume_info_percentage() {
        let resume = ResumeInfo {
            file_path: PathBuf::from("test.txt"),
            completed_chunks: vec![0, 1],
            total_chunks: 4,
            bytes_downloaded: 200,
            total_bytes: 400,
        };

        assert_eq!(resume.percentage(), 50.0);
    }

    #[test]
    fn test_resume_info_is_complete() {
        let mut resume = ResumeInfo {
            file_path: PathBuf::from("test.txt"),
            completed_chunks: vec![0, 1],
            total_chunks: 3,
            bytes_downloaded: 200,
            total_bytes: 300,
        };

        assert!(!resume.is_complete());

        resume.completed_chunks.push(2);
        assert!(resume.is_complete());
    }

    #[tokio::test]
    async fn test_supports_range_requests() {
        use mockito::Server;

        let mut server = Server::new_async().await;
        let mock = server
            .mock("HEAD", "/test-file.txt")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("Accept-Ranges", "bytes")
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let url = format!("{}/test-file.txt", server.url());

        let supports = supports_range_requests(&client, &url).await;
        assert!(supports);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_supports_range_requests_not_supported() {
        use mockito::Server;

        let mut server = Server::new_async().await;
        let _mock = server
            .mock("HEAD", "/test-file.txt")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let url = format!("{}/test-file.txt", server.url());

        let supports = supports_range_requests(&client, &url).await;
        assert!(!supports);
    }

    #[tokio::test]
    async fn test_download_chunk() {
        use mockito::Server;
        use tempfile::TempDir;

        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/test-file.txt")
            .match_header("authorization", "Bearer test_token")
            .match_header("Range", "bytes=0-9")
            .with_status(206)
            .with_body("0123456789")
            .create_async()
            .await;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        // Pre-create the file
        tokio::fs::write(&file_path, vec![0u8; 20]).await.unwrap();

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let url = format!("{}/test-file.txt", server.url());

        let chunk = Chunk {
            number: 0,
            start: 0,
            end: 9,
            complete: false,
        };

        let bytes = download_chunk(&client, &url, &chunk, &file_path, 1024 * 1024)
            .await
            .unwrap();

        assert_eq!(bytes, 10);

        // Verify the chunk was written correctly
        let contents = tokio::fs::read(&file_path).await.unwrap();
        assert_eq!(&contents[0..10], b"0123456789");

        mock.assert_async().await;
    }
}
