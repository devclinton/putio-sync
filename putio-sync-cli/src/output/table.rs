//! Table output formatter

use super::{format_bytes, format_timestamp};
use putio_sync_client::FileEntry;

/// Format files as a table
pub fn format_table(files: &[FileEntry]) -> String {
    if files.is_empty() {
        return "No files found.\n".to_string();
    }

    let mut output = String::new();

    // Header
    output.push_str(&format!(
        "{:<12} {:<40} {:<12} {:<20} {:<10}\n",
        "ID", "Name", "Size", "Modified", "Type"
    ));
    output.push_str(&"─".repeat(100));
    output.push('\n');

    // Rows
    for file in files {
        let size_str = if file.file_type == "FOLDER" {
            "-".to_string()
        } else {
            format_bytes(file.size)
        };

        let name = if file.file_type == "FOLDER" {
            format!("{}/", file.name)
        } else {
            file.name.clone()
        };

        let file_type = file.file_type.to_lowercase();

        output.push_str(&format!(
            "{:<12} {:<40} {:<12} {:<20} {:<10}\n",
            file.id,
            truncate(&name, 40),
            size_str,
            format_timestamp(&file.created_at),
            file_type
        ));
    }

    output
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len - 3])
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_file(id: i64, name: &str, size: i64, file_type: &str) -> FileEntry {
        FileEntry {
            id,
            name: name.to_string(),
            size,
            file_type: file_type.to_string(),
            parent_id: Some(0),
            content_type: None,
            created_at: Some("2025-11-06T10:00:00Z".to_string()),
            first_accessed_at: None,
            is_folder: file_type == "FOLDER",
            is_shared: false,
            screenshot: None,
            opensubtitles_hash: None,
            crc32: None,
            extension: None,
            sort_by: None,
        }
    }

    #[test]
    fn test_format_table() {
        let files = vec![
            create_test_file(1, "Movies", 0, "FOLDER"),
            create_test_file(2, "video.mp4", 1073741824, "VIDEO"),
        ];

        let output = format_table(&files);
        assert!(output.contains("ID"));
        assert!(output.contains("Name"));
        assert!(output.contains("Movies/"));
        assert!(output.contains("video.mp4"));
        assert!(output.contains("1.00 GB"));
    }

    #[test]
    fn test_format_table_empty() {
        let files = vec![];
        let output = format_table(&files);
        assert!(output.contains("No files found"));
    }
}
