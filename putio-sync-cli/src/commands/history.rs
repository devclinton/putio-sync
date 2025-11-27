//! History command implementation

use anyhow::{Context, Result};
use putio_sync_core::config::Config;
use putio_sync_core::db::{Database, HistoryRepository};
use putio_sync_core::db::models::HistoryAction;
use serde::Serialize;
use std::path::PathBuf;
use tracing::{debug, info};

/// History entry for display
#[derive(Debug, Serialize)]
struct HistoryDisplay {
    timestamp: String,
    action: String,
    filename: String,
    size: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    speed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration: Option<String>,
}

/// Execute history command
///
/// Displays download and delete history from the database
#[allow(clippy::too_many_arguments)]
pub async fn execute_history(
    action: Option<String>,
    since: Option<String>,
    until: Option<String>,
    limit: usize,
    offset: usize,
    output_format: String,
    config_path: Option<PathBuf>,
) -> Result<()> {
    info!("Starting history command");

    // Load configuration
    let config = load_config(config_path)?;

    // Parse action filter
    let action_filter = if let Some(ref action_str) = action {
        Some(parse_action(action_str)?)
    } else {
        None
    };

    // Validate and format dates
    let since_date = validate_and_format_date(since.as_deref())?;
    let until_date = validate_and_format_date(until.as_deref())?;

    // Initialize database
    let db_path = config.download.target_directory.join(".putiosync.db");

    debug!("Opening database at: {:?}", db_path);
    let db = Database::new(&db_path)
        .await
        .context("Failed to open database. Have you downloaded any files yet?")?;

    let history_repo = HistoryRepository::new(db.pool());

    // Get total count for pagination info
    let total_count = history_repo
        .count(action_filter.clone())
        .await
        .context("Failed to count history entries")?;

    // Query history
    let history_entries = if since_date.is_some() || until_date.is_some() {
        history_repo
            .query_by_date(
                since_date.as_deref(),
                until_date.as_deref(),
                action_filter,
                limit as i64,
                offset as i64,
            )
            .await
            .context("Failed to query history")?
    } else {
        history_repo
            .query(action_filter, limit as i64, offset as i64)
            .await
            .context("Failed to query history")?
    };

    // Convert to display format
    let display_entries: Vec<HistoryDisplay> = history_entries
        .into_iter()
        .map(|entry| HistoryDisplay {
            timestamp: format_timestamp(&entry.timestamp),
            action: entry.action.clone(),
            filename: entry.filename.clone(),
            size: format_bytes(entry.file_size),
            speed: entry.average_speed.map(|s| format_speed(s as f64)),
            duration: entry.duration_seconds.map(format_duration),
        })
        .collect();

    // Display based on output format
    match output_format.as_str() {
        "json" => display_json(&display_entries)?,
        "csv" => display_csv(&display_entries)?,
        "table" | _ => display_table(&display_entries, total_count, limit, offset, action.as_deref(), since.as_deref(), until.as_deref())?,
    }

    Ok(())
}

/// Load configuration from file or use default
fn load_config(config_path: Option<PathBuf>) -> Result<Config> {
    let path = config_path.unwrap_or_else(|| {
        Config::default_path().expect("Failed to determine default config path")
    });

    Config::load(&path)
        .or_else(|_| Ok::<Config, anyhow::Error>(Config::default()))
        .context("Failed to load configuration")
}

/// Parse action string to HistoryAction
fn parse_action(action_str: &str) -> Result<HistoryAction> {
    action_str
        .parse::<HistoryAction>()
        .map_err(|e| anyhow::anyhow!("Invalid action '{}': {}. Expected 'downloaded' or 'deleted'", action_str, e))
}

/// Validate and format date to ISO 8601
fn validate_and_format_date(date_str: Option<&str>) -> Result<Option<String>> {
    if let Some(date) = date_str {
        use chrono::NaiveDate;

        let formats = vec![
            "%Y-%m-%d",     // 2025-01-15
            "%Y/%m/%d",     // 2025/01/15
            "%m/%d/%Y",     // 01/15/2025
            "%d-%b-%Y",     // 15-Jan-2025
        ];

        for format in &formats {
            if let Ok(parsed_date) = NaiveDate::parse_from_str(date, format) {
                // Return in ISO 8601 format with time
                return Ok(Some(format!("{} 00:00:00", parsed_date.format("%Y-%m-%d"))));
            }
        }

        anyhow::bail!("Invalid date format: '{}'. Expected format: YYYY-MM-DD", date);
    }

    Ok(None)
}

/// Format timestamp for display
fn format_timestamp(timestamp: &str) -> String {
    // SQLite timestamp format: "2025-11-14 10:30:45"
    // Just return as-is for now, could parse and reformat if needed
    timestamp.to_string()
}

