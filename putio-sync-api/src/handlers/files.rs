//! File handler implementations

use crate::error::{ApiError, ApiResult};
use crate::models::files::{DeleteFilesRequest, FileResponse, ListFilesRequest};
use crate::models::{ListResponse, SuccessResponse};
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::{AuthMethod, Config};

/// List files from Put.io
///
/// # Arguments
///
/// * `config` - Application configuration
/// * `request` - List files request parameters
///
/// # Returns
///
/// List of files matching the request criteria
pub async fn list_files(
    config: &Config,
    request: ListFilesRequest,
) -> ApiResult<ListResponse<FileResponse>> {
    // Get auth config
    let auth = config
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::Unauthorized("No authentication configured".to_string()))?;

    // Create API client based on auth method
    let client = match auth.auth_method {
        AuthMethod::Token => {
            let token = auth.token.as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Token not configured".to_string()))?;
            PutioApiClient::new(token.clone())
        }
        AuthMethod::AppPassword => {
            let username = auth.username.as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Username not configured".to_string()))?;
            let password = auth.password.as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Password not configured".to_string()))?;
            PutioApiClient::with_credentials(username.clone(), password.clone())
        }
    };

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
/// * `config` - Application configuration
/// * `file_id` - File ID to retrieve
///
/// # Returns
///
/// File information
pub async fn get_file(config: &Config, file_id: i64) -> ApiResult<FileResponse> {
    // Get auth config
    let auth = config
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::Unauthorized("No authentication configured".to_string()))?;

    // Create API client based on auth method
    let client = match auth.auth_method {
        AuthMethod::Token => {
            let token = auth.token.as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Token not configured".to_string()))?;
            PutioApiClient::new(token.clone())
        }
        AuthMethod::AppPassword => {
            let username = auth.username.as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Username not configured".to_string()))?;
            let password = auth.password.as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Password not configured".to_string()))?;
            PutioApiClient::with_credentials(username.clone(), password.clone())
        }
    };

    // Get file
    let file_response = client.get_file_details(file_id).await?;

    Ok(FileResponse::from(file_response.file))
}

/// Delete files from Put.io
///
/// # Arguments
///
/// * `config` - Application configuration
/// * `request` - Delete files request with file IDs
///
/// # Returns
///
/// Success response
pub async fn delete_files(
    config: &Config,
    request: DeleteFilesRequest,
) -> ApiResult<SuccessResponse> {
    // Get auth config
    let auth = config
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::Unauthorized("No authentication configured".to_string()))?;

    // Create API client based on auth method
    let client = match auth.auth_method {
        AuthMethod::Token => {
            let token = auth.token.as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Token not configured".to_string()))?;
            PutioApiClient::new(token.clone())
        }
        AuthMethod::AppPassword => {
            let username = auth.username.as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Username not configured".to_string()))?;
            let password = auth.password.as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Password not configured".to_string()))?;
            PutioApiClient::with_credentials(username.clone(), password.clone())
        }
    };

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
