//! Transfer handler implementations

use crate::error::{ApiError, ApiResult};
use crate::models::transfers::{AddTransferRequest, CancelTransfersRequest, TransferResponse};
use crate::models::{ListResponse, SuccessResponse};
use base64::Engine;
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::{AuthMethod, Config};

/// List transfers from Put.io
///
/// # Arguments
///
/// * `config` - Application configuration
///
/// # Returns
///
/// List of active transfers
pub async fn list_transfers(config: &Config) -> ApiResult<ListResponse<TransferResponse>> {
    // Get auth config
    let auth = config
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::Unauthorized("No authentication configured".to_string()))?;

    // Create API client based on auth method
    let client = match auth.auth_method {
        AuthMethod::Token => {
            let token = auth
                .token
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Token not configured".to_string()))?;
            PutioApiClient::new(token.clone())
        }
        AuthMethod::AppPassword => {
            let username = auth
                .username
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Username not configured".to_string()))?;
            let password = auth
                .password
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Password not configured".to_string()))?;
            PutioApiClient::with_credentials(username.clone(), password.clone())
        }
    };

    // List transfers
    let transfers_response = client.list_transfers().await?;

    // Convert to response models
    let transfer_responses: Vec<TransferResponse> = transfers_response
        .transfers
        .into_iter()
        .map(TransferResponse::from)
        .collect();

    Ok(ListResponse::new(transfer_responses))
}

/// Add a new transfer
///
/// # Arguments
///
/// * `config` - Application configuration
/// * `request` - Add transfer request with URL or file data
///
/// # Returns
///
/// Transfer information
pub async fn add_transfer(
    config: &Config,
    request: AddTransferRequest,
) -> ApiResult<TransferResponse> {
    // Get auth config
    let auth = config
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::Unauthorized("No authentication configured".to_string()))?;

    // Create API client based on auth method
    let client = match auth.auth_method {
        AuthMethod::Token => {
            let token = auth
                .token
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Token not configured".to_string()))?;
            PutioApiClient::new(token.clone())
        }
        AuthMethod::AppPassword => {
            let username = auth
                .username
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Username not configured".to_string()))?;
            let password = auth
                .password
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Password not configured".to_string()))?;
            PutioApiClient::with_credentials(username.clone(), password.clone())
        }
    };

    // Add transfer based on what's provided
    let transfer = if let Some(url) = request.url {
        // Add via URL (magnet link or torrent URL)
        client
            .add_transfer_url(&url, Some(request.parent_id), None)
            .await?
    } else if let Some(file_data) = request.file_data {
        // Decode base64 file data
        let file_bytes = base64::engine::general_purpose::STANDARD
            .decode(&file_data)
            .map_err(|e| ApiError::BadRequest(format!("Invalid base64 file data: {}", e)))?;

        // Write to temporary file
        let temp_dir = std::env::temp_dir();
        let temp_file_path = temp_dir.join(format!("upload_{}.torrent", uuid::Uuid::new_v4()));

        std::fs::write(&temp_file_path, file_bytes).map_err(|e| {
            ApiError::InternalError(format!("Failed to write temporary file: {}", e))
        })?;

        // Upload the file
        let result = client
            .add_transfer_file(&temp_file_path, Some(request.parent_id), None)
            .await;

        // Clean up temporary file
        let _ = std::fs::remove_file(&temp_file_path);

        result?
    } else {
        return Err(ApiError::BadRequest(
            "Either url or file_data must be provided".to_string(),
        ));
    };

    Ok(crate::models::transfers::TransferResponse::from(transfer))
}

