//! Delete command implementation

use anyhow::{Context, Result};
use chrono::NaiveDate;
use glob::Pattern;
use putio_sync_core::api::{ApiClient, FileEntry};
use putio_sync_core::config::Config;
use std::io::{self, Write};
use std::path::PathBuf;
use tracing::{debug, info};

/// Execute delete command
///
/// Deletes files from Put.io with various filtering options
#[allow(clippy::too_many_arguments)]
pub async fn execute_delete(
    files: Vec<i64>,
    all: bool,
    filter: Option<String>,
    recursive: bool,
    before: Option<String>,
    dry_run: bool,
    yes: bool,
    config_path: Option<PathBuf>,
) -> Result<()> {
    info!("Starting delete command");

    // Load configuration
    let config = load_config(config_path)?;

    // Create API client
    let auth_config = config
        .auth
        .clone()
        .context("No authentication configured. Run 'putio-sync login' first.")?;
    let client = ApiClient::from_auth_config(auth_config);

    // Validate arguments
    if !all && files.is_empty() {
        anyhow::bail!("Either specify file IDs or use --all flag");
    }

    // Parse date filter if provided
    let before_date = if let Some(date_str) = &before {
        Some(parse_date(date_str)?)
    } else {
        None
    };

    // Get list of files to delete
    let files_to_delete = if all {
        // Get all files from root
        debug!("Fetching all files from root");
        let response = client.list_files(0).await?;
        filter_files(response.files, filter.as_deref(), before_date.as_ref())
    } else {
        // Get specific files by ID
        debug!("Fetching specific files: {:?}", files);
        let mut fetched_files = Vec::new();
        for file_id in &files {
            let response = client.get_file(*file_id).await?;
            fetched_files.push(response.file);
        }
        filter_files(fetched_files, filter.as_deref(), before_date.as_ref())
    };

    if files_to_delete.is_empty() {
        println!("No files to delete");
        return Ok(());
    }

    // Calculate total size
    let total_size: i64 = files_to_delete.iter().map(|f| f.size).sum();

    // Display files to be deleted
    println!("\nFiles to be deleted:");
    println!("{:<12} {:<50} {:<15} {}", "ID", "Name", "Size", "Type");
    println!("{}", "-".repeat(90));
    for file in &files_to_delete {
        let file_type = if file.is_folder { "Folder" } else { "File" };
        println!(
            "{:<12} {:<50} {:<15} {}",
            file.id,
            truncate_string(&file.name, 50),
            format_bytes(file.size),
            file_type
        );
    }
    println!("{}", "-".repeat(90));
    println!(
        "Total: {} file(s), {} to be freed",
        files_to_delete.len(),
        format_bytes(total_size)
    );

    // Dry run - just show what would be deleted
    if dry_run {
        println!("\nDry run mode - no files were deleted");
        return Ok(());
    }

    // Confirmation prompt (unless --yes flag is set)
    if !yes && !confirm_deletion(&files_to_delete)? {
        println!("Deletion cancelled");
        return Ok(());
    }

    // Delete files
    println!("\nDeleting files...");
    let mut deleted_count = 0;
    let mut deleted_size = 0i64;
    let mut errors = Vec::new();

    for file in &files_to_delete {
        print!("  Deleting {} ... ", file.name);
        io::stdout().flush()?;

        let result = if file.is_folder && recursive {
            client.delete_folder_recursive(file.id).await
        } else {
            client.delete_files(vec![file.id]).await
        };

        match result {
            Ok(_) => {
                println!("✓");
                deleted_count += 1;
                deleted_size += file.size;
            }
            Err(e) => {
                println!("✗");
                errors.push(format!("Failed to delete {}: {}", file.name, e));
            }
        }
    }

    // Display summary
    println!("\n{}", "=".repeat(90));
    println!("Deletion Summary:");
    println!("  Files deleted: {}/{}", deleted_count, files_to_delete.len());
    println!("  Space freed:   {}", format_bytes(deleted_size));

    if !errors.is_empty() {
        println!("\nErrors encountered:");
        for error in &errors {
            println!("  • {}", error);
        }
        anyhow::bail!("{} file(s) failed to delete", errors.len());
    }

    println!("{}", "=".repeat(90));
    Ok(())
}

/// Load configuration from file or use default
fn load_config(config_path: Option<PathBuf>) -> Result<Config> {
    let path = config_path.unwrap_or_else(|| {
        Config::default_path().expect("Failed to determine default config path")
    });

    Config::load(&path).context("Failed to load configuration. Run 'putio-sync login' first.")
}

/// Filter files based on pattern and date
fn filter_files(
    files: Vec<FileEntry>,
    pattern: Option<&str>,
    before_date: Option<&NaiveDate>,
) -> Vec<FileEntry> {
    files
        .into_iter()
        .filter(|file| {
            // Apply glob pattern filter
            if let Some(p) = pattern {
                if let Ok(glob_pattern) = Pattern::new(p) {
                    if !glob_pattern.matches(&file.name) {
                        return false;
                    }
                }
            }

            // Apply date filter
            if let Some(date) = before_date {
                if let Some(created_at) = &file.created_at {
                    if let Ok(file_date) = NaiveDate::parse_from_str(
                        &created_at[..10], // Extract date part (YYYY-MM-DD)
                        "%Y-%m-%d",
                    ) {
                        if file_date >= *date {
                            return false;
                        }
                    }
                }
            }

            true
        })
        .collect()
}

