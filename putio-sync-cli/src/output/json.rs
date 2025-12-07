//! JSON output formatter

use putio_sync_client::FileEntry;
use serde_json;

/// Format files as JSON
pub fn format_json(files: &[FileEntry]) -> String {
    let json_output = serde_json::json!({
        "files": files,
        "count": files.len()
    });

    serde_json::to_string_pretty(&json_output).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_file(id: i64, name: &str) -> FileEntry {
        FileEntry {
            id,
            name: name.to_string(),
            size: 1024,
            file_type: "FILE".to_string(),
            parent_id: Some(0),
            content_type: None,
            created_at: Some("2025-11-06T10:00:00Z".to_string()),
            first_accessed_at: None,
            is_folder: false,
            is_shared: false,
            screenshot: None,
            opensubtitles_hash: None,
            crc32: None,
            extension: None,
            sort_by: None,
        }
    }

    #[test]
    fn test_format_json() {
        let files = vec![create_test_file(1, "test.txt")];
        let output = format_json(&files);
        assert!(output.contains("\"files\""));
        assert!(output.contains("\"count\""));
        assert!(output.contains("test.txt"));
    }
}
