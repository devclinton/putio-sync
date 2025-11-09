//! File filtering and sorting logic
//!
//! This module provides client-side filtering and sorting capabilities
//! for file listings from the Put.io API.

use crate::api::FileEntry;
use glob::Pattern;
use std::cmp::Ordering;
use std::str::FromStr;

/// Sort field options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    /// Sort by file name
    Name,
    /// Sort by file size
    Size,
    /// Sort by creation date
    Date,
}

impl FromStr for SortField {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "name" => Ok(Self::Name),
            "size" => Ok(Self::Size),
            "date" | "created" => Ok(Self::Date),
            _ => Err(format!("Invalid sort field: {}", s)),
        }
    }
}

/// File type filter options
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileTypeFilter {
    /// Only files
    File,
    /// Only folders
    Folder,
    /// Only video files
    Video,
    /// Only audio files
    Audio,
    /// Only image files
    Image,
    /// Only archive files
    Archive,
}

impl FromStr for FileTypeFilter {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "file" => Ok(Self::File),
            "folder" => Ok(Self::Folder),
            "video" => Ok(Self::Video),
            "audio" => Ok(Self::Audio),
            "image" => Ok(Self::Image),
            "archive" => Ok(Self::Archive),
            _ => Err(format!("Invalid file type filter: {}", s)),
        }
    }
}

impl FileTypeFilter {
    /// Check if a file matches this type filter
    pub fn matches(&self, file: &FileEntry) -> bool {
        match self {
            Self::Folder => file.file_type == "FOLDER",
            Self::File => file.file_type != "FOLDER",
            Self::Video => file.file_type == "VIDEO",
            Self::Audio => file.file_type == "AUDIO",
            Self::Image => file.file_type == "IMAGE",
            Self::Archive => {
                // Check for common archive extensions
                if let Some(ext) = &file.extension {
                    matches!(
                        ext.to_lowercase().as_str(),
                        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "tgz"
                    )
                } else {
                    false
                }
            }
        }
    }
}

/// File filter builder
#[derive(Debug, Clone, Default)]
pub struct FileFilter {
    /// Name pattern (glob)
    name_pattern: Option<String>,
    /// File type filter
    file_type: Option<FileTypeFilter>,
    /// Minimum file size in bytes
    min_size: Option<i64>,
    /// Maximum file size in bytes
    max_size: Option<i64>,
}

impl FileFilter {
    /// Create a new empty file filter
    pub fn new() -> Self {
        Self::default()
    }

    /// Set name pattern filter (glob pattern)
    pub fn with_name_pattern(mut self, pattern: String) -> Self {
        self.name_pattern = Some(pattern);
        self
    }

    /// Set file type filter
    pub fn with_file_type(mut self, file_type: FileTypeFilter) -> Self {
        self.file_type = Some(file_type);
        self
    }

    /// Set minimum file size
    pub fn with_min_size(mut self, size: i64) -> Self {
        self.min_size = Some(size);
        self
    }

    /// Set maximum file size
    pub fn with_max_size(mut self, size: i64) -> Self {
        self.max_size = Some(size);
        self
    }

    /// Check if a file matches all filters
    pub fn matches(&self, file: &FileEntry) -> bool {
        // Check name pattern
        if let Some(pattern) = &self.name_pattern {
            if let Ok(glob_pattern) = Pattern::new(pattern) {
                if !glob_pattern.matches(&file.name) {
                    return false;
                }
            }
        }

        // Check file type
        if let Some(file_type) = &self.file_type {
            if !file_type.matches(file) {
                return false;
            }
        }

        // Check minimum size
        if let Some(min_size) = self.min_size {
            if file.size < min_size {
                return false;
            }
        }

        // Check maximum size
        if let Some(max_size) = self.max_size {
            if file.size > max_size {
                return false;
            }
        }

        true
    }

    /// Filter a list of files
    pub fn filter(&self, files: Vec<FileEntry>) -> Vec<FileEntry> {
        files.into_iter().filter(|f| self.matches(f)).collect()
    }
}

