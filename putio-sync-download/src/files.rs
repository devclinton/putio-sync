//! File utilities for download management

use std::path::{Path, PathBuf};

// Error type will need to be updated to use the new crate's error type
use crate::error::{DownloadError, DownloadResult};

/// Create a temporary download path with .downloading extension and . prefix
///
/// # Arguments
///
/// * `target_path` - The final destination path
///
/// # Returns
///
/// A path like `/path/to/.filename.ext.downloading`
pub fn create_temp_path(target_path: &Path) -> DownloadResult<PathBuf> {
    let parent = target_path.parent();
    let filename = target_path
        .file_name()
        .ok_or_else(|| DownloadError::InvalidInput("Invalid file path".to_string()))?
        .to_str()
        .ok_or_else(|| DownloadError::InvalidInput("Invalid filename encoding".to_string()))?;

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

/// Check if a download can be resumed
///
/// Returns the number of bytes already downloaded if resume is possible
pub async fn check_resume(
    temp_path: &Path,
    expected_size: u64,
) -> DownloadResult<Option<u64>> {
    if temp_path.exists() && is_partial_download(temp_path) {
        // Get the current size of the partial file
        match tokio::fs::metadata(temp_path).await {
            Ok(metadata) => {
                let partial_size = metadata.len();
                if partial_size > 0 && partial_size < expected_size {
                    Ok(Some(partial_size))
                } else if partial_size >= expected_size {
                    // File appears to be complete, clean it up and start fresh
                    let _ = tokio::fs::remove_file(temp_path).await;
                    Ok(None)
                } else {
                    // Empty partial file, clean it up
                    let _ = tokio::fs::remove_file(temp_path).await;
                    Ok(None)
                }
            }
            Err(_) => {
                // Can't read metadata, clean up and start fresh
                let _ = tokio::fs::remove_file(temp_path).await;
                Ok(None)
            }
        }
    } else {
        Ok(None)
    }
}

/// Clean up a partial download file
pub async fn cleanup_partial(path: &Path) {
    use tracing::warn;
    
    if path.exists() {
        if let Err(e) = tokio::fs::remove_file(path).await {
            warn!("Failed to clean up partial download {}: {}", path.display(), e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_temp_path() {
        let target = PathBuf::from("/downloads/file.txt");
        let temp = create_temp_path(&target).unwrap();
        assert_eq!(temp, PathBuf::from("/downloads/.file.txt.downloading"));
    }

    #[test]
    fn test_create_temp_path_no_parent() {
        let target = PathBuf::from("file.txt");
        let temp = create_temp_path(&target).unwrap();
        assert_eq!(temp, PathBuf::from(".file.txt.downloading"));
    }

    #[test]
    fn test_is_partial_download() {
        assert!(is_partial_download(&PathBuf::from(".file.txt.downloading")));
        assert!(is_partial_download(&PathBuf::from("/path/.file.txt.downloading")));
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
}
