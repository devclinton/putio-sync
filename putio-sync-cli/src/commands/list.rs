//! List command implementation
//!
//! Handles listing files and folders from Put.io with filtering, sorting,
//! and multiple output formats.

use anyhow::{anyhow, Context, Result};
use putio_sync_client::{FileEntry, PutioApiClient};
use putio_sync_core::config::{AuthMethod, Config};
use putio_sync_core::filter::{parse_size, sort_files, FileFilter, FileTypeFilter, SortField};
use std::path::PathBuf;
use std::str::FromStr;
use tracing::{debug, info, warn};

use crate::output::{format_and_output, OutputFormat};

/// Execute the list command
#[allow(clippy::too_many_arguments)]
pub async fn execute_list(
    path: Option<String>,
    id: Option<i64>,
    recursive: bool,
    _long: bool, // Reserved for future use
    filter: Option<String>,
    file_type: Option<String>,
    min_size: Option<String>,
    max_size: Option<String>,
    sort: Option<String>,
    reverse: bool,
    output: String,
    output_file: Option<PathBuf>,
    config_path: Option<PathBuf>,
) -> Result<()> {
    // Load configuration to get auth token
    let config_file_path = config_path
        .or_else(|| Config::default_path().ok())
        .ok_or_else(|| anyhow!("Could not determine config file path"))?;

    let config = Config::load(&config_file_path)
        .context("Failed to load configuration. Have you run 'putio-sync login'?")?;

    let auth = config.auth.ok_or_else(|| {
        anyhow!("No authentication credentials found. Please run 'putio-sync login' first.")
    })?;

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

    // Determine folder ID to list
    let folder_id = determine_folder_id(&client, path, id).await?;

    info!("Listing files in folder ID: {}", folder_id);

    // Fetch files from API
    let response = client
        .list_files(folder_id)
        .await
        .context("Failed to fetch file list from Put.io")?;

    let mut files = response.files;

    // Handle recursive listing
    if recursive {
        files = list_recursive(&client, folder_id).await?;
    }

    debug!("Retrieved {} files before filtering", files.len());

    // Apply filtering
    files = apply_filters(files, filter, file_type, min_size, max_size)?;

    debug!("Filtered to {} files", files.len());

    // Apply sorting
    files = apply_sorting(files, sort, reverse)?;

    // Format and output
    let output_format = OutputFormat::from_str(&output)
        .map_err(|e| anyhow!("Invalid output format '{}': {}", output, e))?;

    format_and_output(&files, output_format, output_file.as_deref())
        .context("Failed to write output")?;

    Ok(())
}

/// Determine the folder ID to list
async fn determine_folder_id(
    client: &PutioApiClient,
    path: Option<String>,
    id: Option<i64>,
) -> Result<i64> {
    match (path, id) {
        (Some(p), None) => {
            info!("Resolving path: {}", p);
            client
                .resolve_path(&p)
                .await
                .with_context(|| format!("Failed to resolve path: {}", p))
        }
        (None, Some(i)) => {
            info!("Using provided folder ID: {}", i);
            Ok(i)
        }
        (None, None) => {
            debug!("No path or ID provided, using root (0)");
            Ok(0)
        }
        (Some(_), Some(_)) => Err(anyhow!("Cannot specify both path and ID")),
    }
}

