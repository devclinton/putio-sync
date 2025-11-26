//! Shared download operations for both standalone download and daemon commands

use anyhow::{Context, Result};
use putio_sync_core::api::{ApiClient, FileEntry};
use putio_sync_core::config::DownloadConfig;
use putio_sync_core::db::Database;
use putio_sync_core::download::{DownloadFile, DownloadManager};
use putio_sync_core::filter::{sort_files, FileFilter, SortField};
use std::sync::Arc;
use std::time::Instant;
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
    database: Arc<Database>,
    config: DownloadConfig,
    sync_parent_id: i64,
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

        // Initialize database
        let db_path = config.target_directory.join(".putiosync.db");
        let database = Arc::new(
            Database::new(&db_path)
                .await
                .context("Failed to initialize database")?,
        );

        Ok(Self {
            client,
            database,
            config,
            sync_parent_id,
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

        debug!("Starting download sync cycle...");

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
        let file_entries: Vec<FileEntry> = file_entries_with_path.iter().map(|f| f.entry.clone()).collect();
        let mut filtered_files = self.apply_filters(file_entries, filter)?;
        filtered_files = self.apply_sorting(filtered_files)?;

        // Create a lookup map for relative paths
        let path_map: std::collections::HashMap<i64, String> = file_entries_with_path
            .iter()
            .map(|f| (f.entry.id, f.relative_path.clone()))
            .collect();

        // Only process files (not folders), but preserve path information
        let download_files: Vec<DownloadFile> = filtered_files
            .iter()
            .filter(|f| !f.is_folder)
            .map(|f| DownloadFile {
                file_id: f.id,
                filename: f.name.clone(),
                relative_path: path_map.get(&f.id).cloned().unwrap_or_default(),
                file_size: f.size.max(0) as u64,
                download_url: self.client.get_download_url(f.id),
            })
            .collect();

        result.files_found = download_files.len();

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

        // Create download manager
        let manager = DownloadManager::new(
            self.client.clone(),
            self.database.clone(),
            self.config.clone(),
        );

        // Download files
        for download_file in download_files {
            let callback = Some(progress_display.clone().create_callback());

            info!("🔽 Starting download: {} ({})", download_file.filename, format_size(download_file.file_size));

            // Validate that the file still exists and is actually a file (not folder) before attempting download
            debug!("Validating file {} still exists on Put.io", download_file.file_id);
            match self.client.get_file(download_file.file_id).await {
                Ok(file_response) => {
                    // Check if it's actually a folder
                    if file_response.file.is_folder {
                        warn!("Skipping folder (ID {}): {} - cannot download directories", download_file.file_id, file_response.file.name);
                        error!("❌ Download failed for {}: Item is a folder, not a downloadable file", download_file.filename);
                        result.files_failed += 1;
                        continue;
                    }
                    // File exists and is actually a file, proceed with download
                    debug!("File validation successful: {} (size: {} bytes)", file_response.file.name, file_response.file.size);
                }
                Err(e) => {
                    warn!("File {} no longer exists on Put.io: {}", download_file.file_id, e);
                    error!("❌ Download failed for file {}: File no longer exists on Put.io", download_file.filename);
                    result.files_failed += 1;
                    continue;
                }
            }

            match manager
                .download_file(download_file.clone(), callback)
                .await
            {
                Ok(download_result) => {
                    if download_result.success {
                        result.files_downloaded += 1;
                        result.total_bytes += download_result.bytes_downloaded;
                        
                        info!("✅ Download completed: {} ({} downloaded)", 
                              download_file.filename, 
                              format_size(download_result.bytes_downloaded));
                        
                        // Optional: Delete from Put.io if configured
                        if self.config.delete_after_download {
                            debug!("Deleting file from Put.io: {}", download_file.filename);
                            if let Err(e) = self.client.delete_files(vec![download_file.file_id]).await {
                                warn!("Failed to delete file {} from Put.io: {}", download_file.filename, e);
                            } else {
                                info!("🗑️  Successfully deleted {} from Put.io (delete_after_download enabled)", download_file.filename);
                                
                                // Check if we need to clean up empty directories on Put.io
                                if !download_file.relative_path.is_empty() {
                                    if let Err(e) = self.cleanup_empty_directories_after_deletion(&download_file).await {
                                        warn!("Failed to cleanup empty directories: {}", e);
                                    }
                                }
                            }
                        }
                    } else {
                        result.files_failed += 1;
                        error!(
                            "❌ Download failed for file {}: {}",
                            download_file.filename,
                            download_result.error.unwrap_or_else(|| "Unknown error".to_string())
                        );
                    }
                }
                Err(e) => {
                    result.files_failed += 1;
                    error!("❌ Download failed for file {}: {}", download_file.filename, e);
                }
            }
        }

        // Finish progress display
        progress_display.finish();

        result.duration_secs = start_time.elapsed().as_secs_f64();

        debug!(
            "Download sync cycle completed - found: {}, downloaded: {}, failed: {}, duration: {:.1}s",
            result.files_found,
            result.files_downloaded,
            result.files_failed,
            result.duration_secs
        );

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

            for entry in response.files {
                if entry.is_folder {
                    // For folders, recursively get their contents
                    let folder_path = if current_path.is_empty() {
                        entry.name.clone()
                    } else {
                        format!("{}/{}", current_path, entry.name)
                    };
                    
                    let mut subfolder_files = self.get_files_recursive(entry.id, folder_path.clone()).await?;
                    all_files.append(&mut subfolder_files);
                } else {
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
        if download_file.relative_path.is_empty() {
            // File is in root directory, no cleanup needed
            return Ok(());
        }

        // Parse the relative_path to get directory hierarchy
        let path_components: Vec<&str> = download_file.relative_path.split('/').collect();
        
        // Start from the deepest directory and work our way up
        for i in (0..path_components.len()).rev() {
            let current_path = if i == 0 {
                path_components[0].to_string()
            } else {
                path_components[0..=i].join("/")
            };
            
            debug!("Checking if directory is empty: {}", current_path);
            
            // Get the directory ID by searching for it
            if let Ok(dir_id) = self.find_directory_id(&current_path).await {
                // Check if the directory is empty
                match self.client.list_files(dir_id).await {
                    Ok(response) => {
                        if response.files.is_empty() {
                            info!("🗑️  Deleting empty directory from Put.io: {}", current_path);
                            if let Err(e) = self.client.delete_files(vec![dir_id]).await {
                                warn!("Failed to delete empty directory '{}' from Put.io: {}", current_path, e);
                                break; // Stop cleanup on first failure
                            } else {
                                debug!("Successfully deleted empty directory: {}", current_path);
                            }
                        } else {
                            debug!("Directory is not empty, stopping cleanup: {}", current_path);
                            break; // Directory not empty, stop cleanup
                        }
                    }
                    Err(e) => {
                        warn!("Failed to check directory contents for '{}': {}", current_path, e);
                        break; // Stop on error
                    }
                }
            } else {
                debug!("Could not find directory ID for path: {}", current_path);
                break; // If we can't find the directory, stop cleanup
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