//! Streaming download functionality with buffering

use crate::api::ApiClient;
use crate::error::{CoreError, CoreResult};
use std::path::{Path, PathBuf};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncWriteExt, BufWriter};
use tracing::{debug, info, warn};

/// Default buffer size for streaming downloads (16MB)
pub const DEFAULT_BUFFER_SIZE: usize = 16 * 1024 * 1024;

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
/// * `expected_size` - Expected file size in bytes (for validation)
/// * `buffer_size_mb` - Buffer size in megabytes
///
/// # Returns
///
/// A `StreamDownloadResult` containing download statistics
pub async fn stream_download(
    client: &ApiClient,
    url: &str,
    target_path: &Path,
    expected_size: Option<u64>,
    buffer_size_mb: usize,
) -> CoreResult<StreamDownloadResult> {
    let buffer_size = buffer_size_mb * 1024 * 1024;

    debug!(
        "Starting stream download: {} -> {} (buffer: {}MB)",
        url,
        target_path.display(),
        buffer_size_mb
    );

    // Create temporary download path with .downloading extension and . prefix
    let temp_path = create_temp_download_path(target_path)?;

    // Ensure parent directory exists
    if let Some(parent) = temp_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            CoreError::IoError(std::io::Error::new(
                e.kind(),
                format!("Failed to create download directory: {}", e),
            ))
        })?;
    }

    // Make authenticated HTTP request
    let response = client
        .authenticated_request(url)?
        .send()
        .await
        .map_err(|e| CoreError::HttpError(e))?;

    if !response.status().is_success() {
        return Err(CoreError::ApiError(format!(
            "HTTP error {}: {}",
            response.status(),
            response.status().canonical_reason().unwrap_or("Unknown")
        )));
    }

    // Open file for writing with buffering
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temp_path)
        .await
        .map_err(|e| {
            CoreError::IoError(std::io::Error::new(
                e.kind(),
                format!("Failed to create download file: {}", e),
            ))
        })?;

    let mut writer = BufWriter::with_capacity(buffer_size, file);
    let mut bytes_downloaded = 0u64;

    // Stream the response body
    let mut stream = response.bytes_stream();

    use futures_util::StreamExt;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| CoreError::HttpError(e))?;

        writer.write_all(&chunk).await.map_err(|e| {
            CoreError::IoError(std::io::Error::new(
                e.kind(),
                format!("Failed to write to download file: {}", e),
            ))
        })?;

        bytes_downloaded += chunk.len() as u64;
    }

    // Flush any remaining buffered data
    writer.flush().await.map_err(|e| {
        CoreError::IoError(std::io::Error::new(
            e.kind(),
            format!("Failed to flush download file: {}", e),
        ))
    })?;

    // Ensure all data is written to disk
    writer.shutdown().await.map_err(|e| {
        CoreError::IoError(std::io::Error::new(
            e.kind(),
            format!("Failed to close download file: {}", e),
        ))
    })?;

    debug!(
        "Downloaded {} bytes to {}",
        bytes_downloaded,
        temp_path.display()
    );

    // Validate file size if expected size was provided
    if let Some(expected) = expected_size {
        if bytes_downloaded != expected {
            warn!(
                "Downloaded size ({}) doesn't match expected size ({})",
                bytes_downloaded, expected
            );
            // Clean up incomplete file
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(CoreError::ApiError(format!(
                "Size mismatch: downloaded {} bytes, expected {} bytes",
                bytes_downloaded, expected
            )));
        }
    }

    // Atomically rename to final path
    tokio::fs::rename(&temp_path, target_path)
        .await
        .map_err(|e| {
            CoreError::IoError(std::io::Error::new(
                e.kind(),
                format!("Failed to rename download file: {}", e),
            ))
        })?;

    info!(
        "Successfully downloaded {} bytes to {}",
        bytes_downloaded,
        target_path.display()
    );

    Ok(StreamDownloadResult {
        bytes_downloaded,
        final_path: target_path.to_path_buf(),
        success: true,
    })
}

/// Create a temporary download path with .downloading extension and . prefix
///
/// # Arguments
///
/// * `target_path` - The final destination path
///
/// # Returns
///
/// A path like `/path/to/.filename.ext.downloading`
fn create_temp_download_path(target_path: &Path) -> CoreResult<PathBuf> {
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

/// Check if a file is a partial download (has .downloading extension)
pub fn is_partial_download(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext == "downloading")
        .unwrap_or(false)
}

/// Get the original path from a partial download path
pub fn get_original_path(partial_path: &Path) -> Option<PathBuf> {
    if !is_partial_download(partial_path) {
        return None;
    }

    let parent = partial_path.parent();
    let stem = partial_path.file_stem()?.to_str()?;

    // Remove leading dot
    let original_name = stem.strip_prefix('.')?;

    let original_path = if let Some(parent) = parent {
        parent.join(original_name)
    } else {
        PathBuf::from(original_name)
    };

    Some(original_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_temp_download_path() {
        let target = PathBuf::from("/downloads/file.txt");
        let temp = create_temp_download_path(&target).unwrap();
        assert_eq!(temp, PathBuf::from("/downloads/.file.txt.downloading"));
    }

    #[test]
    fn test_create_temp_download_path_no_parent() {
        let target = PathBuf::from("file.txt");
        let temp = create_temp_download_path(&target).unwrap();
        assert_eq!(temp, PathBuf::from(".file.txt.downloading"));
    }

    #[test]
    fn test_is_partial_download() {
        assert!(is_partial_download(&PathBuf::from(
            ".file.txt.downloading"
        )));
        assert!(is_partial_download(&PathBuf::from(
            "/path/.file.txt.downloading"
        )));
        assert!(!is_partial_download(&PathBuf::from("file.txt")));
        assert!(!is_partial_download(&PathBuf::from(".file.txt")));
    }

    #[test]
    fn test_get_original_path() {
        let partial = PathBuf::from("/downloads/.file.txt.downloading");
        let original = get_original_path(&partial).unwrap();
        assert_eq!(original, PathBuf::from("/downloads/file.txt"));
    }

    #[test]
    fn test_get_original_path_no_parent() {
        let partial = PathBuf::from(".file.txt.downloading");
        let original = get_original_path(&partial).unwrap();
        assert_eq!(original, PathBuf::from("file.txt"));
    }

    #[test]
    fn test_get_original_path_not_partial() {
        let path = PathBuf::from("/downloads/file.txt");
        assert!(get_original_path(&path).is_none());
    }

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

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let url = format!("{}/test-file.txt", server.url());

        let result = stream_download(&client, &url, &target_path, Some(13), 1)
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

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let url = format!("{}/test-file.txt", server.url());

        // Expect 10 bytes but will only get 5
        let result = stream_download(&client, &url, &target_path, Some(10), 1).await;

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

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let url = format!("{}/not-found.txt", server.url());

        let result = stream_download(&client, &url, &target_path, None, 1).await;

        assert!(result.is_err());
    }
}
