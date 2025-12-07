//! Tree output formatter

use super::format_bytes;
use putio_sync_client::FileEntry;

/// Format files as a tree
pub fn format_tree(files: &[FileEntry]) -> String {
    let mut output = String::new();
    output.push_str(".\n");

    let file_count = files.len();
    for (index, file) in files.iter().enumerate() {
        let is_last = index == file_count - 1;
        let prefix = if is_last { "└── " } else { "├── " };

        let name = if file.file_type == "FOLDER" {
            format!("{}/", file.name)
        } else {
            format!("{} ({})", file.name, format_bytes(file.size))
        };

        output.push_str(&format!("{}{}\n", prefix, name));
    }

    output
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
    fn test_format_tree() {
        let files = vec![
            create_test_file(1, "Movies", 0, "FOLDER"),
            create_test_file(2, "video.mp4", 1073741824, "VIDEO"),
        ];

        let output = format_tree(&files);
        assert!(output.contains("Movies/"));
        assert!(output.contains("video.mp4"));
        assert!(output.contains("1.00 GB"));
        assert!(output.contains("├──"));
        assert!(output.contains("└──"));
    }
}
