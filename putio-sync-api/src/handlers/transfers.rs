//! Transfer handler implementations

use crate::error::{ApiError, ApiResult};
use crate::models::transfers::{
    AddTransferRequest, CancelTransfersRequest, ClearRequest, ClearResponse, ClearedTransfer,
    ReaddResponse, RetryFailure, RetryManyResponse, StallInfo, TransferResponse,
};
use crate::models::{ListResponse, SuccessResponse};
use base64::Engine;
use putio_sync_client::PutioApiClient;
use putio_sync_core::stall::StallTracker;
use std::time::SystemTime;

/// A transfer for the API, with the tracker's stall verdict attached
fn with_stall(transfer: putio_sync_client::Transfer, tracker: &StallTracker) -> TransferResponse {
    let stall = tracker
        .get(transfer.id)
        .map(|stall| StallInfo::new(&stall, SystemTime::now()));
    TransferResponse {
        stall,
        ..TransferResponse::from(transfer)
    }
}

/// List transfers from Put.io
///
/// # Arguments
///
/// * `client` - The shared Put.io client from `AppState`
/// * `tracker` - Stall verdicts from `AppState`
///
/// # Returns
///
/// List of active transfers
pub async fn list_transfers(
    client: &PutioApiClient,
    tracker: &StallTracker,
) -> ApiResult<ListResponse<TransferResponse>> {
    // List transfers
    let transfers_response = client.list_transfers().await?;

    // Convert to response models
    let transfer_responses: Vec<TransferResponse> = transfers_response
        .transfers
        .into_iter()
        .map(|transfer| with_stall(transfer, tracker))
        .collect();

    Ok(ListResponse::new(transfer_responses))
}

