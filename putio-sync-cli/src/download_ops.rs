//! Shared download operations for both standalone download and daemon commands

use anyhow::{Context, Result};
use futures::stream::{FuturesUnordered, StreamExt};
use putio_sync_core::api::{ApiClient, FileEntry};
use putio_sync_core::config::DownloadConfig;
use putio_sync_core::db::Database;
use putio_sync_core::download::{DownloadFile, DownloadManager};
use putio_sync_core::filter::{sort_files, FileFilter, SortField};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

use crate::ui::ProgressDisplay;

/// FileEntry with additional path information for recursive downloads
#[derive(Debug, Clone)]
struct FileEntryWithPath {
    pub entry: FileEntry,
    pub relative_path: String,
}

/// Result of a download cycle
#[derive(Debug, Clone)]
pub struct DownloadCycleResult {
    pub files_found: usize,
    pub files_downloaded: usize,
    pub files_failed: usize,
    pub total_bytes: u64,
    pub duration_secs: f64,
}

/// Tracks empty directories for cleanup
#[derive(Debug, Clone)]
struct EmptyDirectoryTracker {
    /// Directory path -> (directory_id, first_seen_time, scan_count)
    empty_directories: HashMap<String, (i64, Instant, u32)>,
    /// Number of scans before deletion (default: 5)
    scan_threshold: u32,
}

impl EmptyDirectoryTracker {
    fn new() -> Self {
        Self {
            empty_directories: HashMap::new(),
            scan_threshold: 5,
        }
    }

    /// Mark a directory as empty, tracking how many times we've seen it
    fn mark_empty(&mut self, path: String, directory_id: i64) -> bool {
        let current_time = Instant::now();
        
        match self.empty_directories.get_mut(&path) {
            Some((_, _first_seen, count)) => {
                *count += 1;
                info!("Empty directory '{}' seen {} times (threshold: {})", path, count, self.scan_threshold);
                *count >= self.scan_threshold
            }
            None => {
                info!("First time seeing empty directory: {} (will delete after {} more scans)", path, self.scan_threshold - 1);
                self.empty_directories.insert(path, (directory_id, current_time, 1));
                false
            }
        }
    }

    /// Remove a directory from tracking (if it's no longer empty)
    fn mark_not_empty(&mut self, path: &str) {
        if self.empty_directories.remove(path).is_some() {
            debug!("Directory '{}' is no longer empty, removed from tracking", path);
        }
    }

    /// Get directories that should be deleted
    fn get_directories_for_deletion(&self) -> Vec<(String, i64)> {
        self.empty_directories
            .iter()
            .filter(|(_, (_, _, count))| *count >= self.scan_threshold)
            .map(|(path, (id, _, _))| (path.clone(), *id))
            .collect()
    }

    /// Clean up old entries to prevent memory growth
    fn cleanup_old_entries(&mut self) {
        let cutoff_time = Instant::now() - Duration::from_secs(3600); // 1 hour
        let before_count = self.empty_directories.len();
        self.empty_directories.retain(|_, (_, first_seen, _)| *first_seen > cutoff_time);
        let after_count = self.empty_directories.len();
        if before_count != after_count {
            debug!("Cleaned up {} old empty directory entries (kept {})", before_count - after_count, after_count);
        }
    }

    /// Get current tracking status for debugging
    fn get_tracking_status(&self) -> String {
        if self.empty_directories.is_empty() {
            "No directories being tracked".to_string()
        } else {
            let mut status = Vec::new();
            for (path, (_, _, count)) in &self.empty_directories {
                status.push(format!("{}: {}/{} scans", path, count, self.scan_threshold));
            }
            format!("Tracking {} directories: [{}]", self.empty_directories.len(), status.join(", "))
        }
    }
}

impl Default for DownloadCycleResult {
    fn default() -> Self {
        Self::new()
    }
}

