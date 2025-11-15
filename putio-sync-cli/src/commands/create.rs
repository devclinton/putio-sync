//! Create command implementation

use anyhow::{Context, Result};
use putio_sync_core::api::ApiClient;
use putio_sync_core::config::Config;
use std::path::PathBuf;
use tracing::{debug, info};

/// Execute create command
///
/// Adds torrents to Put.io from magnet links or .torrent files
pub async fn execute_create(
    sources: Vec<String>,
    watch: Vec<PathBuf>,
    parent_id: Option<i64>,
    daemon: bool,
    interval: u64,
    config_path: Option<PathBuf>,
) -> Result<()> {
    info!("Starting create command");

    // Validate arguments
    if sources.is_empty() && watch.is_empty() {
        anyhow::bail!("No sources or watch directories specified. Provide torrent files, magnet links, or watch directories.");
    }

    // Load configuration
    let config = load_config(config_path)?;

    // Create API client
    let auth_config = config.auth.ok_or_else(|| {
        anyhow::anyhow!("No authentication configured. Please run 'putio-sync login' first.")
    })?;

    let client = ApiClient::from_auth_config(auth_config);

    // Process source files/links if provided
    if !sources.is_empty() {
        add_sources(&client, &sources, parent_id).await?;
    }

    // Handle watch mode
    if !watch.is_empty() {
        if daemon {
            info!("Starting daemon mode with watch directories: {:?}", watch);
            watch_daemon(&client, &watch, parent_id, interval).await?;
        } else {
            info!("Processing watch directories once: {:?}", watch);
            process_watch_directories(&client, &watch, parent_id).await?;
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

/// Add sources (torrent files or magnet links) to Put.io
async fn add_sources(
    client: &ApiClient,
    sources: &[String],
    parent_id: Option<i64>,
) -> Result<()> {
    info!("Adding {} source(s)", sources.len());

    for source in sources {
        debug!("Processing source: {}", source);

        // Detect if source is a file or magnet link
        if is_magnet_link(source) {
            add_magnet_link(client, source, parent_id).await?;
        } else if is_torrent_file(source) {
            add_torrent_file(client, source, parent_id).await?;
        } else {
            anyhow::bail!(
                "Invalid source: '{}'. Must be a magnet link (magnet:?...) or .torrent file",
                source
            );
        }
    }

    Ok(())
}

/// Check if a string is a magnet link
fn is_magnet_link(s: &str) -> bool {
    s.starts_with("magnet:?")
}

/// Check if a string is a .torrent file path
fn is_torrent_file(s: &str) -> bool {
    let path = std::path::Path::new(s);
    path.exists() && path.extension().and_then(|e| e.to_str()) == Some("torrent")
}

/// Add a magnet link to Put.io
async fn add_magnet_link(
    client: &ApiClient,
    magnet: &str,
    parent_id: Option<i64>,
) -> Result<()> {
    info!("Adding magnet link");
    debug!("Magnet: {}", magnet);

    let transfer = client
        .add_transfer_url(magnet, parent_id, None)
        .await
        .context("Failed to add magnet link")?;

    println!(
        "✓ Transfer added: {} (ID: {})",
        transfer.name, transfer.id
    );
    println!("  Status: {}", transfer.status);
    println!("  Size: {}", format_bytes(transfer.size));
    if let Some(parent) = parent_id {
        println!("  Parent folder: {}", parent);
    }

    Ok(())
}

/// Add a .torrent file to Put.io
async fn add_torrent_file(
    client: &ApiClient,
    file_path: &str,
    parent_id: Option<i64>,
) -> Result<()> {
    let path = std::path::Path::new(file_path);
    info!("Adding .torrent file: {}", path.display());

    // Verify file exists
    if !path.exists() {
        anyhow::bail!("File not found: {}", path.display());
    }

    // Verify file extension
    if path.extension().and_then(|e| e.to_str()) != Some("torrent") {
        anyhow::bail!("Invalid file extension. Expected .torrent file");
    }

    let transfer = client
        .add_transfer_file(path, parent_id, None)
        .await
        .context("Failed to upload .torrent file")?;

    println!(
        "✓ Transfer added: {} (ID: {})",
        transfer.name, transfer.id
    );
    println!("  Status: {}", transfer.status);
    println!("  Size: {}", format_bytes(transfer.size));
    if let Some(parent) = parent_id {
        println!("  Parent folder: {}", parent);
    }

    Ok(())
}

/// Process watch directories once
async fn process_watch_directories(
    client: &ApiClient,
    watch_dirs: &[PathBuf],
    parent_id: Option<i64>,
) -> Result<()> {
    info!("Processing watch directories");

    for dir in watch_dirs {
        if !dir.exists() {
            eprintln!("Warning: Watch directory does not exist: {}", dir.display());
            continue;
        }

        if !dir.is_dir() {
            eprintln!("Warning: Not a directory: {}", dir.display());
            continue;
        }

        debug!("Scanning directory: {}", dir.display());

        // Find all .torrent files in the directory
        let torrent_files = find_torrent_files(dir)?;

        if torrent_files.is_empty() {
            println!("No .torrent files found in {}", dir.display());
        } else {
            println!("Found {} .torrent file(s) in {}", torrent_files.len(), dir.display());

            for file in torrent_files {
                add_torrent_file(client, &file.to_string_lossy(), parent_id).await?;
            }
        }
    }

    Ok(())
}

/// Watch directories in daemon mode
async fn watch_daemon(
    _client: &ApiClient,
    _watch_dirs: &[PathBuf],
    _parent_id: Option<i64>,
    interval: u64,
) -> Result<()> {
    info!("Starting daemon mode with {}s interval", interval);

    // TODO: Implement filesystem watching with notify crate
    // For now, use polling approach
    anyhow::bail!("Daemon mode not yet implemented. Will be added in Phase 7.4 (Watch Mode Implementation).");
}

/// Find all .torrent files in a directory (non-recursive)
fn find_torrent_files(dir: &PathBuf) -> Result<Vec<PathBuf>> {
    let mut torrent_files = Vec::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "torrent" {
                    torrent_files.push(path);
                }
            }
        }
    }

    Ok(torrent_files)
}

/// Format bytes as human-readable string
fn format_bytes(bytes: i64) -> String {
    const KB: i64 = 1024;
    const MB: i64 = KB * 1024;
    const GB: i64 = MB * 1024;
    const TB: i64 = GB * 1024;

    if bytes == 0 {
        return "Unknown".to_string();
    } else if bytes >= TB {
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
    fn test_is_magnet_link() {
        assert!(is_magnet_link("magnet:?xt=urn:btih:1234567890abcdef"));
        assert!(!is_magnet_link("http://example.com/file.torrent"));
        assert!(!is_magnet_link("file.torrent"));
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "Unknown");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048576), "1.00 MB");
        assert_eq!(format_bytes(1073741824), "1.00 GB");
        assert_eq!(format_bytes(1099511627776), "1.00 TB");
    }
}