/// Add a new transfer
///
/// # Arguments
///
/// * `client` - The shared Put.io client from `AppState`
/// * `request` - Add transfer request with URL or file data
///
/// # Returns
///
/// Transfer information
pub async fn add_transfer(
    client: &PutioApiClient,
    request: AddTransferRequest,
) -> ApiResult<TransferResponse> {
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
/// * `client` - The shared Put.io client from `AppState`
/// * `tracker` - Stall verdicts from `AppState`
/// * `transfer_id` - Transfer ID to retrieve
///
/// # Returns
///
/// Transfer information
pub async fn get_transfer(
    client: &PutioApiClient,
    tracker: &StallTracker,
    transfer_id: i64,
) -> ApiResult<TransferResponse> {
    // Get transfer
    let transfer_response = client.get_transfer_info(transfer_id).await?;

    Ok(with_stall(transfer_response.transfer, tracker))
}

/// Cancel a transfer and add its magnet again to the same folder (feature 007)
///
/// Put.io refuses a second transfer for a torrent it already has, so the cancel comes
/// first. If the add then fails the error says the old transfer is gone and carries the
/// magnet in `details`, so the operator can add it by hand.
///
/// # Arguments
///
/// * `client` - The shared Put.io client from `AppState`
/// * `transfer_id` - Transfer ID to re-add
///
/// # Returns
///
/// The cancelled ID and the new transfer
pub async fn readd_transfer(client: &PutioApiClient, transfer_id: i64) -> ApiResult<ReaddResponse> {
    let old = client.get_transfer_info(transfer_id).await?.transfer;
    let magnet = old.magnet().map(str::to_string).ok_or_else(|| {
        ApiError::BadRequest(format!(
            "Transfer {} has no magnet link to re-add",
            transfer_id
        ))
    })?;

    client.cancel_transfers(vec![transfer_id]).await?;

    let added = client
        .add_transfer_url(&magnet, Some(old.save_parent_id), None)
        .await
        .map_err(|e| {
            tracing::warn!(
                transfer_id,
                name = %old.name,
                error = %e,
                "Re-add: cancelled but couldn't add again"
            );
            ApiError::Internal(format!(
                "Cancelled transfer {} ({}) but couldn't add it again: {}",
                transfer_id, old.name, e
            ))
            .with_details(magnet.clone())
        })?;

    tracing::info!(
        transfer_id,
        name = %old.name,
        new_transfer_id = added.id,
        save_parent_id = old.save_parent_id,
        "Re-added transfer"
    );
    Ok(ReaddResponse {
        cancelled_id: transfer_id,
        transfer: TransferResponse::from(added),
    })
}

/// Cancel transfers
///
/// # Arguments
///
/// * `client` - The shared Put.io client from `AppState`
/// * `request` - Cancel transfers request with transfer IDs
///
/// # Returns
///
/// Success response
pub async fn cancel_transfers(
    client: &PutioApiClient,
    request: CancelTransfersRequest,
) -> ApiResult<SuccessResponse> {
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
/// * `client` - The shared Put.io client from `AppState`
/// * `transfer_id` - Transfer ID to remove
///
/// # Returns
///
/// Success response
pub async fn remove_transfer(
    client: &PutioApiClient,
    transfer_id: i64,
) -> ApiResult<SuccessResponse> {
    // Use cancel_transfers with a single transfer ID
    let request = CancelTransfersRequest {
        transfer_ids: vec![transfer_id],
    };
    cancel_transfers(client, request).await
}

/// Remove several transfers with one Put.io cancel (feature 008)
///
/// # Arguments
///
/// * `client` - The shared Put.io client from `AppState`
/// * `transfer_ids` - Transfers to remove, already deduplicated
///
/// # Returns
///
/// Success response with the count
pub async fn remove_transfers(
    client: &PutioApiClient,
    transfer_ids: Vec<i64>,
) -> ApiResult<SuccessResponse> {
    let count = transfer_ids.len();
    client.cancel_transfers(transfer_ids).await?;
    tracing::info!(count, "Removed transfers");
    Ok(SuccessResponse {
        message: format!("Removed {} transfer(s)", count),
    })
}

/// Retry several transfers, one Put.io call each (Put.io has no multi-id retry); a
/// refusal is reported for that id and the rest still run (feature 008)
///
/// # Arguments
///
/// * `client` - The shared Put.io client from `AppState`
/// * `transfer_ids` - Transfers to retry, already deduplicated
///
/// # Returns
///
/// The ids Put.io accepted and those it refused, with why
pub async fn retry_transfers(
    client: &PutioApiClient,
    transfer_ids: Vec<i64>,
) -> ApiResult<RetryManyResponse> {
    let mut response = RetryManyResponse::default();
    for id in transfer_ids {
        match client.retry_transfer(id).await {
            Ok(()) => response.retried.push(id),
            Err(e) => response.failed.push(RetryFailure {
                id,
                error: e.to_string(),
            }),
        }
    }
    tracing::info!(
        retried = response.retried.len(),
        failed = response.failed.len(),
        "Retried transfers"
    );
    Ok(response)
}

/// Clear the transfers in a status group from the Put.io transfer list (feature 008)
///
/// Lists the transfers, keeps those in the group, then (unless `dry_run`) cleans them with
/// one call and cancels any the clean left in place with one more (research R3).
///
/// # Arguments
///
/// * `client` - The shared Put.io client from `AppState`
/// * `request` - Status group and `dry_run`
///
/// # Returns
///
/// The transfers cleared (or that would be)
pub async fn clear_transfers(
    client: &PutioApiClient,
    request: ClearRequest,
) -> ApiResult<ClearResponse> {
    let matching: Vec<ClearedTransfer> = client
        .list_transfers()
        .await?
        .transfers
        .into_iter()
        .filter(|t| request.status.matches(&t.status))
        .map(|t| ClearedTransfer {
            id: t.id,
            name: t.name,
            status: t.status,
        })
        .collect();

    if !request.dry_run && !matching.is_empty() {
        let ids: Vec<i64> = matching.iter().map(|t| t.id).collect();
        let cleaned = client.clean_transfers(ids.clone()).await?;
        let left: Vec<i64> = ids.into_iter().filter(|id| !cleaned.contains(id)).collect();
        let cancelled = left.len();
        if !left.is_empty() {
            client.cancel_transfers(left).await?;
        }
        tracing::info!(
            group = ?request.status,
            count = matching.len(),
            cancelled,
            "Cleared transfers"
        );
    }

    Ok(ClearResponse {
        status: request.status,
        dry_run: request.dry_run,
        count: matching.len(),
        transfers: matching,
    })
}

/// Retry a failed transfer
///
/// # Arguments
///
/// * `client` - The shared Put.io client from `AppState`
/// * `transfer_id` - Transfer ID to retry
///
/// # Returns
///
/// Transfer information
pub async fn retry_transfer(
    client: &PutioApiClient,
    transfer_id: i64,
) -> ApiResult<crate::models::transfers::TransferResponse> {
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