impl DownloadCycleResult {
    pub fn new() -> Self {
        Self {
            files_found: 0,
            files_downloaded: 0,
            files_failed: 0,
            total_bytes: 0,
            duration_secs: 0.0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.files_found == 0
    }
}

/// Shared download operations that can be used by both download command and daemon
pub struct DownloadOps {
    client: Arc<ApiClient>,
    database: Option<Arc<Database>>,
    config: DownloadConfig,
    sync_parent_id: i64,
    empty_directory_tracker: Arc<tokio::sync::Mutex<EmptyDirectoryTracker>>,
}

impl DownloadOps {
    /// Create new download operations instance
    pub async fn new(
        client: Arc<ApiClient>,
        config: DownloadConfig,
        sync_parent_id: i64,
    ) -> Result<Self> {
        info!("DownloadOps::new - config.target_directory: {}", config.target_directory.display());
        // Create target directory if it doesn't exist
        if !config.target_directory.exists() {
            info!(
                "Creating target directory: {}",
                config.target_directory.display()
            );
            std::fs::create_dir_all(&config.target_directory).with_context(|| {
                format!(
                    "Failed to create target directory: {}",
                    config.target_directory.display()
                )
            })?;
        }

        // Initialize database - use fallback if it fails
        let db_path = config.target_directory.join(".putiosync.db");
        let database = match Database::new(&db_path).await {
            Ok(db) => {
                info!("Database initialized successfully at: {}", db_path.display());
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

        Ok(Self {
            client,
            database,
            config,
            sync_parent_id,
            empty_directory_tracker: Arc::new(tokio::sync::Mutex::new(EmptyDirectoryTracker::new())),
        })
    }

    /// Run a single download sync cycle
    pub async fn run_sync_cycle(
        &self,
        filter: Option<&str>,
        all: bool,
        specific_files: &[i64],
        show_progress: bool,
    ) -> Result<DownloadCycleResult> {
        let start_time = Instant::now();
        let mut result = DownloadCycleResult::new();

        info!("🔄 Starting download sync cycle - delete_empty_folders: {}, sync_parent_id: {}", 
              self.config.delete_empty_folders, self.sync_parent_id);
        debug!("Sync cycle parameters: all={}, filter={:?}, specific_files={:?}", all, filter, specific_files);

        // Get list of files to download (recursive)
        let file_entries_with_path = if all {
            // Get all files from root folder recursively
            self.get_files_recursive(0, "".to_string()).await
                .context("Failed to fetch file list from Put.io")?
        } else if !specific_files.is_empty() {
            // Get specific files by ID (non-recursive for specific files)
            let mut entries = Vec::new();
            for &file_id in specific_files {
                match self.client.get_file(file_id).await {
                    Ok(file_response) => {
                        // Skip folders when explicitly specified by ID
                        if file_response.file.is_folder {
                            warn!("Skipping folder ID {}: {} - cannot download directories directly", file_id, file_response.file.name);
                            continue;
                        }
                        entries.push(FileEntryWithPath {
                            entry: file_response.file,
                            relative_path: "".to_string(),
                        });
                    }
                    Err(e) => {
                        warn!("Failed to get file {}: {}", file_id, e);
                    }
                }
            }
            entries
        } else {
            // For daemon mode, get all files from sync.remote_parent_id recursively
            self.get_files_recursive(self.sync_parent_id, "".to_string()).await
                .context("Failed to fetch file list from Put.io")?
        };

        // Apply filters and sorting to the entries (extracting the FileEntry part)
        debug!("🔍 Converting {} FileEntryWithPath entries to FileEntry:", file_entries_with_path.len());
        let file_entries: Vec<FileEntry> = file_entries_with_path
            .iter()
            .map(|f| {
                debug!("📋 Converting: '{}' (ID: {}, is_folder: {}, file_type: '{}')", 
                       f.entry.name, f.entry.id, f.entry.is_folder, f.entry.file_type);
                f.entry.clone()
            })
            .collect();
        
        debug!("🔍 After conversion, checking {} FileEntry entries:", file_entries.len());
        for (i, entry) in file_entries.iter().enumerate() {
            debug!("📋 Entry {}: '{}' (ID: {}, is_folder: {}, file_type: '{}')", 
                   i + 1, entry.name, entry.id, entry.is_folder, entry.file_type);
        }
        
        let mut filtered_files = self.apply_filters(file_entries, filter)?;
        
        debug!("🔍 After apply_filters, checking {} entries:", filtered_files.len());
        for (i, entry) in filtered_files.iter().enumerate() {
            debug!("📋 Filtered entry {}: '{}' (ID: {}, is_folder: {}, file_type: '{}')", 
                   i + 1, entry.name, entry.id, entry.is_folder, entry.file_type);
        }
        
        filtered_files = self.apply_sorting(filtered_files)?;
        
        debug!("🔍 After apply_sorting, checking {} entries:", filtered_files.len());
        for (i, entry) in filtered_files.iter().enumerate() {
            debug!("📋 Sorted entry {}: '{}' (ID: {}, is_folder: {}, file_type: '{}')", 
                   i + 1, entry.name, entry.id, entry.is_folder, entry.file_type);
        }
        
        // Safety check: ensure all entries with file_type "FOLDER" have is_folder = true
        debug!("🔧 Applying safety check for folder flag consistency...");
        for entry in &mut filtered_files {
            if entry.file_type == "FOLDER" && !entry.is_folder {
                warn!("🛠️  Fixed inconsistent folder flag for '{}' (ID: {}): file_type='FOLDER' but is_folder=false", 
                      entry.name, entry.id);
                entry.is_folder = true;
            }
        }
        
        debug!("🔍 After safety check, verifying {} entries:", filtered_files.len());
        for (i, entry) in filtered_files.iter().enumerate() {
            debug!("📋 Final entry {}: '{}' (ID: {}, is_folder: {}, file_type: '{}')", 
                   i + 1, entry.name, entry.id, entry.is_folder, entry.file_type);
        }

        // Create a lookup map for relative paths
        let path_map: std::collections::HashMap<i64, String> = file_entries_with_path
            .iter()
            .map(|f| (f.entry.id, f.relative_path.clone()))
            .collect();

        // Only process files (not folders), but preserve path information
        debug!("🔍 Filtering {} entries for download:", filtered_files.len());
        for (i, f) in filtered_files.iter().enumerate() {
            debug!("  Entry {}: '{}' (ID: {}, is_folder: {}, file_type: '{}')", 
                   i + 1, f.name, f.id, f.is_folder, f.file_type);
        }
        
        let download_files: Vec<DownloadFile> = filtered_files
            .iter()
            .filter(|f| {
                let is_file = !f.is_folder;
                debug!("📋 Entry '{}' (ID: {}): is_folder={}, file_type='{}' → include_in_downloads={}", 
                       f.name, f.id, f.is_folder, f.file_type, is_file);
                is_file
            })
            .map(|f| DownloadFile {
                file_id: f.id,
                filename: f.name.clone(),
                relative_path: path_map.get(&f.id).cloned().unwrap_or_default(),
                file_size: f.size.max(0) as u64,
                download_url: self.client.get_download_url(f.id),
            })
            .collect();

        result.files_found = download_files.len();

        // Always run empty directory cleanup if enabled, regardless of whether there are files to download
        if self.config.delete_empty_folders {
            info!("Starting empty directory scan (delete_empty_folders is enabled)");
            if let Err(e) = self.scan_and_track_empty_directories().await {
                warn!("Failed to scan for empty directories: {}", e);
            }

            // Delete directories that have been empty for the threshold number of scans
            if let Err(e) = self.cleanup_persistently_empty_directories().await {
                warn!("Failed to cleanup persistently empty directories: {}", e);
            }
            info!("Empty directory scan completed");
        } else {
            debug!("Empty directory cleanup is disabled (delete_empty_folders = false)");
        }

        if download_files.is_empty() {
            debug!("No files to download in this cycle");
            result.duration_secs = start_time.elapsed().as_secs_f64();
            return Ok(result);
        }

        info!("Found {} files to download", download_files.len());

        // Calculate total size
        let total_size: u64 = download_files.iter().map(|f| f.file_size).sum();

        debug!(
            "Total download size: {} ({} bytes)",
            format_size(total_size),
            total_size
        );

        // Create progress display - always create it for proper completion handling,
        // but only show it if show_progress is true
        let progress_display = Arc::new(ProgressDisplay::new(
            if show_progress {
                self.config.progress_style.clone()
            } else {
                putio_sync_core::config::ProgressStyle::None // Hide progress but still handle completion
            },
            download_files.len() as u64,
            total_size,
        ));

        // Create download manager - handle missing database gracefully
        let manager = match &self.database {
            Some(db) => {
                Arc::new(DownloadManager::new(
                    self.client.clone(),
                    db.clone(),
                    self.config.clone(),
                ))
            }
            None => {
                // Create a memory-only download manager without database
                warn!("Creating download manager without database - no download tracking will be available");
                Arc::new(DownloadManager::new_without_database(
                    self.client.clone(),
                    self.config.clone(),
                ))
            }
        };

        // Download files in parallel using worker pool
        
        let max_workers = self.config.workers.max(1); // Ensure at least 1 worker
        let semaphore = Arc::new(Semaphore::new(max_workers));
        info!("Starting parallel downloads with {} workers", max_workers);
        
        let mut futures = FuturesUnordered::new();
        
        for download_file in download_files {
            let manager = manager.clone();
            let client = self.client.clone();
            let progress_display = progress_display.clone();
            let config = self.config.clone();
            let semaphore = semaphore.clone();
            
            let future = async move {
                // Acquire semaphore permit for worker slot
                let _permit = semaphore.acquire().await.expect("Semaphore closed");
                
                let callback = Some(progress_display.create_callback());
                info!("🔽 Starting download: {} ({})", download_file.filename, format_size(download_file.file_size));

                // Validate that the file still exists and is actually a file (not folder) before attempting download
                debug!("Validating file {} still exists on Put.io", download_file.file_id);
                match client.get_file(download_file.file_id).await {
                    Ok(file_response) => {
                        debug!("File validation response: {:?}", file_response);
                        debug!("File is a folder: {}", file_response.file.is_folder);
                        debug!("File Type: {}", file_response.file.file_type);
                        // Check if it's actually a folder
                        if file_response.file.is_folder {
                            warn!("Skipping folder (ID {}): {} - cannot download directories", download_file.file_id, file_response.file.name);
                            error!("❌ Download failed for {}: Item is a folder, not a downloadable file", download_file.filename);
                            return (false, 0u64, download_file);
                        }
                        // File exists and is actually a file, proceed with download
                        debug!("File validation successful: {} (size: {} bytes)", file_response.file.name, file_response.file.size);
                    }
                    Err(e) => {
                        warn!("File {} no longer exists on Put.io: {}", download_file.file_id, e);
                        error!("❌ Download failed for file {}: File no longer exists on Put.io", download_file.filename);
                        return (false, 0u64, download_file);
                    }
                }

                match manager.download_file(download_file.clone(), callback).await {
                    Ok(download_result) => {
                        if download_result.success {
                            info!("✅ Download completed: {} ({})", 
                                  download_file.filename, 
                                  format_size(download_result.bytes_downloaded));
                            
                            // Optional: Delete from Put.io if configured
                            if config.delete_after_download {
                                debug!("Deleting file from Put.io: {}", download_file.filename);
                                if let Err(e) = client.delete_files(vec![download_file.file_id]).await {
                                    warn!("Failed to delete file {} from Put.io: {}", download_file.filename, e);
                                } else {
                                    info!("🗑️  Successfully deleted {} from Put.io (delete_after_download enabled)", download_file.filename);
                                }
                            }
                        } else {
                            error!(
                                "❌ Download failed for file {}: {}",
                                download_file.filename,
                                download_result.error.unwrap_or_else(|| "Unknown error".to_string())
                            );
                        }
                        
                        (download_result.success, download_result.bytes_downloaded, download_file)
                    }
                    Err(e) => {
                        error!("❌ Download failed for file {}: {}", download_file.filename, e);
                        (false, 0u64, download_file)
                    }
                }
            };
            
            futures.push(future);
        }
        
        // Process results as they complete and collect files for cleanup
        let mut files_needing_cleanup = Vec::new();
        
        while let Some((success, bytes_downloaded, download_file)) = futures.next().await {
            if success {
                result.files_downloaded += 1;
                result.total_bytes += bytes_downloaded;
                
                // Collect files that were deleted and might need directory cleanup
                if self.config.delete_after_download && !download_file.relative_path.is_empty() {
                    files_needing_cleanup.push(download_file);
                }
            } else {
                result.files_failed += 1;
            }
        }
        
        // Cleanup empty directories after all downloads complete (if enabled)
        if self.config.delete_empty_folders && !files_needing_cleanup.is_empty() {
            info!("Cleaning up {} potentially empty directories after file deletions", files_needing_cleanup.len());
            for download_file in files_needing_cleanup {
                if let Err(e) = self.cleanup_empty_directories_after_deletion(&download_file).await {
                    warn!("Failed to cleanup empty directories for {}: {}", download_file.filename, e);
                }
            }
            info!("Immediate empty directory cleanup completed");
        }

        // Note: Empty directory scanning and cleanup now happens earlier in the cycle 
        // (before downloads) to ensure it runs even when there are no files to download

        // Finish progress display
        progress_display.finish();

        result.duration_secs = start_time.elapsed().as_secs_f64();

        // Generate a comprehensive summary
        info!(
            "📊 Download sync cycle completed - found: {}, downloaded: {}, failed: {}, duration: {:.1}s",
            result.files_found,
            result.files_downloaded,
            result.files_failed,
            result.duration_secs
        );
        
        if self.config.delete_empty_folders {
            let tracker_status = {
                let tracker = self.empty_directory_tracker.lock().await;
                tracker.get_tracking_status()
            };
            info!("🗂️  Empty directory tracking summary: {}", tracker_status);
        }

        Ok(result)
    }

    /// Apply filters to file list
    fn apply_filters(&self, files: Vec<FileEntry>, filter: Option<&str>) -> Result<Vec<FileEntry>> {
        let mut filtered = files;

        // Apply glob pattern filter if specified
        if let Some(pattern) = filter {
            let file_filter = FileFilter::new().with_name_pattern(pattern.to_string());
            filtered.retain(|f| file_filter.matches(f));
        }

        Ok(filtered)
    }

    /// Apply sorting to file list
    fn apply_sorting(&self, files: Vec<FileEntry>) -> Result<Vec<FileEntry>> {
        let sort_field = match self.config.sort_order {
            putio_sync_core::config::SortOrder::ByDate => SortField::Date,
            putio_sync_core::config::SortOrder::ByName => SortField::Name,
            putio_sync_core::config::SortOrder::ByExtension => SortField::Name, // Use Name as fallback for extension
            putio_sync_core::config::SortOrder::BySize => SortField::Size,
        };

        Ok(sort_files(files, sort_field, self.config.reverse_sort))
    }

    /// Update configuration (for hot reload)
    pub fn update_config(&mut self, new_config: DownloadConfig) {
        self.config = new_config;
    }

    /// Get current configuration
    pub fn config(&self) -> &DownloadConfig {
        &self.config
    }

    /// Recursively get all files from a directory, preserving path structure
    fn get_files_recursive(&self, parent_id: i64, current_path: String) -> std::pin::Pin<std::boxed::Box<dyn std::future::Future<Output = Result<Vec<FileEntryWithPath>>> + '_>> {
        Box::pin(async move {
            let response = self.client
                .list_files(parent_id)
                .await
                .context("Failed to fetch file list from Put.io")?;

            let mut all_files = Vec::new();

            for mut entry in response.files {
                entry.is_folder = entry.file_type == "FOLDER";
                if entry.is_folder {
                    debug!("{}<{}> Is a folder", entry.name, entry.id);
                    // For folders, recursively get their contents
                    let folder_path = if current_path.is_empty() {
                        entry.name.clone()
                    } else {
                        format!("{}/{}", current_path, entry.name)
                    };
                    
                    let mut subfolder_files = self.get_files_recursive(entry.id, folder_path.clone()).await?;
                    all_files.append(&mut subfolder_files);
                } else {
                    debug!("{} is a {}", entry.name, entry.file_type);
                    // For files, add them with the current path
                    all_files.push(FileEntryWithPath {
                        entry,
                        relative_path: current_path.clone(),
                    });
                }
            }

            Ok(all_files)
        })
    }

    /// Clean up empty directories on Put.io after file deletion
    async fn cleanup_empty_directories_after_deletion(&self, download_file: &DownloadFile) -> Result<()> {
        info!("🧹 Starting immediate empty directory cleanup for file: {} (relative_path: '{}')", 
              download_file.filename, download_file.relative_path);
              
        if download_file.relative_path.is_empty() {
            debug!("File '{}' is in root directory, no cleanup needed", download_file.filename);
            return Ok(());
        }

        // Parse the relative_path to get directory hierarchy
        let path_components: Vec<&str> = download_file.relative_path.split('/').collect();
        info!("Directory hierarchy for cleanup: {:?}", path_components);
        
        // Start from the deepest directory and work our way up
        for i in (0..path_components.len()).rev() {
            let current_path = if i == 0 {
                path_components[0].to_string()
            } else {
                path_components[0..=i].join("/")
            };
            
            info!("🔍 Checking if directory '{}' is empty for immediate cleanup", current_path);
            
            // Get the directory ID by searching for it
            match self.find_directory_id(&current_path).await {
                Ok(dir_id) => {
                    debug!("Found directory '{}' with ID: {}", current_path, dir_id);
                    // Check if the directory is empty
                    match self.client.list_files(dir_id).await {
                        Ok(response) => {
                            debug!("Directory '{}' contains {} items", current_path, response.files.len());
                            if response.files.is_empty() {
                                info!("🗑️  Deleting empty directory from Put.io: '{}'", current_path);
                                if let Err(e) = self.client.delete_files(vec![dir_id]).await {
                                    error!("❌ Failed to delete empty directory '{}' from Put.io: {}", current_path, e);
                                    break; // Stop cleanup on first failure
                                } else {
                                    info!("✅ Successfully deleted empty directory: '{}'", current_path);
                                }
                            } else {
                                info!("📂 Directory '{}' is not empty ({} items), stopping cleanup", current_path, response.files.len());
                                // Log what's in the directory
                                for (i, item) in response.files.iter().take(3).enumerate() {
                                    debug!("  Item {}: '{}' (type: {})", i + 1, item.name, item.file_type);
                                }
                                break; // Directory not empty, stop cleanup
                            }
                        }
                        Err(e) => {
                            warn!("❌ Failed to check directory contents for '{}': {}", current_path, e);
                            break; // Stop on error
                        }
                    }
                }
                Err(_) => {
                    debug!("Could not find directory ID for path: '{}' (may have already been deleted)", current_path);
                    break; // If we can't find the directory, stop cleanup
                }
            }
        }
        
        Ok(())
    }

    /// Find directory ID by path (simple implementation - could be optimized)
    async fn find_directory_id(&self, path: &str) -> Result<i64> {
        let components: Vec<&str> = path.split('/').filter(|&s| !s.is_empty()).collect();
        let mut current_parent_id = 0i64; // Start from root

        for component in components {
            let response = self.client.list_files(current_parent_id).await?;
            
            // Look for a folder with the matching name
            if let Some(folder) = response.files.iter().find(|f| f.is_folder && f.name == component) {
                current_parent_id = folder.id;
            } else {
                return Err(anyhow::anyhow!("Directory not found: {}", component));
            }
        }

        Ok(current_parent_id)
    }

    /// Scan all directories and track which ones are empty
    async fn scan_and_track_empty_directories(&self) -> Result<()> {
        info!("Scanning directories for empty folders to track");
        
        // Get all directories recursively from the sync parent
        let directories = self.get_all_directories_recursive(self.sync_parent_id, "".to_string()).await?;
        info!("Found {} total directories to scan", directories.len());
        
        let mut tracker = self.empty_directory_tracker.lock().await;
        
        // Clean up old entries first
        let before_cleanup_count = tracker.empty_directories.len();
        tracker.cleanup_old_entries();
        let after_cleanup_count = tracker.empty_directories.len();
        if before_cleanup_count != after_cleanup_count {
            info!("Cleaned up {} stale directory tracking entries", before_cleanup_count - after_cleanup_count);
        }
        
        let mut empty_count = 0;
        let mut non_empty_count = 0;
        let mut ready_for_deletion = 0;
        
        for (path, dir_id) in directories {
            // Check if directory is empty
            debug!("🔍 Checking if directory '{}' (ID: {}) is empty", path, dir_id);
            match self.client.list_files(dir_id).await {
                Ok(response) => {
                    debug!("Directory '{}' contains {} items", path, response.files.len());
                    
                    if !response.files.is_empty() {
                        // Log details about what's in the directory
                        for (i, item) in response.files.iter().take(5).enumerate() {
                            debug!("  Item {}: '{}' (type: {}, is_folder: {})", i + 1, item.name, item.file_type, item.is_folder);
                        }
                        if response.files.len() > 5 {
                            debug!("  ... and {} more items", response.files.len() - 5);
                        }
                    }
                    
                    if response.files.is_empty() {
                        empty_count += 1;
                        info!("🗂️  Empty directory detected: '{}' (ID: {})", path, dir_id);
                        // Directory is empty, track it
                        let should_delete = tracker.mark_empty(path.clone(), dir_id);
                        if should_delete {
                            ready_for_deletion += 1;
                            info!("🗑️  Directory '{}' has been empty for {} scans and is ready for deletion", path, tracker.scan_threshold);
                        } else {
                            let current_count = tracker.empty_directories.get(&path).map(|(_, _, count)| *count).unwrap_or(0);
                            info!("📋 Empty directory '{}' added to tracking ({}/{} scans)", path, current_count, tracker.scan_threshold);
                        }
                    } else {
                        non_empty_count += 1;
                        // Directory is not empty, remove from tracking
                        if tracker.empty_directories.contains_key(&path) {
                            info!("📂 Directory '{}' is no longer empty, removing from tracking", path);
                        }
                        tracker.mark_not_empty(&path);
                    }
                }
                Err(e) => {
                    warn!("❌ Failed to check directory '{}': {}", path, e);
                }
            }
        }
        
        info!("Directory scan results: {} empty, {} non-empty, {} ready for deletion", 
              empty_count, non_empty_count, ready_for_deletion);
        
        let tracking_status = tracker.get_tracking_status();
        info!("Current tracking status: {}", tracking_status);
        
        Ok(())
    }

    /// Delete directories that have been empty for the required number of scans
    async fn cleanup_persistently_empty_directories(&self) -> Result<()> {
        let directories_to_delete = {
            let tracker = self.empty_directory_tracker.lock().await;
            tracker.get_directories_for_deletion()
        };

        if directories_to_delete.is_empty() {
            debug!("No directories ready for deletion");
            return Ok(());
        }

        info!("Attempting to delete {} persistently empty directories", directories_to_delete.len());

        for (path, dir_id) in directories_to_delete {
            // Double-check that the directory is still empty before deletion
            info!("🔍 Verifying directory '{}' (ID: {}) is still empty before deletion", path, dir_id);
            match self.client.list_files(dir_id).await {
                Ok(response) => {
                    debug!("Pre-deletion check: directory '{}' contains {} items", path, response.files.len());
                    
                    if response.files.is_empty() {
                        info!("✅ Confirmed empty - deleting persistently empty directory: '{}'", path);
                        match self.client.delete_files(vec![dir_id]).await {
                            Ok(_) => {
                                // Remove from tracking after successful deletion
                                let mut tracker = self.empty_directory_tracker.lock().await;
                                tracker.mark_not_empty(&path);
                                info!("🗑️  ✅ Successfully deleted empty directory: '{}'", path);
                            }
                            Err(e) => {
                                error!("❌ Failed to delete empty directory '{}': {}", path, e);
                                debug!("Delete error details: {:#}", e);
                            }
                        }
                    } else {
                        info!("📂 Directory '{}' is no longer empty (has {} items), skipping deletion", path, response.files.len());
                        // Log what's in the directory
                        for (i, item) in response.files.iter().take(3).enumerate() {
                            debug!("  Item {}: '{}' (type: {})", i + 1, item.name, item.file_type);
                        }
                        if response.files.len() > 3 {
                            debug!("  ... and {} more items", response.files.len() - 3);
                        }
                        // Remove from tracking since it's no longer empty
                        let mut tracker = self.empty_directory_tracker.lock().await;
                        tracker.mark_not_empty(&path);
                    }
                }
                Err(e) => {
                    error!("❌ Failed to verify directory '{}' before deletion: {}", path, e);
                    debug!("Verification error details: {:#}", e);
                }
            }
        }

        Ok(())
    }

    /// Get all directories recursively with their paths and IDs
    fn get_all_directories_recursive(&self, parent_id: i64, current_path: String) -> std::pin::Pin<std::boxed::Box<dyn std::future::Future<Output = Result<Vec<(String, i64)>>> + '_>> {
        Box::pin(async move {
            let mut directories = Vec::new();
            
            debug!("🔍 Starting recursive scan of directory ID {} (path: '{}')", parent_id, current_path);
            
            match self.client.list_files(parent_id).await {
                Ok(response) => {
                    info!("📋 Directory ID {} contains {} items", parent_id, response.files.len());
                    
                    let mut folder_count = 0;
                    let mut file_count = 0;
                    
                    // Log each item for detailed debugging
                    for (i, entry) in response.files.iter().enumerate() {
                        debug!("📋 Item {}: '{}' (ID: {}, type: '{}', is_folder: {})", 
                               i + 1, entry.name, entry.id, entry.file_type, entry.is_folder);
                    }
                    
                    for entry in response.files {
                        // Make sure is_folder is set correctly based on file_type
                        let mut entry = entry;
                        let original_is_folder = entry.is_folder;
                        if entry.file_type == "FOLDER" {
                            entry.is_folder = true;
                            if !original_is_folder {
                                debug!("🔧 Corrected is_folder flag for '{}' from false to true", entry.name);
                            }
                        }
                        
                        if entry.is_folder || entry.file_type == "FOLDER" {
                            folder_count += 1;
                            let dir_path = if current_path.is_empty() {
                                entry.name.clone()
                            } else {
                                format!("{}/{}", current_path, entry.name)
                            };
                            
                            info!("📂 Adding directory to list: '{}' (ID: {})", dir_path, entry.id);
                            
                            // Add this directory to the list
                            directories.push((dir_path.clone(), entry.id));
                            
                            // Recursively scan subdirectories
                            debug!("🔄 Recursively scanning subdirectory '{}' (ID: {})", entry.name, entry.id);
                            match self.get_all_directories_recursive(entry.id, dir_path).await {
                                Ok(mut sub_dirs) => {
                                    info!("✅ Subdirectory scan of '{}' returned {} directories", entry.name, sub_dirs.len());
                                    for (sub_path, sub_id) in &sub_dirs {
                                        debug!("  Subdirectory: '{}' (ID: {})", sub_path, sub_id);
                                    }
                                    directories.append(&mut sub_dirs);
                                }
                                Err(e) => {
                                    error!("❌ Failed to scan subdirectory '{}': {}", entry.name, e);
                                }
                            }
                        } else {
                            file_count += 1;
                            debug!("📄 File detected: '{}' (ID: {}, type: {})", entry.name, entry.id, entry.file_type);
                        }
                    }
                    
                    info!("✅ Directory ID {} scan complete: {} folders, {} files, {} total directories found in this branch", 
                           parent_id, folder_count, file_count, directories.len());
                }
                Err(e) => {
                    error!("❌ Failed to list files for directory ID {}: {}", parent_id, e);
                    return Err(e.into());
                }
            }
            
            debug!("🔍 Returning {} directories from scan of ID {} ('{}')", directories.len(), parent_id, current_path);
            Ok(directories)
        })
    }
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