/// Display history in table format
fn display_table(
    entries: &[HistoryDisplay],
    total_count: i64,
    _limit: usize,
    offset: usize,
    action_filter: Option<&str>,
    since: Option<&str>,
    until: Option<&str>,
) -> Result<()> {
    println!("\nDownload & Delete History");
    println!("{}", "═".repeat(95));

    // Show filters if applied
    if action_filter.is_some() || since.is_some() || until.is_some() {
        print!("Filters:              ");
        let mut filters = Vec::new();
        if let Some(a) = action_filter {
            filters.push(format!("Action: {}", a));
        }
        if let Some(s) = since {
            filters.push(format!("Since: {}", s));
        }
        if let Some(u) = until {
            filters.push(format!("Until: {}", u));
        }
        println!("{}", filters.join(", "));
        println!("{}", "─".repeat(95));
    }

    if entries.is_empty() {
        println!("\nNo history entries found.");
        if action_filter.is_none() && since.is_none() && until.is_none() {
            println!("Try downloading some files first!");
        }
    } else {
        println!();
        println!(
            "{:<20} {:<12} {:<40} {:<12} Speed",
            "Timestamp", "Action", "Filename", "Size"
        );
        println!("{}", "─".repeat(95));

        for entry in entries {
            let speed_display = entry.speed.as_deref().unwrap_or("-");
            println!(
                "{:<20} {:<12} {:<40} {:<12} {}",
                entry.timestamp,
                entry.action,
                truncate_string(&entry.filename, 40),
                entry.size,
                speed_display
            );
        }

        println!("{}", "─".repeat(95));
    }

    // Pagination info
    let showing_from = if entries.is_empty() { 0 } else { offset + 1 };
    let showing_to = offset + entries.len();
    println!(
        "\nShowing {} - {} of {} entries{}",
        showing_from,
        showing_to,
        format_count(total_count),
        if showing_to < total_count as usize {
            ". Use --offset to see more."
        } else {
            ""
        }
    );

    println!("{}", "═".repeat(95));
    println!();

    Ok(())
}

/// Display history in JSON format
fn display_json(entries: &[HistoryDisplay]) -> Result<()> {
    let json = serde_json::to_string_pretty(entries)
        .context("Failed to serialize history to JSON")?;
    println!("{}", json);
    Ok(())
}

/// Display history in CSV format
fn display_csv(entries: &[HistoryDisplay]) -> Result<()> {
    // Print CSV header
    println!("Timestamp,Action,Filename,Size,Speed,Duration");

    // Print each entry
    for entry in entries {
        println!(
            "{},{},{},{},{},{}",
            escape_csv(&entry.timestamp),
            escape_csv(&entry.action),
            escape_csv(&entry.filename),
            escape_csv(&entry.size),
            entry.speed.as_deref().unwrap_or(""),
            entry.duration.as_deref().unwrap_or("")
        );
    }

    Ok(())
}

/// Escape CSV field
fn escape_csv(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// Format count with thousands separator
fn format_count(count: i64) -> String {
    let s = count.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();

    for (i, c) in chars.iter().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.insert(0, ',');
        }
        result.insert(0, *c);
    }

    result
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

/// Format duration as human-readable string
fn format_duration(seconds: i64) -> String {
    if seconds == 0 {
        return "0s".to_string();
    }

    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    let mut parts = Vec::new();

    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if secs > 0 || parts.is_empty() {
        parts.push(format!("{}s", secs));
    }

    parts.join(" ")
}

/// Format speed as human-readable string
fn format_speed(bytes_per_second: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    if bytes_per_second >= GB {
        format!("{:.2} GB/s", bytes_per_second / GB)
    } else if bytes_per_second >= MB {
        format!("{:.2} MB/s", bytes_per_second / MB)
    } else if bytes_per_second >= KB {
        format!("{:.2} KB/s", bytes_per_second / KB)
    } else if bytes_per_second > 0.0 {
        format!("{:.0} B/s", bytes_per_second)
    } else {
        "N/A".to_string()
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
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(3661), "1h 1m 1s");
        assert_eq!(format_duration(7265), "2h 1m 5s");
    }

    #[test]
    fn test_format_speed() {
        assert_eq!(format_speed(0.0), "N/A");
        assert_eq!(format_speed(512.0), "512 B/s");
        assert_eq!(format_speed(1024.0), "1.00 KB/s");
        assert_eq!(format_speed(1048576.0), "1.00 MB/s");
        assert_eq!(format_speed(1073741824.0), "1.00 GB/s");
    }

    #[test]
    fn test_format_count() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1000), "1,000");
        assert_eq!(format_count(1234567), "1,234,567");
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("short", 10), "short");
        assert_eq!(truncate_string("exact length!", 13), "exact length!");
        assert_eq!(truncate_string("this is a very long string", 15), "this is a ve...");
    }

    #[test]
    fn test_escape_csv() {
        assert_eq!(escape_csv("simple"), "simple");
        assert_eq!(escape_csv("with,comma"), "\"with,comma\"");
        assert_eq!(escape_csv("with\"quote"), "\"with\"\"quote\"");
        assert_eq!(escape_csv("with\nline"), "\"with\nline\"");
    }

    #[test]
    fn test_parse_action() {
        assert!(parse_action("downloaded").is_ok());
        assert!(parse_action("deleted").is_ok());
        assert!(parse_action("invalid").is_err());
    }

    #[test]
    fn test_validate_date_format() {
        assert!(validate_and_format_date(None).unwrap().is_none());
        assert_eq!(
            validate_and_format_date(Some("2025-01-15")).unwrap(),
            Some("2025-01-15 00:00:00".to_string())
        );
        assert_eq!(
            validate_and_format_date(Some("2025/01/15")).unwrap(),
            Some("2025-01-15 00:00:00".to_string())
        );
        assert!(validate_and_format_date(Some("invalid")).is_err());
    }
}
