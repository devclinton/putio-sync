//! Download command implementation
//!
//! Handles downloading files from Put.io with support for workers, progress tracking,
//! chunking, resume, and daemon mode.

use anyhow::{anyhow, Context, Result};
use putio_sync_core::api::ApiClient;
use putio_sync_core::config::{Config, DownloadConfig, ProgressStyle, SortOrder};
use putio_sync_core::filter::parse_size;
use std::path::PathBuf;
use tracing::{debug, info};

/// Execute the download command
#[allow(clippy::too_many_arguments)]
pub async fn execute_download(
    files: Vec<i64>,
    all: bool,
    filter: Option<String>,
    dir: Option<PathBuf>,
    workers: Option<usize>,
    chunk_size: Option<String>,
    buffer_size: Option<String>,
    no_resume: bool,
    skip_existing: bool,
    delete_after: bool,
    sort: Option<String>,
    reverse: bool,
    progress: Option<String>,
    daemon: bool,
    interval: u64,
    config_path: Option<PathBuf>,
) -> Result<()> {
    // Validate arguments
    if !all && files.is_empty() {
        return Err(anyhow!(
            "Either provide file IDs or use --all flag to download all files"
        ));
    }

    // Load configuration
    let config_file_path = config_path
        .or_else(|| Config::default_path().ok())
        .ok_or_else(|| anyhow!("Could not determine config file path"))?;

    let config = Config::load(&config_file_path)
        .context("Failed to load configuration. Have you run 'putio-sync login'?")?;

    let auth = config.auth.ok_or_else(|| {
        anyhow!("No authentication credentials found. Please run 'putio-sync login' first.")
    })?;

    // Create download configuration from config + CLI overrides
    let mut download_config = config.download;

    // Apply CLI overrides to download config
    if let Some(d) = dir {
        download_config.target_directory = d;
    }
    if let Some(w) = workers {
        download_config.workers = w;
    }
    if let Some(cs) = chunk_size {
        let size_bytes = parse_size(&cs)
            .map_err(|e| anyhow!("Invalid chunk size '{}': {}", cs, e))?;
        download_config.chunk_size_mb = Some((size_bytes / 1024 / 1024) as usize);
    }
    if let Some(bs) = buffer_size {
        let size_bytes = parse_size(&bs)
            .map_err(|e| anyhow!("Invalid buffer size '{}': {}", bs, e))?;
        download_config.buffer_size_mb = (size_bytes / 1024 / 1024) as usize;
    }
    if no_resume {
        download_config.resume_enabled = false;
    }
    if skip_existing {
        download_config.skip_existing = true;
    }
    if delete_after {
        download_config.delete_after_download = true;
    }
    if let Some(s) = sort {
        download_config.sort_order = parse_sort_order(&s)?;
    }
    if reverse {
        download_config.reverse_sort = true;
    }
    if let Some(p) = progress {
        download_config.progress_style = parse_progress_style(&p)?;
    }

    // Create API client
    let client = ApiClient::from_auth_config(auth);

    info!("Download configuration:");
    debug!("  Workers: {}", download_config.workers);
    debug!("  Target directory: {:?}", download_config.target_directory);
    debug!("  Chunk size: {:?} MB", download_config.chunk_size_mb);
    debug!("  Buffer size: {} MB", download_config.buffer_size_mb);
    debug!("  Resume enabled: {}", download_config.resume_enabled);
    debug!("  Skip existing: {}", download_config.skip_existing);
    debug!("  Delete after: {}", download_config.delete_after_download);
    debug!("  Sort order: {:?}", download_config.sort_order);
    debug!("  Reverse sort: {}", download_config.reverse_sort);
    debug!("  Progress style: {:?}", download_config.progress_style);

    if daemon {
        info!("Starting daemon mode with interval: {} seconds", interval);
        run_daemon_mode(client, download_config, filter, all, files, interval).await
    } else {
        run_single_download(client, download_config, filter, all, files).await
    }
}

/// Run a single download operation
async fn run_single_download(
    _client: ApiClient,
    _download_config: DownloadConfig,
    _filter: Option<String>,
    _all: bool,
    _files: Vec<i64>,
) -> Result<()> {
    // TODO: Implement in section 3.3
    info!("Download command will be implemented in section 3.3");
    Ok(())
}

/// Run in daemon mode
async fn run_daemon_mode(
    _client: ApiClient,
    _download_config: DownloadConfig,
    _filter: Option<String>,
    _all: bool,
    _files: Vec<i64>,
    _interval: u64,
) -> Result<()> {
    // TODO: Implement in section 3.4
    info!("Daemon mode will be implemented in section 3.4");
    Ok(())
}

/// Parse sort order from string
fn parse_sort_order(s: &str) -> Result<SortOrder> {
    match s.to_lowercase().as_str() {
        "date" => Ok(SortOrder::ByDate),
        "name" => Ok(SortOrder::ByName),
        "extension" | "ext" => Ok(SortOrder::ByExtension),
        "size" => Ok(SortOrder::BySize),
        _ => Err(anyhow!(
            "Invalid sort order '{}'. Valid options: date, name, extension, size",
            s
        )),
    }
}

/// Parse progress style from string
fn parse_progress_style(s: &str) -> Result<ProgressStyle> {
    match s.to_lowercase().as_str() {
        "per-file" | "perfile" => Ok(ProgressStyle::PerFile),
        "overall" => Ok(ProgressStyle::Overall),
        "individual" => Ok(ProgressStyle::Individual),
        "rotating" => Ok(ProgressStyle::Rotating),
        "none" => Ok(ProgressStyle::None),
        _ => Err(anyhow!(
            "Invalid progress style '{}'. Valid options: per-file, overall, individual, rotating, none",
            s
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sort_order() {
        assert!(matches!(
            parse_sort_order("date").unwrap(),
            SortOrder::ByDate
        ));
        assert!(matches!(
            parse_sort_order("name").unwrap(),
            SortOrder::ByName
        ));
        assert!(matches!(
            parse_sort_order("extension").unwrap(),
            SortOrder::ByExtension
        ));
        assert!(matches!(
            parse_sort_order("ext").unwrap(),
            SortOrder::ByExtension
        ));
        assert!(matches!(
            parse_sort_order("size").unwrap(),
            SortOrder::BySize
        ));
        assert!(parse_sort_order("invalid").is_err());
    }

    #[test]
    fn test_parse_progress_style() {
        assert!(matches!(
            parse_progress_style("per-file").unwrap(),
            ProgressStyle::PerFile
        ));
        assert!(matches!(
            parse_progress_style("perfile").unwrap(),
            ProgressStyle::PerFile
        ));
        assert!(matches!(
            parse_progress_style("overall").unwrap(),
            ProgressStyle::Overall
        ));
        assert!(matches!(
            parse_progress_style("individual").unwrap(),
            ProgressStyle::Individual
        ));
        assert!(matches!(
            parse_progress_style("rotating").unwrap(),
            ProgressStyle::Rotating
        ));
        assert!(matches!(
            parse_progress_style("none").unwrap(),
            ProgressStyle::None
        ));
        assert!(parse_progress_style("invalid").is_err());
    }
}
