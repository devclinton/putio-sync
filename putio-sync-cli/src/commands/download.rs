//! Download command implementation
//!
//! Handles downloading files from Put.io with support for workers, progress tracking,
//! resume, and daemon mode.

use anyhow::{anyhow, Context, Result};
use putio_sync_client::{FileEntry, PutioApiClient};
use putio_sync_core::config::{AuthMethod, Config, ExistingFile, ProgressStyle, SortOrder};
use putio_sync_core::filter::{parse_size, sort_files, FileFilter, SortField};
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadFile, DownloadManager};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info, warn};

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
    on_existing: Option<ExistingFile>,
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

    // Get old config for reference
    let old_config = config.download;

    // Build new DownloadConfig with fields that map directly
    let mut target_directory = old_config.target_directory.clone();
    if let Some(d) = dir {
        info!("CLI override - original dir argument: {:?}", d);
        target_directory = putio_sync_core::path_utils::expand_path(d)
            .context("Failed to expand target directory path")?;
        info!(
            "CLI override - expanded target_directory: {}",
            target_directory.display()
        );
    }

    let mut buffer_size_mb = old_config.buffer_size_mb;
    if let Some(bs) = buffer_size {
        let size_bytes =
            parse_size(&bs).map_err(|e| anyhow!("Invalid buffer size '{}': {}", bs, e))?;
        buffer_size_mb = (size_bytes / 1024 / 1024) as usize;
    }

    // The schedule applies unless a limit is given here, which holds for the whole run
    let mut bandwidth_limit_bytes_per_sec = old_config.bandwidth_limit_bytes_per_sec;
    let mut bandwidth_schedule = old_config.bandwidth_schedule.clone();
    if let Some(bl) = bandwidth_limit {
        let limit_bytes_per_sec =
            parse_size(&bl).map_err(|e| anyhow!("Invalid bandwidth limit '{}': {}", bl, e))?;
        if limit_bytes_per_sec == 0 {
            return Err(anyhow!("Invalid bandwidth limit '{}': must be above 0", bl));
        }
        bandwidth_limit_bytes_per_sec = Some(limit_bytes_per_sec as u64);
        bandwidth_schedule.clear();
    }

    // Folder rules still override it per folder
    let on_existing = on_existing.unwrap_or_else(|| old_config.on_existing());
    let resume_enabled = !no_resume && old_config.resume_enabled;

    // Handle chunk size warning
    if chunk_size.is_some() {
        warn!("Chunk size specified but chunked downloads not supported - using streaming with range request resume instead");
    }

    // Store settings that need to be handled separately
    let workers_count = workers.unwrap_or(old_config.workers);

    // Create new download config
    let delete_after_download = delete_after || old_config.delete_after_download;
    let download_config = DownloadConfig {
        target_directory: target_directory.clone(),
        on_existing,
        resume_enabled,
        buffer_size_mb,
        bandwidth_limit_bytes_per_sec,
        bandwidth_schedule,
        slow_limit_bytes_per_sec: old_config.slow_limit_bytes_per_sec,
        max_retries: old_config.max_retries,
        retry_initial_delay_secs: old_config.retry_initial_delay_secs,
        workers: workers_count,
        delete_after_download,
        rules: putio_sync_core::rules::RuleSet::from_config(&old_config),
        verify: putio_sync_download::manager::VerifySettings::from(&old_config),
    };
    let delete_empty_folders_flag = delete_empty_folders.unwrap_or(old_config.delete_empty_folders);
    let sort_order = if let Some(s) = sort {
        parse_sort_order(&s)?
    } else {
        old_config.sort_order
    };
    let reverse_sort = reverse || old_config.reverse_sort;
    let progress_style = if let Some(p) = progress {
        parse_progress_style(&p)?
    } else {
        old_config.progress_style
    };

    // Create API client
    let client = match auth.auth_method {
        AuthMethod::Token => {
            let token = auth.token.ok_or_else(|| anyhow!("Token not configured"))?;
            PutioApiClient::new(token)
        }
        AuthMethod::AppPassword => {
            let username = auth
                .username
                .ok_or_else(|| anyhow!("Username not configured"))?;
            let password = auth
                .password
                .ok_or_else(|| anyhow!("Password not configured"))?;
            PutioApiClient::with_credentials(username, password)
        }
    };

    info!("Download configuration:");
    debug!("  Workers: {}", workers_count);
    debug!("  Target directory: {:?}", download_config.target_directory);
    debug!("  Buffer size: {} MB", download_config.buffer_size_mb);
    debug!("  Resume enabled: {}", resume_enabled);
    debug!("  On existing: {}", download_config.on_existing);
    debug!("  Delete after: {}", delete_after_download);
    debug!("  Delete empty folders: {}", delete_empty_folders_flag);
    debug!("  Sort order: {:?}", sort_order);
    debug!("  Reverse sort: {}", reverse_sort);
    debug!("  Progress style: {:?}", progress_style);

    let client_arc = Arc::new(client);

    if daemon {
        info!("Starting daemon mode with interval: {} seconds", interval);
        run_daemon_mode(
            client_arc,
            download_config,
            filter,
            all,
            files,
            interval,
            workers_count,
            delete_after_download,
            delete_empty_folders_flag,
            sort_order,
            reverse_sort,
            progress_style,
        )
        .await
    } else {
        run_single_download(
            client_arc,
            download_config,
            filter,
            all,
            files,
            workers_count,
            delete_after_download,
            delete_empty_folders_flag,
            sort_order,
            reverse_sort,
            progress_style,
        )
        .await
    }
}

