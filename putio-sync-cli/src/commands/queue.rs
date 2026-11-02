//! Queue command implementation

use anyhow::{Context, Result};
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::{AuthMethod, Config};
use putio_sync_core::filter::FileFilter;
use putio_sync_db::models::{QueueItem, QueueStatus};
use putio_sync_db::{Database, QueueStats};
use serde::Serialize;
use std::path::PathBuf;
use tracing::{debug, info};

/// Queue statistics for display
#[derive(Debug, Serialize)]
struct QueueStatsDisplay {
    total: i64,
    pending: i64,
    downloading: i64,
    paused: i64,
    completed: i64,
    failed: i64,
}

impl From<QueueStats> for QueueStatsDisplay {
    fn from(stats: QueueStats) -> Self {
        Self {
            total: stats.total,
            pending: stats.pending,
            downloading: stats.downloading,
            paused: stats.paused,
            completed: stats.completed,
            failed: stats.failed,
        }
    }
}

/// Execute queue list subcommand
pub async fn execute_list(
    status_filter: Option<String>,
    output_format: String,
    config_path: Option<PathBuf>,
) -> Result<()> {
    info!("Starting queue list command");

    let config = load_config(config_path)?;
    let db = open_database(&config).await?;
    let queue_repo = db.queue();

    // Get queue items
    let items = if let Some(status) = status_filter {
        let status_enum: QueueStatus = status
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid status '{}': {}", status, e))?;

        // Get all items and filter by status
        queue_repo
            .get_all()
            .await
            .context("Failed to get queue items")?
            .into_iter()
            .filter(|item| item.status == status_enum.to_string())
            .collect()
    } else {
        queue_repo
            .get_all()
            .await
            .context("Failed to get queue items")?
    };

    // Display based on output format
    match output_format.as_str() {
        "json" => display_json(&items)?,
        _ => display_queue_table(&items)?,
    }

    Ok(())
}

/// Execute queue add subcommand
pub async fn execute_add(
    file_ids: Vec<i64>,
    priority: i64,
    all: bool,
    filter: Option<String>,
    config_path: Option<PathBuf>,
) -> Result<()> {
    info!("Starting queue add command");

    let config = load_config(config_path)?;
    let client = create_api_client(&config)?;
    let db = open_database(&config).await?;
    let queue_repo = db.queue();

    // Determine which files to add
    let files_to_add = if all || filter.is_some() {
        // List all files and filter
        let response = client
            .list_files(0)
            .await
            .context("Failed to list files from Put.io")?;

        // Apply name filter if specified
        if let Some(name_pattern) = filter {
            let file_filter = FileFilter::new().with_name_pattern(name_pattern);
            file_filter.filter(response.files)
        } else {
            response.files
        }
    } else if !file_ids.is_empty() {
        // Get specific files
        let mut files = Vec::new();
        for file_id in file_ids {
            let response = client
                .get_file_details(file_id)
                .await
                .with_context(|| format!("Failed to get file {}", file_id))?;
            files.push(response.file);
        }
        files
    } else {
        anyhow::bail!("Must specify either file IDs, --all, or --filter");
    };

    if files_to_add.is_empty() {
        println!("No files to add to queue");
        return Ok(());
    }

    // Add files to queue
    let mut added = 0;
    for file in files_to_add {
        // Skip folders
        if file.file_type == "FOLDER" {
            debug!("Skipping folder: {}", file.name);
            continue;
        }

        let download_url = client.get_download_url(file.id);

        queue_repo
            .add(
                file.id,
                &file.name,
                file.size,
                &download_url,
                "",
                priority,
                QueueStatus::Pending,
            )
            .await
            .with_context(|| format!("Failed to add file {} to queue", file.id))?;

        added += 1;
        println!("Added to queue: {} (priority: {})", file.name, priority);
    }

    println!("\nAdded {} file(s) to download queue", added);

    Ok(())
}

/// Execute queue pause subcommand
pub async fn execute_pause(file_ids: Vec<i64>, config_path: Option<PathBuf>) -> Result<()> {
    info!("Starting queue pause command");

    let config = load_config(config_path)?;
    let db = open_database(&config).await?;
    let queue_repo = db.queue();

    let count = if file_ids.is_empty() {
        // Pause all
        queue_repo
            .pause_all()
            .await
            .context("Failed to pause queue")?
    } else {
        // Pause specific files
        let mut paused = 0;
        for file_id in file_ids {
            queue_repo
                .update_status(file_id, QueueStatus::Paused)
                .await
                .with_context(|| format!("Failed to pause file {}", file_id))?;
            paused += 1;
        }
        paused
    };

    println!("Paused {} queue item(s)", count);

    Ok(())
}

/// Execute queue resume subcommand
pub async fn execute_resume(file_ids: Vec<i64>, config_path: Option<PathBuf>) -> Result<()> {
    info!("Starting queue resume command");

    let config = load_config(config_path)?;
    let db = open_database(&config).await?;
    let queue_repo = db.queue();

    let count = if file_ids.is_empty() {
        // Resume all
        queue_repo
            .resume_all()
            .await
            .context("Failed to resume queue")?
    } else {
        // Resume specific files
        let mut resumed = 0;
        for file_id in file_ids {
            queue_repo
                .update_status(file_id, QueueStatus::Pending)
                .await
                .with_context(|| format!("Failed to resume file {}", file_id))?;
            resumed += 1;
        }
        resumed
    };

    println!("Resumed {} queue item(s)", count);

    Ok(())
}

