//! Transfers command implementation

use anyhow::{Context, Result};
use putio_sync_core::api::ApiClient;
use putio_sync_core::config::Config;
use serde::Serialize;
use std::path::PathBuf;
use tracing::info;

/// Transfer information for display
#[derive(Debug, Serialize)]
struct TransferDisplay {
    id: i64,
    name: String,
    status: String,
    size: Option<i64>,
    percent_done: Option<i32>,
    down_speed: Option<i64>,
    estimated_time: Option<i64>,
    error_message: Option<String>,
}

/// Execute transfers list subcommand
pub async fn execute_list(
    output_format: String,
    config_path: Option<PathBuf>,
) -> Result<()> {
    info!("Starting transfers list command");

    let config = load_config(config_path)?;
    let client = create_api_client(&config)?;

    // Get transfers from API
    let response = client.list_transfers().await
        .context("Failed to list transfers from Put.io")?;

    let transfers: Vec<TransferDisplay> = response.transfers
        .into_iter()
        .map(|t| TransferDisplay {
            id: t.id,
            name: t.name,
            status: t.status,
            size: t.size,
            percent_done: t.percent_done,
            down_speed: t.down_speed,
            estimated_time: t.estimated_time.filter(|&time| time > 0),
            error_message: t.error_message,
        })
        .collect();

    // Display based on output format
    match output_format.as_str() {
        "json" => display_json(&transfers)?,
        "table" | _ => display_table(&transfers)?,
    }

    Ok(())
}

/// Execute transfers cancel subcommand
pub async fn execute_cancel(
    transfer_ids: Vec<i64>,
    yes: bool,
    config_path: Option<PathBuf>,
) -> Result<()> {
    info!("Starting transfers cancel command");

    if transfer_ids.is_empty() {
        anyhow::bail!("Must specify at least one transfer ID to cancel");
    }

    let config = load_config(config_path)?;
    let client = create_api_client(&config)?;

    // Confirm deletion unless --yes flag is provided
    if !yes {
        println!("This will cancel {} transfer(s):", transfer_ids.len());
        for id in &transfer_ids {
            println!("  - Transfer {}", id);
        }
        print!("Continue? [y/N] ");
        use std::io::{self, Write};
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled");
            return Ok(());
        }
    }

    // Cancel transfers
    client.cancel_transfers(transfer_ids.clone()).await
        .context("Failed to cancel transfers")?;

    println!("Successfully cancelled {} transfer(s)", transfer_ids.len());

    Ok(())
}

/// Execute transfers retry subcommand
pub async fn execute_retry(
    transfer_id: i64,
    config_path: Option<PathBuf>,
) -> Result<()> {
    info!("Starting transfers retry command");

    let config = load_config(config_path)?;
    let client = create_api_client(&config)?;

    // Retry transfer
    client.retry_transfer(transfer_id).await
        .context("Failed to retry transfer")?;

    println!("Successfully retried transfer {}", transfer_id);

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

/// Create API client from configuration
fn create_api_client(config: &Config) -> Result<ApiClient> {
    let auth_config = config.auth.clone()
        .ok_or_else(|| anyhow::anyhow!("No authentication configured. Run 'putio-sync login' first."))?;
    Ok(ApiClient::from_auth_config(auth_config))
}

/// Display transfers in table format
fn display_table(transfers: &[TransferDisplay]) -> Result<()> {
    if transfers.is_empty() {
        println!("No active transfers");
        return Ok(());
    }

    println!("\nActive Transfers");
    println!("{}", "═".repeat(120));
    println!(
        "{:<8} {:<10} {:<50} {:>12} {:>8} {:>12} {:>8}",
        "ID", "Status", "Name", "Size", "Progress", "Speed", "ETA"
    );
    println!("{}", "─".repeat(120));

    for transfer in transfers {
        let name = if transfer.name.len() > 50 {
            format!("{}...", &transfer.name[..47])
        } else {
            transfer.name.clone()
        };

        let eta = if let Some(seconds) = transfer.estimated_time {
            format_duration(seconds)
        } else {
            "N/A".to_string()
        };

        let status_display = if let Some(ref error) = transfer.error_message {
            format!("{} ({})", transfer.status, error)
        } else {
            transfer.status.clone()
        };

        let percent_display = transfer.percent_done
            .map(|p| format!("{}%", p))
            .unwrap_or_else(|| "N/A".to_string());
        
        println!(
            "{:<8} {:<10} {:<50} {:>12} {:>7} {:>12} {:>8}",
            transfer.id,
            status_display,
            name,
            format_bytes(transfer.size.unwrap_or(0)),
            percent_display,
            format_speed(transfer.down_speed.unwrap_or(0) as f64),
            eta
        );
    }

    println!("{}", "═".repeat(120));
    println!("Total: {} transfer(s)\n", transfers.len());

    Ok(())
}

/// Display transfers in JSON format
fn display_json<T: Serialize>(data: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(data)
        .context("Failed to serialize to JSON")?;
    println!("{}", json);
    Ok(())
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
    fn test_format_speed() {
        assert_eq!(format_speed(0.0), "N/A");
        assert_eq!(format_speed(512.0), "512 B/s");
        assert_eq!(format_speed(1024.0), "1.00 KB/s");
        assert_eq!(format_speed(1048576.0), "1.00 MB/s");
        assert_eq!(format_speed(1073741824.0), "1.00 GB/s");
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
}