/// Run a single download operation
#[allow(clippy::too_many_arguments)]
async fn run_single_download(
    client: Arc<PutioApiClient>,
    download_config: DownloadConfig,
    filter: Option<String>,
    all: bool,
    file_ids: Vec<i64>,
    _workers_count: usize,
    delete_after_download: bool,
    delete_empty_folders: bool,
    sort_order: SortOrder,
    reverse_sort: bool,
    _progress_style: ProgressStyle,
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

    // Initialize database - use fallback if it fails
    let db_path = download_config.target_directory.join(".putiosync.db");
    let database = match Database::new(&db_path).await {
        Ok(db) => {
            info!(
                "Database initialized successfully at: {}",
                db_path.display()
            );
            Some(Arc::new(db))
        }
        Err(e) => {
            warn!(
                "Failed to initialize database at {}: {}. Downloads will continue without database tracking.",
                db_path.display(),
                e
            );
            None
        }
    };

    // Fetch file list from Put.io
    info!("Fetching file list from Put.io...");
    let mut files_to_download: Vec<FileEntry> = if all {
        // Get all files recursively
        list_all_files(&client, 0).await?
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
        return Err(anyhow!("No files to download"));
    };

    // Apply filter if specified
    if let Some(filter_pattern) = filter {
        let file_filter = FileFilter::new().with_name_pattern(filter_pattern);
        files_to_download = file_filter.filter(files_to_download);
    }

    // Filter out folders - only download files
    files_to_download.retain(|f| f.file_type != "FOLDER");

    if files_to_download.is_empty() {
        info!("No files to download after filtering");
        return Ok(());
    }

    // Apply sorting
    let sort_field = match sort_order {
        SortOrder::ByDate => SortField::Date,
        SortOrder::ByName => SortField::Name,
        SortOrder::ByExtension => SortField::Extension,
        SortOrder::BySize => SortField::Size,
    };
    files_to_download = sort_files(files_to_download, sort_field, reverse_sort);

    info!("Found {} files to download", files_to_download.len());

    // Create DownloadManager
    let download_manager = if let Some(db) = database.clone() {
        DownloadManager::new(client.clone(), db, download_config.clone())
    } else {
        DownloadManager::new_without_database(client.clone(), download_config.clone())
    };

    // Track statistics
    let mut total_bytes = 0u64;
    let mut files_downloaded = 0usize;
    let mut files_failed = 0usize;

    // Download each file
    for (idx, file) in files_to_download.iter().enumerate() {
        info!(
            "Downloading file {}/{}: {} ({} bytes)",
            idx + 1,
            files_to_download.len(),
            file.name,
            file.size
        );

        let download_url = client.get_download_url(file.id);
        let download_file = DownloadFile {
            file_id: file.id,
            filename: file.name.clone(),
            relative_path: file.name.clone(), // For single downloads, use filename as path
            file_size: file.size as u64,
            download_url,
            crc32: file.crc32.clone(),
            save_as: None,
            fixed_path: false,
        };

        match download_manager.download_file(download_file, None).await {
            Ok(result) => {
                if result.success {
                    info!("✓ Downloaded: {}", file.name);
                    total_bytes += result.bytes_downloaded;
                    files_downloaded += 1;

                    // Delete from Put.io if requested
                    if delete_after_download {
                        match client.delete_files(vec![file.id]).await {
                            Ok(_) => info!("Deleted from Put.io: {}", file.name),
                            Err(e) => warn!("Failed to delete {} from Put.io: {}", file.name, e),
                        }
                    }
                } else {
                    error!(
                        "✗ Failed to download {}: {}",
                        file.name,
                        result.error.unwrap_or_else(|| "Unknown error".to_string())
                    );
                    files_failed += 1;
                }
            }
            Err(e) => {
                error!("✗ Failed to download {}: {}", file.name, e);
                files_failed += 1;
            }
        }
    }

    // Clean up empty folders if requested
    if delete_empty_folders {
        if let Err(e) = cleanup_empty_directories(&download_config.target_directory) {
            warn!("Failed to cleanup empty directories: {}", e);
        }
    }

    let duration = start_time.elapsed();
    let duration_secs = duration.as_secs_f64();

    // Display summary
    println!("\n📊 Download Summary");
    println!("═══════════════════════════════════════════════");
    println!("Files downloaded:  {}", files_downloaded);
    if files_failed > 0 {
        println!("Files failed:      {}", files_failed);
    }
    println!(
        "Total size:        {} ({} bytes)",
        format_size(total_bytes),
        total_bytes
    );
    println!("Time elapsed:      {:.2}s", duration_secs);
    let avg_speed = if duration_secs > 0.0 {
        total_bytes as f64 / duration_secs
    } else {
        0.0
    };
    println!("Average speed:     {}/s", format_size(avg_speed as u64));
    println!("═══════════════════════════════════════════════");

    if files_failed > 0 {
        return Err(anyhow!("{} files failed to download", files_failed));
    }

    Ok(())
}