/// Get a single transfer by ID
///
/// # Arguments
///
/// * `config` - Application configuration
/// * `transfer_id` - Transfer ID to retrieve
///
/// # Returns
///
/// Transfer information
pub async fn get_transfer(config: &Config, transfer_id: i64) -> ApiResult<TransferResponse> {
    // Get auth config
    let auth = config
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::Unauthorized("No authentication configured".to_string()))?;

    // Create API client based on auth method
    let client = match auth.auth_method {
        AuthMethod::Token => {
            let token = auth
                .token
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Token not configured".to_string()))?;
            PutioApiClient::new(token.clone())
        }
        AuthMethod::AppPassword => {
            let username = auth
                .username
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Username not configured".to_string()))?;
            let password = auth
                .password
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Password not configured".to_string()))?;
            PutioApiClient::with_credentials(username.clone(), password.clone())
        }
    };

    // Get transfer
    let transfer_response = client.get_transfer_info(transfer_id).await?;

    Ok(crate::models::transfers::TransferResponse::from(
        transfer_response.transfer,
    ))
}

/// Cancel transfers
///
/// # Arguments
///
/// * `config` - Application configuration
/// * `request` - Cancel transfers request with transfer IDs
///
/// # Returns
///
/// Success response
pub async fn cancel_transfers(
    config: &Config,
    request: CancelTransfersRequest,
) -> ApiResult<SuccessResponse> {
    // Get auth config
    let auth = config
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::Unauthorized("No authentication configured".to_string()))?;

    // Create API client based on auth method
    let client = match auth.auth_method {
        AuthMethod::Token => {
            let token = auth
                .token
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Token not configured".to_string()))?;
            PutioApiClient::new(token.clone())
        }
        AuthMethod::AppPassword => {
            let username = auth
                .username
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Username not configured".to_string()))?;
            let password = auth
                .password
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Password not configured".to_string()))?;
            PutioApiClient::with_credentials(username.clone(), password.clone())
        }
    };

    // Cancel transfers
    client
        .cancel_transfers(request.transfer_ids.clone())
        .await?;

    Ok(SuccessResponse {
        message: format!(
            "Successfully cancelled {} transfer(s)",
            request.transfer_ids.len()
        ),
    })
}

/// Remove a single transfer
///
/// # Arguments
///
/// * `config` - Application configuration
/// * `transfer_id` - Transfer ID to remove
///
/// # Returns
///
/// Success response
pub async fn remove_transfer(config: &Config, transfer_id: i64) -> ApiResult<SuccessResponse> {
    // Use cancel_transfers with a single transfer ID
    let request = CancelTransfersRequest {
        transfer_ids: vec![transfer_id],
    };
    cancel_transfers(config, request).await
}

/// Retry a failed transfer
///
/// # Arguments
///
/// * `config` - Application configuration
/// * `transfer_id` - Transfer ID to retry
///
/// # Returns
///
/// Transfer information
pub async fn retry_transfer(
    config: &Config,
    transfer_id: i64,
) -> ApiResult<crate::models::transfers::TransferResponse> {
    // Get auth config
    let auth = config
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::Unauthorized("No authentication configured".to_string()))?;

    // Create API client based on auth method
    let client = match auth.auth_method {
        AuthMethod::Token => {
            let token = auth
                .token
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Token not configured".to_string()))?;
            PutioApiClient::new(token.clone())
        }
        AuthMethod::AppPassword => {
            let username = auth
                .username
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Username not configured".to_string()))?;
            let password = auth
                .password
                .as_ref()
                .ok_or_else(|| ApiError::Unauthorized("Password not configured".to_string()))?;
            PutioApiClient::with_credentials(username.clone(), password.clone())
        }
    };

    // Retry transfer
    client.retry_transfer(transfer_id).await?;

    // Get the transfer after retrying
    let transfer_response = client.get_transfer_info(transfer_id).await?;

    Ok(crate::models::transfers::TransferResponse::from(
        transfer_response.transfer,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_transfer_request_construction() {
        let request = AddTransferRequest {
            url: Some("magnet:?xt=urn:btih:test".to_string()),
            file_data: None,
            parent_id: 123,
        };

        assert_eq!(request.url, Some("magnet:?xt=urn:btih:test".to_string()));
        assert_eq!(request.parent_id, 123);
    }

    #[test]
    fn test_cancel_transfers_request_construction() {
        let request = CancelTransfersRequest {
            transfer_ids: vec![1, 2, 3],
        };

        assert_eq!(request.transfer_ids.len(), 3);
    }
}
