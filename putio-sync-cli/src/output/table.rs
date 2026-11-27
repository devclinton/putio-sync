//! Table output formatter

use super::{format_bytes, format_timestamp, pad, truncate};
use putio_sync_client::FileEntry;
use putio_sync_core::t;

/// Format files as a table
pub fn format_table(files: &[FileEntry]) -> String {
    if files.is_empty() {
        return format!("{}\n", t!("No files found."));
    }

    let mut output = String::new();

    // Header
    output.push_str(&row(&[
        &t!("ID"),
        &t!("Name"),
        &t!("Size"),
        &t!("Modified"),
        &t!("Type"),
    ]));
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

        output.push_str(&row(&[
            &file.id.to_string(),
            &truncate(&name, 40),
            &size_str,
            &format_timestamp(&file.created_at),
            &file_type,
        ]));
    }

    output
}

/// Column widths, in terminal columns
const COLUMNS: [usize; 5] = [12, 40, 12, 20, 10];

/// One line of the table, each cell padded to its column by display width
fn row(cells: &[&str; 5]) -> String {
    let padded: Vec<String> = cells
        .iter()
        .zip(COLUMNS)
        .map(|(cell, columns)| pad(cell, columns))
        .collect();
    format!("{}\n", padded.join(" ").trim_end())
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

    #[test]
    fn wide_and_multi_byte_names_do_not_panic_and_line_up() {
        use crate::output::width;
        // 45 CJK characters: a multi-byte character sits across byte 37, where the old
        // byte slice panicked
        let cjk = "映".repeat(45);
        let files = vec![
            create_test_file(1, &cjk, 10, "VIDEO"),
            create_test_file(2, &format!("{}é.mkv", "a".repeat(36)), 10, "VIDEO"),
            create_test_file(3, "short.mkv", 10, "VIDEO"),
        ];
        let output = format_table(&files);
        let size_column: Vec<usize> = output
            .lines()
            .skip(2)
            .map(|line| width(&line[..line.find("10 B").unwrap()]))
            .collect();
        assert_eq!(size_column, [54, 54, 54], "{output}");
        assert!(output.contains("..."));
    }
}
