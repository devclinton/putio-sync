//! Local file handler implementations

use crate::error::{ApiError, ApiResult};
use crate::models::{ListResponse, SuccessResponse};
use putio_sync_core::config::Config;
use serde::{Deserialize, Serialize};
use std::fs;

/// Local file information response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalFileResponse {
    /// File name
    pub name: String,
    /// File path (relative to download directory)
    pub path: String,
    /// File size in bytes
    pub size: u64,
    /// Is directory
    pub is_dir: bool,
    /// Modified time (Unix timestamp)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<i64>,
}

/// List local files in the download directory
///
/// # Arguments
///
/// * `config` - Application configuration
/// * `path` - Optional subdirectory path
///
/// # Returns
///
/// List of local files
pub async fn list_local_files(
    config: &Config,
    path: Option<String>,
) -> ApiResult<ListResponse<LocalFileResponse>> {
    // Get download directory from config
    let base_dir = &config.download.target_directory;

    // Construct full path
    let full_path = if let Some(subpath) = path {
        base_dir.join(subpath)
    } else {
        base_dir.clone()
    };

    // Check if path exists and is a directory
    if !full_path.exists() {
        return Err(ApiError::NotFound(format!(
            "Path does not exist: {}",
            full_path.display()
        )));
    }

    if !full_path.is_dir() {
        return Err(ApiError::BadRequest(format!(
            "Path is not a directory: {}",
            full_path.display()
        )));
    }

    // Read directory entries
    let entries = fs::read_dir(&full_path)
        .map_err(|e| ApiError::Internal(format!("Failed to read directory: {}", e)))?;

    let mut files = Vec::new();

    for entry in entries {
        let entry = entry
            .map_err(|e| ApiError::Internal(format!("Failed to read directory entry: {}", e)))?;

        let metadata = entry
            .metadata()
            .map_err(|e| ApiError::Internal(format!("Failed to read metadata: {}", e)))?;

        let name = entry.file_name().to_string_lossy().to_string();
        let relative_path = entry
            .path()
            .strip_prefix(base_dir)
            .unwrap_or(&entry.path())
            .to_string_lossy()
            .to_string();

        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);

        files.push(LocalFileResponse {
            name,
            path: relative_path,
            size: metadata.len(),
            is_dir: metadata.is_dir(),
            modified,
        });
    }

    // Sort by name
    files.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(ListResponse::new(files))
}

/// Delete a local file
///
/// # Arguments
///
/// * `config` - Application configuration
/// * `path` - File path relative to download directory
///
/// # Returns
///
/// Success response
pub async fn delete_local_file(config: &Config, path: String) -> ApiResult<SuccessResponse> {
    // Get download directory from config
    let base_dir = &config.download.target_directory;

    // Construct full path
    let full_path = base_dir.join(&path);

    // Security check: ensure the resolved path is still within base_dir
    let canonical_base = fs::canonicalize(base_dir)
        .map_err(|e| ApiError::Internal(format!("Failed to canonicalize base path: {}", e)))?;

    let canonical_target = fs::canonicalize(&full_path)
        .map_err(|e| ApiError::NotFound(format!("File not found: {} ({})", path, e)))?;

    if !canonical_target.starts_with(&canonical_base) {
        return Err(ApiError::BadRequest(
            "Path escapes download directory".to_string(),
        ));
    }

    // Delete file or directory
    if canonical_target.is_dir() {
        fs::remove_dir_all(&canonical_target)
            .map_err(|e| ApiError::Internal(format!("Failed to delete directory: {}", e)))?;
    } else {
        fs::remove_file(&canonical_target)
            .map_err(|e| ApiError::Internal(format!("Failed to delete file: {}", e)))?;
    }

    Ok(SuccessResponse {
        message: format!("Successfully deleted: {}", path),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_list_local_files() {
        // Create temporary directory with some files
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_path_buf();

        // Create test files
        fs::write(temp_path.join("file1.txt"), "test1").unwrap();
        fs::write(temp_path.join("file2.txt"), "test2").unwrap();
        fs::create_dir(temp_path.join("subdir")).unwrap();

        // Create config
        let mut config = Config::default();
        config.download.target_directory = temp_path;

        // List files
        let result = list_local_files(&config, None).await;
        assert!(result.is_ok());

        let files = result.unwrap().items;
        assert_eq!(files.len(), 3);
    }

    #[tokio::test]
    async fn test_list_local_files_nonexistent() {
        let config = Config::default();
        let result = list_local_files(&config, Some("nonexistent".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_local_file() {
        // Create temporary directory with a file
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_path_buf();
        let test_file = "test.txt";
        fs::write(temp_path.join(test_file), "test").unwrap();

        // Create config
        let mut config = Config::default();
        config.download.target_directory = temp_path.clone();

        // Delete file
        let result = delete_local_file(&config, test_file.to_string()).await;
        assert!(result.is_ok());

        // Verify file is deleted
        assert!(!temp_path.join(test_file).exists());
    }
}
