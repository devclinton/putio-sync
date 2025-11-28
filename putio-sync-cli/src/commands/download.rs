//! Download command implementation
//!
//! Handles downloading files from Put.io with support for workers, progress tracking,
//! chunking, resume, and daemon mode.

use anyhow::{anyhow, Context, Result};
use putio_sync_core::api::{ApiClient, FileEntry};
use putio_sync_core::config::{Config, DownloadConfig, ProgressStyle, SortOrder};
use putio_sync_core::db::Database;
use putio_sync_core::download::{DownloadFile, DownloadManager};
use putio_sync_core::filter::{parse_size, sort_files, FileFilter, SortField};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info, warn};

use crate::ui::ProgressDisplay;

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
    bandwidth_limit: Option<String>,
    no_resume: bool,
    skip_existing: bool,
    delete_after: bool,
    delete_empty_folders: Option<bool>,
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
    
    info!("Config loaded - target_directory: {}", download_config.target_directory.display());

    // Apply CLI overrides to download config
    if let Some(d) = dir {
        info!("CLI override - original dir argument: {:?}", d);
        download_config.target_directory = putio_sync_core::path_utils::expand_path(d)
            .context("Failed to expand target directory path")?;
        info!("CLI override - expanded target_directory: {}", download_config.target_directory.display());
    } else {
        info!("No CLI override for target directory");
    }
    if let Some(w) = workers {
        download_config.workers = w;
    }
    if let Some(cs) = chunk_size {
        warn!("Chunk size specified but chunked downloads not supported - using streaming with range request resume instead");
        let _size_bytes = parse_size(&cs)
            .map_err(|e| anyhow!("Invalid chunk size '{}': {}", cs, e))?;
        // Don't set chunk_size_mb since chunked downloads are not supported
        download_config.chunk_size_mb = None;
    }
    if let Some(bs) = buffer_size {
        let size_bytes = parse_size(&bs)
            .map_err(|e| anyhow!("Invalid buffer size '{}': {}", bs, e))?;
        download_config.buffer_size_mb = (size_bytes / 1024 / 1024) as usize;
    }
    if let Some(bl) = bandwidth_limit {
        let limit_bytes_per_sec = parse_size(&bl)
            .map_err(|e| anyhow!("Invalid bandwidth limit '{}': {}", bl, e))?;
        download_config.bandwidth_limit_bytes_per_sec = Some(limit_bytes_per_sec as u64);
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
    if let Some(def) = delete_empty_folders {
        download_config.delete_empty_folders = def;
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
    debug!("  Delete empty folders: {}", download_config.delete_empty_folders);
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
    client: ApiClient,
    download_config: DownloadConfig,
    filter: Option<String>,
    all: bool,
    files: Vec<i64>,
) -> Result<()> {
    let start_time = Instant::now();

    // Create target directory if it doesn't exist
    if !download_config.target_directory.exists() {
        info!(
            "Creating target directory: {}",
            download_config.target_directory.display()
        );
        std::fs::create_dir_all(&download_config.target_directory).with_context(|| {
            format!(
                "Failed to create target directory: {}",
                download_config.target_directory.display()
            )
        })?;
    }

    // Initialize database
    let db_path = download_config.target_directory.join(".putiosync.db");
    let database = Arc::new(
        Database::new(&db_path)
            .await
            .context("Failed to initialize database")?,
    );

    // Get list of files to download
    info!("Fetching file list from Put.io...");
    let file_entries = if all {
        // Get all files from root folder
        let response = client
            .list_files(0)
            .await
            .context("Failed to fetch file list from Put.io")?;
        response.files
    } else {
        // Get specific files by ID
        let mut entries = Vec::new();
        for file_id in files {
            match client.get_file(file_id).await {
                Ok(file_response) => entries.push(file_response.file),
                Err(e) => {
                    warn!("Failed to get file {}: {}", file_id, e);
                }
            }
        }
        entries
    };

    // Apply filters
    let mut filtered_files = apply_filters(file_entries, filter)?;

    // Apply sorting
    filtered_files = apply_sorting(filtered_files, &download_config)?;

    if filtered_files.is_empty() {
        info!("No files to download after applying filters");
        return Ok(());
    }

    info!("Found {} files to download", filtered_files.len());

    // Calculate total size
    let total_size: u64 = filtered_files
        .iter()
        .filter(|f| !f.is_folder)
        .map(|f| f.size.max(0) as u64)
        .sum();

    info!(
        "Total download size: {} ({} bytes)",
        format_size(total_size),
        total_size
    );

    // Create progress display
    let progress_display = Arc::new(ProgressDisplay::new(
        download_config.progress_style.clone(),
        filtered_files.len() as u64,
        total_size,
    ));

    // Create download manager
    let client_arc = Arc::new(client);
    let manager = DownloadManager::new(
        client_arc.clone(),
        database.clone(),
        download_config.clone(),
    );

    // Convert file entries to download files
    let download_files: Vec<DownloadFile> = filtered_files
        .iter()
        .filter(|f| !f.is_folder)
        .map(|f| DownloadFile {
            file_id: f.id,
            filename: f.name.clone(),
            relative_path: "".to_string(), // Non-daemon downloads don't preserve directory structure by default
            file_size: f.size.max(0) as u64,
            download_url: client_arc.get_download_url(f.id),
        })
        .collect();

    // Set up Ctrl+C handler
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    ctrlc::set_handler(move || {
        info!("Received interrupt signal, shutting down gracefully...");
        shutdown_clone.store(true, std::sync::atomic::Ordering::SeqCst);
    })
    .context("Failed to set Ctrl+C handler")?;

    // Download files
    info!("Starting download of {} files...", download_files.len());

    let mut successful = 0;
    let mut failed = 0;
    let mut total_bytes = 0u64;

    for download_file in download_files {
        // Check for shutdown signal
        if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
            warn!("Download interrupted by user");
            break;
        }

        let callback = progress_display.clone().create_callback();

        match manager
            .download_file(download_file.clone(), Some(callback))
            .await
        {
            Ok(result) => {
                if result.success {
                    successful += 1;
                    total_bytes += result.bytes_downloaded;
                } else {
                    failed += 1;
                    error!(
                        "Download failed for file {}: {}",
                        download_file.file_id,
                        result.error.unwrap_or_else(|| "Unknown error".to_string())
                    );
                }
            }
            Err(e) => {
                failed += 1;
                error!("Download failed for file {}: {}", download_file.file_id, e);
            }
        }
    }

    // Finish progress display
    progress_display.finish();

    // Display summary
    let elapsed = start_time.elapsed();
    let avg_speed = if elapsed.as_secs() > 0 {
        total_bytes as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };

    println!("\n📊 Download Summary");
    println!("═══════════════════════════════════════════════");
    println!("Files downloaded:  {}", successful);
    if failed > 0 {
        println!("Files failed:      {}", failed);
    }
    println!("Total size:        {} ({} bytes)", format_size(total_bytes), total_bytes);
    println!("Time elapsed:      {:.2}s", elapsed.as_secs_f64());
    println!("Average speed:     {}/s", format_size(avg_speed as u64));
    println!("═══════════════════════════════════════════════");

    if failed > 0 {
        return Err(anyhow!("{} files failed to download", failed));
    }

    Ok(())
}