/// Parse date string in various formats
fn parse_date(date_str: &str) -> Result<NaiveDate> {
    // Try different date formats
    let formats = vec![
        "%Y-%m-%d",     // 2026-01-15
        "%Y-%m",        // 2026-01 (interpreted as first day of month)
        "%Y/%m/%d",     // 2026/01/15
        "%Y/%m",        // 2026/01
        "%Y%m%d",       // 20260115
        "%m/%d/%Y",     // 01/15/2026
        "%d-%b-%Y",     // 15-Jan-2026
        "%Y-%b-%d",     // 2026-Jan-15
    ];

    for format in &formats {
        if let Ok(date) = NaiveDate::parse_from_str(date_str, format) {
            return Ok(date);
        }
    }

    // Try parsing year-month format and use first day of month
    if let Some((year_str, month_str)) = date_str.split_once('-') {
        if let (Ok(year), Ok(month)) = (year_str.parse::<i32>(), month_str.parse::<u32>()) {
            if let Some(date) = NaiveDate::from_ymd_opt(year, month, 1) {
                return Ok(date);
            }
        }
    }

    anyhow::bail!(
        "Invalid date format: '{}'. Expected formats: YYYY-MM-DD, YYYY-MM, YYYY/MM/DD, etc.",
        date_str
    )
}

/// Confirm deletion with user
fn confirm_deletion(files: &[FileEntry]) -> Result<bool> {
    print!("\nAre you sure you want to delete {} file(s)? [y/N] ", files.len());
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    Ok(input.trim().to_lowercase() == "y" || input.trim().to_lowercase() == "yes")
}

/// Format bytes as human-readable string
fn format_bytes(bytes: i64) -> String {
    const KB: i64 = 1024;
    const MB: i64 = KB * 1024;
    const GB: i64 = MB * 1024;
    const TB: i64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Truncate string to max length with ellipsis
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048576), "1.00 MB");
        assert_eq!(format_bytes(1073741824), "1.00 GB");
        assert_eq!(format_bytes(1099511627776), "1.00 TB");
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("short", 10), "short");
        assert_eq!(truncate_string("exact length!", 13), "exact length!");
        assert_eq!(truncate_string("this is a very long string", 15), "this is a ve...");
    }

    #[test]
    fn test_parse_date() {
        assert!(parse_date("2026-01-15").is_ok());
        assert!(parse_date("2026-01").is_ok());
        assert!(parse_date("2026/01/15").is_ok());
        assert!(parse_date("01/15/2026").is_ok());
        assert!(parse_date("invalid").is_err());
    }

    #[test]
    fn test_filter_files_with_pattern() {
        let files = vec![
            FileEntry {
                id: 1,
                name: "file1.txt".to_string(),
                size: 1024,
                file_type: "FILE".to_string(),
                parent_id: Some(0),
                content_type: Some("text/plain".to_string()),
                created_at: Some("2025-01-01T00:00:00Z".to_string()),
                first_accessed_at: None,
                is_folder: false,
                is_shared: false,
                screenshot: None,
                opensubtitles_hash: None,
                crc32: None,
                extension: Some("txt".to_string()),
                sort_by: None,
            },
            FileEntry {
                id: 2,
                name: "file2.mkv".to_string(),
                size: 2048,
                file_type: "VIDEO".to_string(),
                parent_id: Some(0),
                content_type: Some("video/x-matroska".to_string()),
                created_at: Some("2025-01-02T00:00:00Z".to_string()),
                first_accessed_at: None,
                is_folder: false,
                is_shared: false,
                screenshot: None,
                opensubtitles_hash: None,
                crc32: None,
                extension: Some("mkv".to_string()),
                sort_by: None,
            },
        ];

        let filtered = filter_files(files.clone(), Some("*.txt"), None);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "file1.txt");

        let filtered = filter_files(files, Some("*.mkv"), None);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "file2.mkv");
    }

    #[test]
    fn test_filter_files_with_date() {
        let files = vec![
            FileEntry {
                id: 1,
                name: "old_file.txt".to_string(),
                size: 1024,
                file_type: "FILE".to_string(),
                parent_id: Some(0),
                content_type: Some("text/plain".to_string()),
                created_at: Some("2024-12-01T00:00:00Z".to_string()),
                first_accessed_at: None,
                is_folder: false,
                is_shared: false,
                screenshot: None,
                opensubtitles_hash: None,
                crc32: None,
                extension: Some("txt".to_string()),
                sort_by: None,
            },
            FileEntry {
                id: 2,
                name: "new_file.txt".to_string(),
                size: 2048,
                file_type: "FILE".to_string(),
                parent_id: Some(0),
                content_type: Some("text/plain".to_string()),
                created_at: Some("2025-02-01T00:00:00Z".to_string()),
                first_accessed_at: None,
                is_folder: false,
                is_shared: false,
                screenshot: None,
                opensubtitles_hash: None,
                crc32: None,
                extension: Some("txt".to_string()),
                sort_by: None,
            },
        ];

        let before = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let filtered = filter_files(files, None, Some(&before));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "old_file.txt");
    }
}
