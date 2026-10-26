//! File handler implementations

use crate::error::ApiResult;
use crate::models::files::{DeleteFilesRequest, FileResponse, ListFilesRequest};
use crate::models::{ListResponse, SuccessResponse};
use putio_sync_client::PutioApiClient;

/// List files from Put.io
///
/// # Arguments
///
/// * `client` - The shared Put.io client from `AppState`
/// * `request` - List files request parameters
///
/// # Returns
///
/// List of files matching the request criteria
pub async fn list_files(
    client: &PutioApiClient,
    request: ListFilesRequest,
) -> ApiResult<ListResponse<FileResponse>> {
    // Determine which parent ID to use
    let parent_id = if let Some(path) = &request.path {
        // Resolve path to ID
        client.resolve_path(path).await?
    } else {
        request.parent_id
    };

    // List files
    let files_response = client.list_files(parent_id).await?;

    // Convert to response models
    let file_responses: Vec<FileResponse> = files_response
        .files
        .into_iter()
        .map(FileResponse::from)
        .collect();

    Ok(ListResponse::new(file_responses))
}

/// Get a single file by ID
///
/// # Arguments
///
/// * `client` - The shared Put.io client from `AppState`
/// * `file_id` - File ID to retrieve
///
/// # Returns
///
/// File information
pub async fn get_file(client: &PutioApiClient, file_id: i64) -> ApiResult<FileResponse> {
    // Get file
    let file_response = client.get_file_details(file_id).await?;

    Ok(FileResponse::from(file_response.file))
}

/// Delete files from Put.io
///
/// # Arguments
///
/// * `client` - The shared Put.io client from `AppState`
/// * `request` - Delete files request with file IDs
///
/// # Returns
///
/// Success response
pub async fn delete_files(
    client: &PutioApiClient,
    request: DeleteFilesRequest,
) -> ApiResult<SuccessResponse> {
    // Delete files
    client.delete_files(request.file_ids.clone()).await?;

    Ok(SuccessResponse {
        message: format!("Successfully deleted {} file(s)", request.file_ids.len()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_files_request_construction() {
        let request = ListFilesRequest {
            parent_id: 123,
            path: None,
            filter: Some("*.txt".to_string()),
            recursive: true,
        };

        assert_eq!(request.parent_id, 123);
        assert!(request.recursive);
        assert_eq!(request.filter, Some("*.txt".to_string()));
    }

    #[test]
    fn test_delete_files_request_construction() {
        let request = DeleteFilesRequest {
            file_ids: vec![1, 2, 3],
        };

        assert_eq!(request.file_ids.len(), 3);
    }
}
