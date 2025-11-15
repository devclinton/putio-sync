//! CSV output formatter

use putio_sync_core::api::FileEntry;

/// Format files as CSV
pub fn format_csv(files: &[FileEntry]) -> String {
    let mut output = String::new();

    // Header
    output.push_str("id,name,size,type,parent_id,created_at\n");

    // Rows
    for file in files {
        output.push_str(&format!(
            "{},{},{},{},{},{}\n",
            file.id,
            escape_csv(&file.name),
            file.size,
            file.file_type,
            file.parent_id.unwrap_or(0),
            file.created_at.as_deref().unwrap_or("")
        ));
    }

    output
}

fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
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
    fn test_format_csv() {
        let files = vec![create_test_file(1, "test.txt")];
        let output = format_csv(&files);
        assert!(output.contains("id,name,size"));
        assert!(output.contains("test.txt"));
        assert!(output.contains("1,test.txt,1024"));
    }

    #[test]
    fn test_escape_csv() {
        assert_eq!(escape_csv("simple"), "simple");
        assert_eq!(escape_csv("with,comma"), "\"with,comma\"");
        assert_eq!(escape_csv("with\"quote"), "\"with\"\"quote\"");
    }
}
