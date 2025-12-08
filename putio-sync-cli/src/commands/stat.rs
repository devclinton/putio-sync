//! Stat command implementation

use anyhow::{Context, Result};
use putio_sync_core::config::Config;
use putio_sync_db::{Database, StatsRepository};
use serde::Serialize;
use std::path::PathBuf;
use tracing::{debug, info};

/// Statistics data for display
#[derive(Debug, Serialize)]
struct Stats {
    total_downloads: i64,
    total_bytes: i64,
    total_seconds: i64,
    average_speed: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    by_extension: Option<Vec<ExtensionStatsDisplay>>,
}

/// Extension statistics for display
#[derive(Debug, Serialize)]
struct ExtensionStatsDisplay {
    extension: String,
    file_count: i64,
    total_bytes: i64,
    percentage: f64,
}

/// Execute stat command
///
/// Displays download statistics from the database
pub async fn execute_stat(
    by_extension: bool,
    since: Option<String>,
    until: Option<String>,
    output_format: String,
    config_path: Option<PathBuf>,
) -> Result<()> {
    info!("Starting stat command");

    // Load configuration
    let config = load_config(config_path)?;

    // Validate date formats
    let since_date = validate_and_format_date(since.as_deref())?;
    let until_date = validate_and_format_date(until.as_deref())?;

    // Initialize database (stored in target directory)
    let db_path = config.download.target_directory.join(".putiosync.db");

    debug!("Opening database at: {:?}", db_path);
    let db = Database::new(&db_path)
        .await
        .context("Failed to open database")?;

    let stats_repo = StatsRepository::new(db.pool());

    // Get statistics
    let (total_downloads, total_bytes, total_seconds) = stats_repo
        .get_for_range(since_date.as_deref(), until_date.as_deref())
        .await
        .context("Failed to get statistics")?;

    // Calculate average speed (bytes per second)
    let average_speed = if total_seconds > 0 {
        total_bytes as f64 / total_seconds as f64
    } else {
        0.0
    };

    // Get extension breakdown if requested
    let extension_stats = if by_extension {
        let ext_stats = stats_repo
            .get_by_extension(since_date.as_deref(), until_date.as_deref())
            .await
            .context("Failed to get extension statistics")?;

        // Calculate percentages
        let total_ext_bytes: i64 = ext_stats.iter().map(|s| s.total_bytes).sum();
        let display_stats: Vec<ExtensionStatsDisplay> = ext_stats
            .into_iter()
            .map(|s| {
                let percentage = if total_ext_bytes > 0 {
                    (s.total_bytes as f64 / total_ext_bytes as f64) * 100.0
                } else {
                    0.0
                };
                ExtensionStatsDisplay {
                    extension: s.extension,
                    file_count: s.file_count,
                    total_bytes: s.total_bytes,
                    percentage,
                }
            })
            .collect();

        Some(display_stats)
    } else {
        None
    };

    let stats = Stats {
        total_downloads,
        total_bytes,
        total_seconds,
        average_speed,
        by_extension: extension_stats,
    };

    // Display based on output format
    match output_format.as_str() {
        "json" => display_json(&stats)?,
        "table" | _ => display_table(&stats, since.as_deref(), until.as_deref())?,
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

/// Validate and format date to ISO 8601
fn validate_and_format_date(date_str: Option<&str>) -> Result<Option<String>> {
    if let Some(date) = date_str {
        // Try to parse various date formats and convert to ISO 8601
        use chrono::NaiveDate;

        let formats = vec![
            "%Y-%m-%d",     // 2025-01-15
            "%Y/%m/%d",     // 2025/01/15
            "%m/%d/%Y",     // 01/15/2025
            "%d-%b-%Y",     // 15-Jan-2025
        ];

        for format in &formats {
            if let Ok(parsed_date) = NaiveDate::parse_from_str(date, format) {
                return Ok(Some(parsed_date.format("%Y-%m-%d").to_string()));
            }
        }

        anyhow::bail!("Invalid date format: '{}'. Expected format: YYYY-MM-DD", date);
    }

    Ok(None)
}

/// Display statistics in table format
fn display_table(stats: &Stats, since: Option<&str>, until: Option<&str>) -> Result<()> {
    println!("\nDownload Statistics");
    println!("{}", "═".repeat(65));

    // Show date range if specified
    if since.is_some() || until.is_some() {
        print!("Date Range:           ");
        if let Some(s) = since {
            print!("Since {}", s);
        }
        if let Some(u) = until {
            if since.is_some() {
                print!(" ");
            }
            print!("Until {}", u);
        }
        println!();
        println!("{}", "─".repeat(65));
    }

    println!("Total Downloads:      {:>12}", format_count(stats.total_downloads));
    println!("Total Size:           {:>12}", format_bytes(stats.total_bytes));
    println!("Total Time:           {:>12}", format_duration(stats.total_seconds));
    println!("Average Speed:        {:>12}", format_speed(stats.average_speed));

    // Show extension breakdown if available
    if let Some(ref ext_stats) = stats.by_extension {
        if !ext_stats.is_empty() {
            println!("\nBy Extension:");
            println!("{}", "─".repeat(65));
            println!("{:<12} {:>10} {:>15} {:>12}", "Extension", "Files", "Total Size", "Percentage");
            println!("{}", "─".repeat(65));

            for ext in ext_stats {
                println!(
                    "{:<12} {:>10} {:>15} {:>11.1}%",
                    format!(".{}", ext.extension),
                    format_count(ext.file_count),
                    format_bytes(ext.total_bytes),
                    ext.percentage
                );
            }

            println!("{}", "─".repeat(65));
        }
    }

    println!("{}", "═".repeat(65));
    println!();

    Ok(())
}

/// Display statistics in JSON format
fn display_json(stats: &Stats) -> Result<()> {
    let json = serde_json::to_string_pretty(stats)
        .context("Failed to serialize statistics to JSON")?;
    println!("{}", json);
    Ok(())
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
    fn test_validate_date_format() {
        assert!(validate_and_format_date(None).unwrap().is_none());
        assert_eq!(
            validate_and_format_date(Some("2025-01-15")).unwrap(),
            Some("2025-01-15".to_string())
        );
        assert_eq!(
            validate_and_format_date(Some("2025/01/15")).unwrap(),
            Some("2025-01-15".to_string())
        );
        assert!(validate_and_format_date(Some("invalid")).is_err());
    }
}