/// Execute queue remove subcommand
pub async fn execute_remove(file_ids: Vec<i64>, config_path: Option<PathBuf>) -> Result<()> {
    info!("Starting queue remove command");

    if file_ids.is_empty() {
        anyhow::bail!("Must specify at least one file ID to remove");
    }

    let config = load_config(config_path)?;
    let db = open_database(&config).await?;
    let queue_repo = db.queue();

    for file_id in &file_ids {
        queue_repo
            .remove(*file_id)
            .await
            .with_context(|| format!("Failed to remove file {} from queue", file_id))?;
        println!("Removed file {} from queue", file_id);
    }

    println!("\nRemoved {} file(s) from queue", file_ids.len());

    Ok(())
}

/// Execute queue clear subcommand
pub async fn execute_clear(yes: bool, config_path: Option<PathBuf>) -> Result<()> {
    info!("Starting queue clear command");

    let config = load_config(config_path)?;
    let db = open_database(&config).await?;
    let queue_repo = db.queue();

    // Check how many completed items exist
    let items = queue_repo.get_all().await?;
    let completed_count = items
        .iter()
        .filter(|item| item.status == QueueStatus::Completed.to_string())
        .count();

    if completed_count == 0 {
        println!("No completed items to clear");
        return Ok(());
    }

    // Confirm deletion unless --yes flag is provided
    if !yes {
        println!(
            "This will remove {} completed item(s) from the queue.",
            completed_count
        );
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

    let cleared = queue_repo
        .clear_completed()
        .await
        .context("Failed to clear completed items")?;

    println!("Cleared {} completed item(s) from queue", cleared);

    Ok(())
}

/// Execute queue priority subcommand
pub async fn execute_priority(
    file_id: i64,
    priority: i64,
    config_path: Option<PathBuf>,
) -> Result<()> {
    info!("Starting queue priority command");

    let config = load_config(config_path)?;
    let db = open_database(&config).await?;
    let queue_repo = db.queue();

    queue_repo
        .update_priority(file_id, priority)
        .await
        .with_context(|| format!("Failed to update priority for file {}", file_id))?;

    println!("Updated priority for file {} to {}", file_id, priority);

    Ok(())
}

/// Execute queue stats subcommand
pub async fn execute_stats(output_format: String, config_path: Option<PathBuf>) -> Result<()> {
    info!("Starting queue stats command");

    let config = load_config(config_path)?;
    let db = open_database(&config).await?;
    let queue_repo = db.queue();

    let stats = queue_repo
        .get_stats()
        .await
        .context("Failed to get queue statistics")?;

    let display_stats = QueueStatsDisplay::from(stats);

    // Display based on output format
    match output_format.as_str() {
        "json" => {
            let json = serde_json::to_string_pretty(&display_stats)
                .context("Failed to serialize statistics to JSON")?;
            println!("{}", json);
        }
        _ => {
            println!("\nQueue Statistics");
            println!("{}", "═".repeat(50));
            println!("Total Items:          {:>12}", display_stats.total);
            println!("Pending:              {:>12}", display_stats.pending);
            println!("Downloading:          {:>12}", display_stats.downloading);
            println!("Paused:               {:>12}", display_stats.paused);
            println!("Completed:            {:>12}", display_stats.completed);
            println!("Failed:               {:>12}", display_stats.failed);
            println!("{}", "═".repeat(50));
            println!();
        }
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

/// Create API client from configuration
fn create_api_client(config: &Config) -> Result<PutioApiClient> {
    let auth_config = config.auth.clone().ok_or_else(|| {
        anyhow::anyhow!("No authentication configured. Run 'putio-sync login' first.")
    })?;

    let client = match auth_config.auth_method {
        AuthMethod::Token => {
            let token = auth_config
                .token
                .ok_or_else(|| anyhow::anyhow!("Token not configured"))?;
            PutioApiClient::new(token)
        }
        AuthMethod::AppPassword => {
            let username = auth_config
                .username
                .ok_or_else(|| anyhow::anyhow!("Username not configured"))?;
            let password = auth_config
                .password
                .ok_or_else(|| anyhow::anyhow!("Password not configured"))?;
            PutioApiClient::with_credentials(username, password)
        }
    };

    Ok(client)
}

/// Open database connection
async fn open_database(config: &Config) -> Result<Database> {
    let db_path = config.download.target_directory.join(".putiosync.db");
    debug!("Opening database at: {:?}", db_path);

    Database::new(&db_path)
        .await
        .context("Failed to open database")
}

/// Display queue items in table format
fn display_queue_table(items: &[QueueItem]) -> Result<()> {
    if items.is_empty() {
        println!("Queue is empty");
        return Ok(());
    }

    println!("\nDownload Queue");
    println!("{}", "═".repeat(100));
    println!(
        "{:<8} {:<10} {:<40} {:>12} {:>8}",
        "File ID", "Status", "Filename", "Size", "Priority"
    );
    println!("{}", "─".repeat(100));

    for item in items {
        let filename = if item.filename.len() > 40 {
            format!("{}...", &item.filename[..37])
        } else {
            item.filename.clone()
        };

        println!(
            "{:<8} {:<10} {:<40} {:>12} {:>8}",
            item.file_id,
            item.status,
            filename,
            format_bytes(item.file_size),
            item.priority
        );
    }

    println!("{}", "═".repeat(100));
    println!("Total: {} item(s)\n", items.len());

    Ok(())
}

/// Display items in JSON format
fn display_json<T: Serialize>(data: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(data).context("Failed to serialize to JSON")?;
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
}