/// List files recursively
async fn list_recursive(client: &PutioApiClient, folder_id: i64) -> Result<Vec<FileEntry>> {
    let mut all_files = Vec::new();
    let mut folders_to_process = vec![folder_id];

    while let Some(current_folder) = folders_to_process.pop() {
        debug!("Processing folder: {}", current_folder);

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

/// Apply filters to file list
fn apply_filters(
    files: Vec<FileEntry>,
    filter: Option<String>,
    file_type: Option<String>,
    min_size: Option<String>,
    max_size: Option<String>,
) -> Result<Vec<FileEntry>> {
    let mut file_filter = FileFilter::new();

    if let Some(pattern) = filter {
        debug!("Applying name filter: {}", pattern);
        file_filter = file_filter.with_name_pattern(pattern);
    }

    if let Some(ft) = file_type {
        let type_filter = FileTypeFilter::from_str(&ft)
            .map_err(|e| anyhow!("Invalid file type '{}': {}", ft, e))?;
        debug!("Applying file type filter: {:?}", type_filter);
        file_filter = file_filter.with_file_type(type_filter);
    }

    if let Some(min) = min_size {
        let size = parse_size(&min).map_err(|e| anyhow!("Invalid min size '{}': {}", min, e))?;
        debug!("Applying min size filter: {} bytes", size);
        file_filter = file_filter.with_min_size(size);
    }

    if let Some(max) = max_size {
        let size = parse_size(&max).map_err(|e| anyhow!("Invalid max size '{}': {}", max, e))?;
        debug!("Applying max size filter: {} bytes", size);
        file_filter = file_filter.with_max_size(size);
    }

    Ok(file_filter.filter(files))
}

/// Apply sorting to file list
fn apply_sorting(
    files: Vec<FileEntry>,
    sort: Option<String>,
    reverse: bool,
) -> Result<Vec<FileEntry>> {
    if let Some(sort_field_str) = sort {
        let sort_field = SortField::from_str(&sort_field_str)
            .map_err(|e| anyhow!("Invalid sort field '{}': {}", sort_field_str, e))?;
        debug!("Sorting by: {:?}, reverse: {}", sort_field, reverse);
        Ok(sort_files(files, sort_field, reverse))
    } else if reverse {
        warn!("--reverse specified without --sort, ignoring");
        Ok(files)
    } else {
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_file(id: i64, name: &str, size: i64, file_type: &str) -> FileEntry {
        FileEntry {
            id,
            name: name.to_string(),
            size,
            file_type: file_type.to_string(),
            parent_id: Some(0),
            content_type: None,
            created_at: Some("2025-11-06T10:00:00Z".to_string()),
            first_accessed_at: None,
            is_folder: file_type == "FOLDER",
            is_shared: false,
            screenshot: None,
            opensubtitles_hash: None,
            crc32: None,
            extension: Some("mp4".to_string()),
            sort_by: None,
        }
    }

    #[test]
    fn test_apply_filters_name_pattern() {
        let files = vec![
            create_test_file(1, "movie.mp4", 1000, "VIDEO"),
            create_test_file(2, "doc.pdf", 100, "FILE"),
        ];

        let filtered = apply_filters(files, Some("*.mp4".to_string()), None, None, None).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "movie.mp4");
    }

    #[test]
    fn test_apply_filters_file_type() {
        let files = vec![
            create_test_file(1, "folder", 0, "FOLDER"),
            create_test_file(2, "file.txt", 100, "FILE"),
        ];

        let filtered = apply_filters(files, None, Some("folder".to_string()), None, None).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "folder");
    }

    #[test]
    fn test_apply_filters_size() {
        let files = vec![
            create_test_file(1, "small.txt", 100, "FILE"),
            create_test_file(2, "medium.txt", 1000, "FILE"),
            create_test_file(3, "large.txt", 10000, "FILE"),
        ];

        let filtered = apply_filters(
            files,
            None,
            None,
            Some("500".to_string()),
            Some("5000".to_string()),
        )
        .unwrap();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "medium.txt");
    }

    #[test]
    fn test_apply_sorting() {
        let files = vec![
            create_test_file(1, "zebra.txt", 300, "FILE"),
            create_test_file(2, "apple.txt", 100, "FILE"),
            create_test_file(3, "banana.txt", 200, "FILE"),
        ];

        let sorted = apply_sorting(files, Some("name".to_string()), false).unwrap();
        assert_eq!(sorted[0].name, "apple.txt");
        assert_eq!(sorted[1].name, "banana.txt");
        assert_eq!(sorted[2].name, "zebra.txt");
    }

    #[test]
    fn test_apply_sorting_reverse() {
        let files = vec![
            create_test_file(1, "a.txt", 100, "FILE"),
            create_test_file(2, "b.txt", 200, "FILE"),
        ];

        let sorted = apply_sorting(files, Some("name".to_string()), true).unwrap();
        assert_eq!(sorted[0].name, "b.txt");
        assert_eq!(sorted[1].name, "a.txt");
    }
}
