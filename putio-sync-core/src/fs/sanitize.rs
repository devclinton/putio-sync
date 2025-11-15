//! Filename sanitization for cross-platform compatibility
//!
//! This module provides functions to sanitize filenames and paths
//! to ensure they work correctly on Windows, Linux, and macOS.

use std::path::{Path, PathBuf};

/// Reserved names on Windows that cannot be used as filenames
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Characters that are invalid in filenames on various platforms
const INVALID_CHARS: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\0'];

/// Maximum filename length (conservative limit that works on all platforms)
const MAX_FILENAME_LENGTH: usize = 255;

/// Maximum path length (conservative limit for Windows)
const MAX_PATH_LENGTH: usize = 260;

/// Sanitize a filename to be safe for use on all platforms
///
/// This function:
/// - Replaces invalid characters with underscores
/// - Handles Windows reserved names
/// - Truncates names that are too long
/// - Preserves Unicode characters where safe
/// - Is reproducible (same input always produces same output)
///
/// # Arguments
///
/// * `filename` - The filename to sanitize
///
/// # Returns
///
/// A sanitized filename that is safe to use on Windows, Linux, and macOS
///
/// # Examples
///
/// ```
/// use putio_sync_core::fs::sanitize_filename;
///
/// let safe = sanitize_filename("file:name?.txt");
/// assert_eq!(safe, "file_name_.txt");
///
/// let safe = sanitize_filename("CON.txt");
/// assert_eq!(safe, "_CON.txt");
/// ```
pub fn sanitize_filename(filename: &str) -> String {
    if filename.is_empty() {
        return "unnamed".to_string();
    }

    // Replace invalid characters with underscores
    let mut sanitized: String = filename
        .chars()
        .map(|c| if INVALID_CHARS.contains(&c) { '_' } else { c })
        .collect();

    // Remove leading/trailing whitespace and dots
    sanitized = sanitized.trim().trim_matches('.').to_string();

    // Handle empty string after trimming
    if sanitized.is_empty() {
        return "unnamed".to_string();
    }

    // Check for Windows reserved names (case-insensitive)
    let name_upper = sanitized.to_uppercase();
    let base_name = name_upper.split('.').next().unwrap_or(&name_upper);

    if WINDOWS_RESERVED_NAMES.contains(&base_name) {
        sanitized = format!("_{}", sanitized);
    }

    // Truncate if too long, preserving extension
    if sanitized.len() > MAX_FILENAME_LENGTH {
        let path = Path::new(&sanitized);
        let extension = path.extension().and_then(|e| e.to_str());
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

        let ext_len = extension.map(|e| e.len() + 1).unwrap_or(0); // +1 for the dot
        let max_stem_len = MAX_FILENAME_LENGTH.saturating_sub(ext_len);

        let truncated_stem = if stem.len() > max_stem_len {
            // Find a safe truncation point (try to break at a character boundary)
            let mut truncate_at = max_stem_len;
            while truncate_at > 0 && !stem.is_char_boundary(truncate_at) {
                truncate_at -= 1;
            }
            &stem[..truncate_at]
        } else {
            stem
        };

        sanitized = if let Some(ext) = extension {
            format!("{}.{}", truncated_stem, ext)
        } else {
            truncated_stem.to_string()
        };
    }

    sanitized
}