/// Parse human-readable file size (e.g., "100MB", "1.5GB")
pub fn parse_size(s: &str) -> Result<i64, String> {
    let s = s.trim().to_uppercase();

    // Find where the number ends and unit begins
    let num_end = s
        .chars()
        .position(|c| !c.is_numeric() && c != '.')
        .unwrap_or(s.len());

    let (num_str, unit_str) = s.split_at(num_end);

    let num: f64 = num_str
        .parse()
        .map_err(|_| format!("Invalid number: {}", num_str))?;

    let multiplier: f64 = match unit_str.trim() {
        "" | "B" => 1.0,
        "KB" | "K" => 1024.0,
        "MB" | "M" => 1024.0 * 1024.0,
        "GB" | "G" => 1024.0 * 1024.0 * 1024.0,
        "TB" | "T" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return Err(format!("Invalid size unit: {}", unit_str)),
    };

    Ok((num * multiplier) as i64)
}

/// Sort files by specified field
pub fn sort_files(mut files: Vec<FileEntry>, field: SortField, reverse: bool) -> Vec<FileEntry> {
    files.sort_by(|a, b| {
        let ordering = match field {
            SortField::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortField::Size => a.size.cmp(&b.size),
            SortField::Date => {
                // Parse created_at timestamps for comparison
                match (&a.created_at, &b.created_at) {
                    (Some(a_time), Some(b_time)) => a_time.cmp(b_time),
                    (Some(_), None) => Ordering::Greater,
                    (None, Some(_)) => Ordering::Less,
                    (None, None) => Ordering::Equal,
                }
            }
        };

        if reverse {
            ordering.reverse()
        } else {
            ordering
        }
    });

    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_file(
        name: &str,
        size: i64,
        file_type: &str,
        extension: Option<&str>,
    ) -> FileEntry {
        FileEntry {
            id: 1,
            name: name.to_string(),
            size,
            file_type: file_type.to_string(),
            parent_id: 0,
            content_type: None,
            created_at: Some("2025-11-01T10:00:00Z".to_string()),
            first_accessed_at: None,
            is_folder: file_type == "FOLDER",
            is_shared: false,
            screenshot: None,
            opensubtitles_hash: None,
            crc32: None,
            extension: extension.map(|s| s.to_string()),
        }
    }

    #[test]
    fn test_sort_field_from_str() {
        assert_eq!(SortField::from_str("name"), Ok(SortField::Name));
        assert_eq!(SortField::from_str("size"), Ok(SortField::Size));
        assert_eq!(SortField::from_str("date"), Ok(SortField::Date));
        assert_eq!(SortField::from_str("created"), Ok(SortField::Date));
        assert!(SortField::from_str("invalid").is_err());
    }

    #[test]
    fn test_file_type_filter_from_str() {
        assert_eq!(FileTypeFilter::from_str("file"), Ok(FileTypeFilter::File));
        assert_eq!(
            FileTypeFilter::from_str("folder"),
            Ok(FileTypeFilter::Folder)
        );
        assert_eq!(FileTypeFilter::from_str("video"), Ok(FileTypeFilter::Video));
        assert_eq!(FileTypeFilter::from_str("audio"), Ok(FileTypeFilter::Audio));
        assert_eq!(FileTypeFilter::from_str("image"), Ok(FileTypeFilter::Image));
        assert_eq!(
            FileTypeFilter::from_str("archive"),
            Ok(FileTypeFilter::Archive)
        );
        assert!(FileTypeFilter::from_str("invalid").is_err());
    }

    #[test]
    fn test_file_type_filter_matches() {
        let folder = create_test_file("test", 0, "FOLDER", None);
        let video = create_test_file("test.mp4", 1000, "VIDEO", Some("mp4"));
        let audio = create_test_file("test.mp3", 500, "AUDIO", Some("mp3"));
        let image = create_test_file("test.jpg", 200, "IMAGE", Some("jpg"));
        let archive = create_test_file("test.zip", 5000, "FILE", Some("zip"));

        assert!(FileTypeFilter::Folder.matches(&folder));
        assert!(!FileTypeFilter::Folder.matches(&video));

        assert!(FileTypeFilter::File.matches(&video));
        assert!(!FileTypeFilter::File.matches(&folder));

        assert!(FileTypeFilter::Video.matches(&video));
        assert!(!FileTypeFilter::Video.matches(&audio));

        assert!(FileTypeFilter::Audio.matches(&audio));
        assert!(!FileTypeFilter::Audio.matches(&image));

        assert!(FileTypeFilter::Image.matches(&image));
        assert!(!FileTypeFilter::Image.matches(&archive));

        assert!(FileTypeFilter::Archive.matches(&archive));
        assert!(!FileTypeFilter::Archive.matches(&video));
    }

    #[test]
    fn test_file_filter_name_pattern() {
        let filter = FileFilter::new().with_name_pattern("*.mp4".to_string());

        let video = create_test_file("movie.mp4", 1000, "VIDEO", Some("mp4"));
        let other = create_test_file("document.pdf", 100, "FILE", Some("pdf"));

        assert!(filter.matches(&video));
        assert!(!filter.matches(&other));
    }

    #[test]
    fn test_file_filter_size() {
        let filter = FileFilter::new().with_min_size(100).with_max_size(1000);

        let small = create_test_file("small.txt", 50, "FILE", Some("txt"));
        let medium = create_test_file("medium.txt", 500, "FILE", Some("txt"));
        let large = create_test_file("large.txt", 2000, "FILE", Some("txt"));

        assert!(!filter.matches(&small));
        assert!(filter.matches(&medium));
        assert!(!filter.matches(&large));
    }

    #[test]
    fn test_file_filter_combined() {
        let filter = FileFilter::new()
            .with_name_pattern("*.mp4".to_string())
            .with_file_type(FileTypeFilter::Video)
            .with_min_size(1000);

        let match_all = create_test_file("movie.mp4", 5000, "VIDEO", Some("mp4"));
        let wrong_type = create_test_file("movie.mp4", 5000, "AUDIO", Some("mp4"));
        let too_small = create_test_file("movie.mp4", 500, "VIDEO", Some("mp4"));
        let wrong_name = create_test_file("movie.mkv", 5000, "VIDEO", Some("mkv"));

        assert!(filter.matches(&match_all));
        assert!(!filter.matches(&wrong_type));
        assert!(!filter.matches(&too_small));
        assert!(!filter.matches(&wrong_name));
    }

    #[test]
    fn test_parse_size() {
        assert_eq!(parse_size("100").unwrap(), 100);
        assert_eq!(parse_size("100B").unwrap(), 100);
        assert_eq!(parse_size("1KB").unwrap(), 1024);
        assert_eq!(parse_size("1MB").unwrap(), 1024 * 1024);
        assert_eq!(parse_size("1GB").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("1TB").unwrap(), 1024 * 1024 * 1024 * 1024);
        assert_eq!(
            parse_size("1.5GB").unwrap(),
            (1.5 * 1024.0 * 1024.0 * 1024.0) as i64
        );

        // Case insensitive
        assert_eq!(parse_size("1gb").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("1Gb").unwrap(), 1024 * 1024 * 1024);

        // Short forms
        assert_eq!(parse_size("1K").unwrap(), 1024);
        assert_eq!(parse_size("1M").unwrap(), 1024 * 1024);
        assert_eq!(parse_size("1G").unwrap(), 1024 * 1024 * 1024);

        // Invalid
        assert!(parse_size("invalid").is_err());
        assert!(parse_size("100ZB").is_err());
    }

    #[test]
    fn test_sort_files_by_name() {
        let files = vec![
            create_test_file("zebra.txt", 100, "FILE", Some("txt")),
            create_test_file("apple.txt", 200, "FILE", Some("txt")),
            create_test_file("banana.txt", 150, "FILE", Some("txt")),
        ];

        let sorted = sort_files(files, SortField::Name, false);
        assert_eq!(sorted[0].name, "apple.txt");
        assert_eq!(sorted[1].name, "banana.txt");
        assert_eq!(sorted[2].name, "zebra.txt");
    }

    #[test]
    fn test_sort_files_by_size() {
        let files = vec![
            create_test_file("large.txt", 1000, "FILE", Some("txt")),
            create_test_file("small.txt", 100, "FILE", Some("txt")),
            create_test_file("medium.txt", 500, "FILE", Some("txt")),
        ];

        let sorted = sort_files(files, SortField::Size, false);
        assert_eq!(sorted[0].name, "small.txt");
        assert_eq!(sorted[1].name, "medium.txt");
        assert_eq!(sorted[2].name, "large.txt");
    }

    #[test]
    fn test_sort_files_reverse() {
        let files = vec![
            create_test_file("a.txt", 100, "FILE", Some("txt")),
            create_test_file("b.txt", 200, "FILE", Some("txt")),
            create_test_file("c.txt", 300, "FILE", Some("txt")),
        ];

        let sorted = sort_files(files, SortField::Name, true);
        assert_eq!(sorted[0].name, "c.txt");
        assert_eq!(sorted[1].name, "b.txt");
        assert_eq!(sorted[2].name, "a.txt");
    }
}
