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

        // Get list of files to download
        let file_entries = if all {
            // Get all files from root folder
            let response = self.client
                .list_files(0)
                .await
                .context("Failed to fetch file list from Put.io")?;
            response.files
        } else if !specific_files.is_empty() {
            // Get specific files by ID
            let mut entries = Vec::new();
            for &file_id in specific_files {
                match self.client.get_file(file_id).await {
                    Ok(file_response) => entries.push(file_response.file),
                    Err(e) => {
                        warn!("Failed to get file {}: {}", file_id, e);
                    }
                }
            }
            entries
        } else {
            // For daemon mode, get all files from sync.remote_parent_id
            let response = self.client
                .list_files(self.sync_parent_id)
                .await
                .context("Failed to fetch file list from Put.io")?;
            response.files
        };

        // Apply filters
        let mut filtered_files = self.apply_filters(file_entries, filter)?;

        // Apply sorting
        filtered_files = self.apply_sorting(filtered_files)?;

        // Filter out folders
        let download_files: Vec<DownloadFile> = filtered_files
            .iter()
            .filter(|f| !f.is_folder)
            .map(|f| DownloadFile {
                file_id: f.id,
                filename: f.name.clone(),
                file_size: f.size.max(0) as u64,
                download_url: format!("https://api.put.io/v2/files/{}/download", f.id),
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

        // Create progress display if needed
        let progress_display = if show_progress {
            Some(Arc::new(ProgressDisplay::new(
                self.config.progress_style.clone(),
                download_files.len() as u64,
                total_size,
            )))
        } else {
            None
        };

        // Create download manager
        let manager = DownloadManager::new(
            self.client.clone(),
            self.database.clone(),
            self.config.clone(),
        );

        // Download files
        for download_file in download_files {
            let callback = progress_display.as_ref().map(|pd| pd.clone().create_callback());

            match manager
                .download_file(download_file.clone(), callback)
                .await
            {
                Ok(download_result) => {
                    if download_result.success {
                        result.files_downloaded += 1;
                        result.total_bytes += download_result.bytes_downloaded;
                    } else {
                        result.files_failed += 1;
                        error!(
                            "Download failed for file {}: {}",
                            download_file.file_id,
                            download_result.error.unwrap_or_else(|| "Unknown error".to_string())
                        );
                    }
                }
                Err(e) => {
                    result.files_failed += 1;
                    error!("Download failed for file {}: {}", download_file.file_id, e);
                }
            }
        }

        // Finish progress display
        if let Some(ref progress) = progress_display {
            progress.finish();
        }

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