/// Sanitize a full path (directory + filename)
///
/// This function sanitizes each component of a path separately,
/// then joins them back together.
///
/// # Arguments
///
/// * `path` - The path to sanitize
///
/// # Returns
///
/// A sanitized path that is safe to use on all platforms
///
/// # Examples
///
/// ```
/// use putio_sync_core::fs::sanitize::sanitize_path;
/// use std::path::PathBuf;
///
/// let safe = sanitize_path(&PathBuf::from("dir/sub:dir/file?.txt"));
/// assert_eq!(safe.to_string_lossy(), "dir/sub_dir/file_.txt");
/// ```
pub fn sanitize_path(path: &Path) -> PathBuf {
    let components: Vec<String> = path
        .components()
        .filter_map(|comp| comp.as_os_str().to_str())
        .map(sanitize_filename)
        .collect();

    let sanitized_path: PathBuf = components.iter().collect();

    // Check total path length
    let path_str = sanitized_path.to_string_lossy();
    if path_str.len() > MAX_PATH_LENGTH {
        // Truncate the last component (filename) to fit
        let parent = sanitized_path.parent();
        if let Some(filename) = sanitized_path.file_name().and_then(|f| f.to_str()) {
            let parent_len = parent
                .map(|p| p.to_string_lossy().len() + 1)
                .unwrap_or(0);
            let max_filename_len = MAX_PATH_LENGTH.saturating_sub(parent_len);

            let truncated = if filename.len() > max_filename_len {
                let path = Path::new(filename);
                let extension = path.extension().and_then(|e| e.to_str());
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

                let ext_len = extension.map(|e| e.len() + 1).unwrap_or(0);
                let max_stem_len = max_filename_len.saturating_sub(ext_len);

                let truncated_stem = &stem[..max_stem_len.min(stem.len())];

                if let Some(ext) = extension {
                    format!("{}.{}", truncated_stem, ext)
                } else {
                    truncated_stem.to_string()
                }
            } else {
                filename.to_string()
            };

            if let Some(parent) = parent {
                return parent.join(truncated);
            } else {
                return PathBuf::from(truncated);
            }
        }
    }

    sanitized_path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename_basic() {
        assert_eq!(sanitize_filename("normal.txt"), "normal.txt");
        assert_eq!(sanitize_filename("file with spaces.txt"), "file with spaces.txt");
    }

    #[test]
    fn test_sanitize_filename_invalid_chars() {
        assert_eq!(sanitize_filename("file:name.txt"), "file_name.txt");
        assert_eq!(sanitize_filename("file/name.txt"), "file_name.txt");
        assert_eq!(sanitize_filename("file\\name.txt"), "file_name.txt");
        assert_eq!(sanitize_filename("file*name.txt"), "file_name.txt");
        assert_eq!(sanitize_filename("file?name.txt"), "file_name.txt");
        assert_eq!(sanitize_filename("file\"name.txt"), "file_name.txt");
        assert_eq!(sanitize_filename("file<name.txt"), "file_name.txt");
        assert_eq!(sanitize_filename("file>name.txt"), "file_name.txt");
        assert_eq!(sanitize_filename("file|name.txt"), "file_name.txt");
    }

    #[test]
    fn test_sanitize_filename_windows_reserved() {
        assert_eq!(sanitize_filename("CON"), "_CON");
        assert_eq!(sanitize_filename("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_filename("con.txt"), "_con.txt");
        assert_eq!(sanitize_filename("PRN"), "_PRN");
        assert_eq!(sanitize_filename("AUX.log"), "_AUX.log");
        assert_eq!(sanitize_filename("NUL"), "_NUL");
        assert_eq!(sanitize_filename("COM1.txt"), "_COM1.txt");
        assert_eq!(sanitize_filename("LPT1.dat"), "_LPT1.dat");
    }

    #[test]
    fn test_sanitize_filename_not_reserved() {
        // These should not be treated as reserved
        assert_eq!(sanitize_filename("CONWAY.txt"), "CONWAY.txt");
        assert_eq!(sanitize_filename("PRINTER.txt"), "PRINTER.txt");
        assert_eq!(sanitize_filename("NULL.txt"), "NULL.txt");
    }

    #[test]
    fn test_sanitize_filename_empty() {
        assert_eq!(sanitize_filename(""), "unnamed");
        assert_eq!(sanitize_filename("   "), "unnamed");
        assert_eq!(sanitize_filename("..."), "unnamed");
    }

    #[test]
    fn test_sanitize_filename_trim() {
        assert_eq!(sanitize_filename("  file.txt  "), "file.txt");
        assert_eq!(sanitize_filename(".hidden"), "hidden");
        assert_eq!(sanitize_filename("file."), "file");
    }

    #[test]
    fn test_sanitize_filename_long() {
        let long_name = "a".repeat(300);
        let sanitized = sanitize_filename(&long_name);
        assert!(sanitized.len() <= MAX_FILENAME_LENGTH);
        assert_eq!(sanitized.len(), MAX_FILENAME_LENGTH);
    }

    #[test]
    fn test_sanitize_filename_long_with_extension() {
        let long_stem = "a".repeat(300);
        let filename = format!("{}.txt", long_stem);
        let sanitized = sanitize_filename(&filename);

        assert!(sanitized.len() <= MAX_FILENAME_LENGTH);
        assert!(sanitized.ends_with(".txt"));

        // Should be 255 total: 251 chars + ".txt" (4 chars)
        assert_eq!(sanitized.len(), MAX_FILENAME_LENGTH);
    }

    #[test]
    fn test_sanitize_filename_unicode() {
        assert_eq!(sanitize_filename("файл.txt"), "файл.txt");
        assert_eq!(sanitize_filename("文件.txt"), "文件.txt");
        assert_eq!(sanitize_filename("ファイル.txt"), "ファイル.txt");
        assert_eq!(sanitize_filename("αρχείο.txt"), "αρχείο.txt");
    }

    #[test]
    fn test_sanitize_filename_reproducible() {
        let input = "test:file?.txt";
        let result1 = sanitize_filename(input);
        let result2 = sanitize_filename(input);
        assert_eq!(result1, result2);
    }

    #[test]
    fn test_sanitize_path_basic() {
        let path = PathBuf::from("dir/subdir/file.txt");
        let sanitized = sanitize_path(&path);
        assert_eq!(sanitized.to_string_lossy(), "dir/subdir/file.txt");
    }

    #[test]
    fn test_sanitize_path_invalid_chars() {
        let path = PathBuf::from("dir/sub:dir/file?.txt");
        let sanitized = sanitize_path(&path);
        assert_eq!(sanitized.to_string_lossy(), "dir/sub_dir/file_.txt");
    }

    #[test]
    fn test_sanitize_path_windows_reserved() {
        let path = PathBuf::from("folder/CON/file.txt");
        let sanitized = sanitize_path(&path);
        assert_eq!(sanitized.to_string_lossy(), "folder/_CON/file.txt");
    }

    #[test]
    fn test_sanitize_path_empty_component() {
        let sanitized = sanitize_filename("");
        assert_eq!(sanitized, "unnamed");
    }
}