/// Run in daemon mode
#[allow(clippy::too_many_arguments)]
async fn run_daemon_mode(
    client: Arc<PutioApiClient>,
    download_config: DownloadConfig,
    filter: Option<String>,
    all: bool,
    files: Vec<i64>,
    interval: u64,
    workers_count: usize,
    delete_after_download: bool,
    delete_empty_folders: bool,
    sort_order: SortOrder,
    reverse_sort: bool,
    progress_style: ProgressStyle,
) -> Result<()> {
    info!("Starting daemon mode with interval: {} seconds", interval);

    // Set up Ctrl+C handler
    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    ctrlc::set_handler(move || {
        info!("Received interrupt signal, shutting down daemon gracefully...");
        shutdown_clone.store(true, std::sync::atomic::Ordering::SeqCst);
    })
    .context("Failed to set Ctrl+C handler")?;

    let mut cycle_count = 0;

    loop {
        cycle_count += 1;
        info!("Daemon cycle #{} starting...", cycle_count);

        // Check for shutdown signal
        if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
            info!("Daemon shutting down gracefully...");
            break;
        }

        // Run single download cycle
        match run_single_download(
            client.clone(),
            download_config.clone(),
            filter.clone(),
            all,
            files.clone(),
            workers_count,
            delete_after_download,
            delete_empty_folders,
            sort_order.clone(),
            reverse_sort,
            progress_style.clone(),
        )
        .await
        {
            Ok(_) => {
                info!("Cycle #{} completed successfully", cycle_count);
            }
            Err(e) => {
                error!("Cycle #{} failed: {}", cycle_count, e);
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

// Note: apply_filters and apply_sorting functions removed as they're now handled
// by DownloadOps::run_sync_cycle for unified behavior between daemon and direct download modes

/// List all files recursively from Put.io
async fn list_all_files(client: &PutioApiClient, parent_id: i64) -> Result<Vec<FileEntry>> {
    let mut all_files = Vec::new();
    let mut folders_to_process = vec![parent_id];

    while let Some(current_folder) = folders_to_process.pop() {
        debug!("Fetching files from folder: {}", current_folder);

        let response = client
            .list_files(current_folder)
            .await
            .with_context(|| format!("Failed to list folder {}", current_folder))?;

        for file in response.files {
            if file.file_type == "FOLDER" {
                folders_to_process.push(file.id);
            }
            all_files.push(file);
        }
    }

    Ok(all_files)
}

/// Cleanup empty directories recursively
fn cleanup_empty_directories(base_path: &PathBuf) -> Result<()> {
    use std::fs;

    fn is_empty_dir(path: &PathBuf) -> bool {
        if let Ok(entries) = fs::read_dir(path) {
            entries.count() == 0
        } else {
            false
        }
    }

    fn remove_empty_dirs_recursive(path: &PathBuf) -> Result<()> {
        if path.is_dir() {
            // Process subdirectories first
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if entry_path.is_dir() {
                        remove_empty_dirs_recursive(&entry_path)?;
                    }
                }
            }

            // Now check if this directory is empty and remove it
            if is_empty_dir(path) {
                info!("Removing empty directory: {}", path.display());
                fs::remove_dir(path).with_context(|| {
                    format!("Failed to remove empty directory: {}", path.display())
                })?;
            }
        }
        Ok(())
    }

    remove_empty_dirs_recursive(base_path)
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
