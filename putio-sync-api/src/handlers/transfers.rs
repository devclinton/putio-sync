//! Transfer handler implementations

use crate::error::{ApiError, ApiResult};
use crate::models::transfers::{AddTransferRequest, CancelTransfersRequest, TransferResponse};
use crate::models::{ListResponse, SuccessResponse};
use putio_sync_core::api::ApiClient;
use putio_sync_core::config::Config;

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

    // Create API client
    let client = ApiClient::from_auth_config(auth.clone());

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

    // Create API client
    let client = ApiClient::from_auth_config(auth.clone());

    // Add transfer based on what's provided
    let transfer = if let Some(url) = request.url {
        // Add via URL (magnet link or torrent URL)
        client.add_transfer_url(&url, Some(request.parent_id), None).await?
    } else if let Some(_file_data) = request.file_data {
        // TODO: Implement file upload
        // For now, return an error
        return Err(ApiError::BadRequest(
            "File upload not yet implemented".to_string(),
        ));
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

    // Create API client
    let client = ApiClient::from_auth_config(auth.clone());

    // Get transfer
    let transfer_response = client.get_transfer(transfer_id).await?;

    Ok(crate::models::transfers::TransferResponse::from(transfer_response.transfer))
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

    // Create API client
    let client = ApiClient::from_auth_config(auth.clone());

    // Cancel transfers
    client.cancel_transfers(request.transfer_ids.clone()).await?;

    Ok(SuccessResponse {
        message: format!(
            "Successfully cancelled {} transfer(s)",
            request.transfer_ids.len()
        ),
    })
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
pub async fn retry_transfer(config: &Config, transfer_id: i64) -> ApiResult<crate::models::transfers::TransferResponse> {
    // Get auth config
    let auth = config
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::Unauthorized("No authentication configured".to_string()))?;

    // Create API client
    let client = ApiClient::from_auth_config(auth.clone());

    // Retry transfer
    client.retry_transfer(transfer_id).await?;

    // Get the transfer after retrying
    let transfer_response = client.get_transfer(transfer_id).await?;

    Ok(crate::models::transfers::TransferResponse::from(transfer_response.transfer))
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