/// Run in daemon mode
async fn run_daemon_mode(
    client: ApiClient,
    download_config: DownloadConfig,
    filter: Option<String>,
    all: bool,
    files: Vec<i64>,
    interval: u64,
) -> Result<()> {
    info!("Starting daemon mode with interval: {} seconds", interval);

    // Create target directory if it doesn't exist
    if !download_config.target_directory.exists() {
        info!(
            "Creating target directory: {}",
            download_config.target_directory.display()
        );
        std::fs::create_dir_all(&download_config.target_directory).with_context(|| {
            format!(
                "Failed to create target directory: {}",
                download_config.target_directory.display()
            )
        })?;
    }

    // Initialize database (not used in new implementation)
    let db_path = download_config.target_directory.join(".putiosync.db");
    let _database = Arc::new(
        Database::new(&db_path)
            .await
            .context("Failed to initialize database")?,
    );

    // Set up Ctrl+C handler
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    ctrlc::set_handler(move || {
        info!("Received interrupt signal, shutting down daemon gracefully...");
        shutdown_clone.store(true, std::sync::atomic::Ordering::SeqCst);
    })
    .context("Failed to set Ctrl+C handler")?;

    let client_arc = Arc::new(client);
    let mut cycle_count = 0;

    loop {
        cycle_count += 1;
        info!("Daemon cycle #{} starting...", cycle_count);

        // Check for shutdown signal
        if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
            info!("Daemon shutting down gracefully...");
            break;
        }

        // Use DownloadOps for recursive downloads
        let sync_parent_id = 0; // Use root for now, this could be configurable
        let download_ops = match crate::download_ops::DownloadOps::new(
            client_arc.clone(),
            download_config.clone(),
            sync_parent_id,
        )
        .await
        {
            Ok(ops) => ops,
            Err(e) => {
                error!("Failed to create download operations: {}", e);
                warn!("Waiting {} seconds before retry...", interval);
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
                continue;
            }
        };

        // Run sync cycle with recursive downloads
        match download_ops
            .run_sync_cycle(filter.as_deref(), all, &files, true) // true = show progress
            .await
        {
            Ok(result) => {
                if result.is_empty() {
                    info!("No new files to download in this cycle");
                } else {
                    info!(
                        "Cycle #{} completed: {} files found, {} downloaded, {} failed in {:.2}s",
                        cycle_count,
                        result.files_found,
                        result.files_downloaded,
                        result.files_failed,
                        result.duration_secs
                    );
                }
            }
            Err(e) => {
                error!("Download cycle failed: {}", e);
                warn!("Waiting {} seconds before retry...", interval);
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
                continue;
            }
        }

        // Wait for next cycle
        info!("Waiting {} seconds until next cycle...", interval);

        // Sleep in smaller intervals to allow for responsive shutdown
        let sleep_interval = std::cmp::min(interval, 5); // Check every 5 seconds max
        let iterations = interval / sleep_interval;
        let remainder = interval % sleep_interval;

        for _ in 0..iterations {
            if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
                info!("Daemon shutting down gracefully...");
                return Ok(());
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(sleep_interval)).await;
        }

        if remainder > 0 {
            if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
                info!("Daemon shutting down gracefully...");
                return Ok(());
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(remainder)).await;
        }
    }

    info!("Daemon mode exited");
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

/// Apply filters to file list
fn apply_filters(files: Vec<FileEntry>, filter: Option<String>) -> Result<Vec<FileEntry>> {
    let mut filtered = files;

    // Apply glob pattern filter if specified
    if let Some(pattern) = filter {
        let file_filter = FileFilter::new().with_name_pattern(pattern);
        filtered.retain(|f| file_filter.matches(f));
    }

    Ok(filtered)
}

/// Apply sorting to file list
fn apply_sorting(files: Vec<FileEntry>, config: &DownloadConfig) -> Result<Vec<FileEntry>> {
    let sort_field = match config.sort_order {
        SortOrder::ByDate => SortField::Date,
        SortOrder::ByName => SortField::Name,
        SortOrder::ByExtension => SortField::Name, // Use Name as fallback for extension
        SortOrder::BySize => SortField::Size,
    };

    Ok(sort_files(files, sort_field, config.reverse_sort))
}

/// Format size in bytes to human-readable format
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[unit_idx])
    } else {
        format!("{:.2} {}", size, UNITS[unit_idx])
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
