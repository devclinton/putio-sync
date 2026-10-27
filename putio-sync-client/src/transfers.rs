//! Transfer (torrent) API endpoints

use super::client::PutioApiClient;
use super::models::{ApiResponse, Transfer, TransferResponse, TransfersListResponse};
use crate::error::ClientResult;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Request body for adding a transfer from URL
#[derive(Debug, Serialize)]
struct AddTransferUrlRequest {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    save_parent_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    callback_url: Option<String>,
}

/// Response for adding a transfer
#[derive(Debug, Deserialize)]
struct AddTransferResponse {
    transfer: Transfer,
}

impl PutioApiClient {
    /// Add a new transfer from a URL (magnet link or torrent URL)
    ///
    /// # Arguments
    ///
    /// * `url` - Magnet link or torrent file URL
    /// * `parent_id` - Optional parent folder ID (None uses user's default folder)
    /// * `callback_url` - Optional callback URL for completion notification
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_client::PutioApiClient;
    /// # async fn example() {
    /// let client = PutioApiClient::new("my_token".to_string());
    ///
    /// // Add transfer with magnet link
    /// let magnet = "magnet:?xt=urn:btih:...";
    /// let transfer = client.add_transfer_url(magnet, None, None).await.unwrap();
    /// println!("Transfer added: {} (ID: {})", transfer.name, transfer.id);
    ///
    /// // Add transfer to specific folder
    /// let transfer = client.add_transfer_url(magnet, Some(12345), None).await.unwrap();
    /// # }
    /// ```
    pub async fn add_transfer_url(
        &self,
        url: &str,
        parent_id: Option<i64>,
        callback_url: Option<String>,
    ) -> ClientResult<Transfer> {
        let path = "/transfers/add";
        let body = AddTransferUrlRequest {
            url: url.to_string(),
            save_parent_id: parent_id,
            callback_url,
        };

        let response: ApiResponse<AddTransferResponse> = self.post(path, &body).await?;
        Ok(response.data.transfer)
    }

    /// Add a new transfer from a .torrent file
    ///
    /// This method uploads a .torrent file to Put.io using the files upload API.
    /// Put.io will automatically interpret the uploaded .torrent file as a transfer.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Path to the .torrent file
    /// * `parent_id` - Optional parent folder ID (None uses user's default folder)
    /// * `callback_url` - Optional callback URL for completion notification (currently ignored)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_client::PutioApiClient;
    /// # use std::path::Path;
    /// # async fn example() {
    /// let client = PutioApiClient::new("my_token".to_string());
    ///
    /// // Add transfer from .torrent file
    /// let file_path = Path::new("/path/to/file.torrent");
    /// let transfer = client.add_transfer_file(file_path, None, None).await.unwrap();
    /// println!("Transfer added: {} (ID: {})", transfer.name, transfer.id);
    /// # }
    /// ```
    pub async fn add_transfer_file(
        &self,
        file_path: &Path,
        parent_id: Option<i64>,
        callback_url: Option<String>,
    ) -> ClientResult<Transfer> {
        // Note: callback_url is currently ignored as the files upload API doesn't support it
        if callback_url.is_some() {
            tracing::warn!("callback_url parameter is not supported by the files upload API and will be ignored");
        }

        // Use the files upload API instead of transfers/add for .torrent files
        let upload_response = self.upload_file(file_path, parent_id).await?;

        // Extract the transfer information
        match upload_response.transfer {
            Some(transfer) => Ok(transfer),
            None => Err(crate::error::ClientError::ApiError(
                "File was uploaded but no transfer was created. The file might not be a valid torrent.".to_string()
            )),
        }
    }

    /// Add a transfer from .torrent bytes, uploaded as `filename` (which should end in
    /// `.torrent`) into `parent_id`
    pub async fn add_transfer_torrent(
        &self,
        filename: &str,
        bytes: Vec<u8>,
        parent_id: Option<i64>,
    ) -> ClientResult<Transfer> {
        let upload_response = self.upload_bytes(filename, bytes, parent_id).await?;
        upload_response.transfer.ok_or_else(|| {
            crate::error::ClientError::ApiError(
                "File was uploaded but no transfer was created. The file might not be a valid torrent.".to_string(),
            )
        })
    }

    /// List all transfers
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_client::PutioApiClient;
    /// # async fn example() {
    /// let client = PutioApiClient::new("my_token".to_string());
    /// let transfers = client.list_transfers().await.unwrap();
    ///
    /// for transfer in transfers.transfers {
    ///     println!("{}: {}% complete", transfer.name, transfer.percent_done.unwrap_or(0));
    /// }
    /// # }
    /// ```
    pub async fn list_transfers(&self) -> ClientResult<TransfersListResponse> {
        let path = "/transfers/list";
        let response: ApiResponse<TransfersListResponse> = self.get(path).await?;
        Ok(response.data)
    }

    /// Get details for a specific transfer
    ///
    /// # Arguments
    ///
    /// * `id` - Transfer ID
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_client::PutioApiClient;
    /// # async fn example() {
    /// let client = PutioApiClient::new("my_token".to_string());
    /// let transfer = client.get_transfer_info(12345).await.unwrap();
    /// println!("Transfer: {} ({}%)", transfer.transfer.name, transfer.transfer.percent_done.unwrap_or(0));
    /// # }
    /// ```
    pub async fn get_transfer_info(&self, id: i64) -> ClientResult<TransferResponse> {
        let path = format!("/transfers/{}", id);
        let response: ApiResponse<TransferResponse> = self.get(&path).await?;
        Ok(response.data)
    }

    /// Cancel one or more transfers
    ///
    /// This stops the specified transfers and removes them from the transfer list.
    ///
    /// # Arguments
    ///
    /// * `transfer_ids` - Vector of transfer IDs to cancel
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_client::PutioApiClient;
    /// # async fn example() {
    /// let client = PutioApiClient::new("my_token".to_string());
    /// client.cancel_transfers(vec![12345, 67890]).await.unwrap();
    /// println!("Transfers cancelled");
    /// # }
    /// ```
    pub async fn cancel_transfers(&self, transfer_ids: Vec<i64>) -> ClientResult<()> {
        #[derive(serde::Serialize)]
        struct CancelRequest {
            transfer_ids: String,
        }

        let ids_str = transfer_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");

        let request = CancelRequest {
            transfer_ids: ids_str,
        };

        let _response: ApiResponse<serde_json::Value> =
            self.post("/transfers/cancel", &request).await?;
        Ok(())
    }

    /// Clean finished transfers from the transfer list (their files stay in the account)
    ///
    /// Put.io answers with the ids it removed; an id it left in place (not finished, or
    /// already gone) is missing from the result. When the answer has no `deleted_ids`,
    /// every id sent counts as removed. Does nothing for an empty list, since Put.io
    /// would then clean every finished transfer.
    ///
    /// # Arguments
    ///
    /// * `transfer_ids` - Transfers to clean
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_client::PutioApiClient;
    /// # async fn example() {
    /// let client = PutioApiClient::new("my_token".to_string());
    /// let removed = client.clean_transfers(vec![12345, 67890]).await.unwrap();
    /// println!("Cleaned {} transfer(s)", removed.len());
    /// # }
    /// ```
    pub async fn clean_transfers(&self, transfer_ids: Vec<i64>) -> ClientResult<Vec<i64>> {
        #[derive(serde::Serialize)]
        struct CleanRequest {
            transfer_ids: String,
        }

        #[derive(Deserialize)]
        struct CleanResponse {
            deleted_ids: Option<Vec<i64>>,
        }

        if transfer_ids.is_empty() {
            return Ok(Vec::new());
        }

        let request = CleanRequest {
            transfer_ids: transfer_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(","),
        };

        let response: ApiResponse<CleanResponse> = self.post("/transfers/clean", &request).await?;
        Ok(response.data.deleted_ids.unwrap_or(transfer_ids))
    }

    /// Retry a failed transfer
    ///
    /// This retries a transfer that has failed. The transfer must be in a failed state.
    ///
    /// # Arguments
    ///
    /// * `transfer_id` - ID of the transfer to retry
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_client::PutioApiClient;
    /// # async fn example() {
    /// let client = PutioApiClient::new("my_token".to_string());
    /// client.retry_transfer(12345).await.unwrap();
    /// println!("Transfer retried");
    /// # }
    /// ```
    pub async fn retry_transfer(&self, transfer_id: i64) -> ClientResult<()> {
        #[derive(serde::Serialize)]
        struct RetryRequest {
            id: i64,
        }

        let request = RetryRequest { id: transfer_id };

        let _response: ApiResponse<serde_json::Value> =
            self.post("/transfers/retry", &request).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::PutioApiClient as ApiClient;
    use mockito::Server;

    #[tokio::test]
    async fn test_list_transfers() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/transfers/list")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "status": "OK",
                    "transfers": [
                        {
                            "id": 1,
                            "name": "test.torrent",
                            "size": 1073741824,
                            "status": "DOWNLOADING",
                            "down_speed": 1048576,
                            "up_speed": 0,
                            "downloaded": 536870912,
                            "uploaded": 0,
                            "percent_done": 50,
                            "estimated_time": 512,
                            "peers": 10,
                            "availability": 100,
                            "current_ratio": 0.0,
                            "seconds_seeding": 0,
                            "error_message": null,
                            "created_at": "2025-11-15T10:00:00Z",
                            "finished_at": null,
                            "save_parent_id": 0,
                            "file_id": null,
                            "is_private": false
                        }
                    ]
                }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let result = client.list_transfers().await;

        assert!(result.is_ok());
        let transfers = result.unwrap();
        assert_eq!(transfers.transfers.len(), 1);
        assert_eq!(transfers.transfers[0].name, "test.torrent");
        assert_eq!(transfers.transfers[0].percent_done, Some(50));

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_transfer() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/transfers/12345")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "status": "OK",
                    "transfer": {
                        "id": 12345,
                        "name": "movie.torrent",
                        "size": 5368709120,
                        "status": "COMPLETED",
                        "down_speed": 0,
                        "up_speed": 0,
                        "downloaded": 5368709120,
                        "uploaded": 1073741824,
                        "percent_done": 100,
                        "estimated_time": 0,
                        "peers": 0,
                        "availability": 100,
                        "current_ratio": 0.2,
                        "seconds_seeding": 3600,
                        "error_message": null,
                        "created_at": "2025-11-15T10:00:00Z",
                        "finished_at": "2025-11-15T11:00:00Z",
                        "save_parent_id": 0,
                        "file_id": 67890,
                        "is_private": false
                    }
                }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let result = client.get_transfer_info(12345).await;

        assert!(result.is_ok());
        let transfer = result.unwrap();
        assert_eq!(transfer.transfer.id, 12345);
        assert_eq!(transfer.transfer.name, "movie.torrent");
        assert_eq!(transfer.transfer.percent_done, Some(100));
        assert_eq!(transfer.transfer.status, "COMPLETED");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_add_transfer_url() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/transfers/add")
            .match_header("authorization", "Bearer test_token")
            .match_header("content-type", "application/json")
            .match_body(mockito::Matcher::JsonString(
                r#"{"url":"magnet:?xt=urn:btih:test"}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "status": "OK",
                    "transfer": {
                        "id": 999,
                        "name": "New Transfer",
                        "size": 0,
                        "status": "WAITING",
                        "down_speed": 0,
                        "up_speed": 0,
                        "downloaded": 0,
                        "uploaded": 0,
                        "percent_done": 0,
                        "estimated_time": 0,
                        "peers": 0,
                        "availability": 0,
                        "current_ratio": 0.0,
                        "seconds_seeding": 0,
                        "error_message": null,
                        "created_at": "2025-11-15T12:00:00Z",
                        "finished_at": null,
                        "save_parent_id": 0,
                        "file_id": null,
                        "is_private": false
                    }
                }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let result = client
            .add_transfer_url("magnet:?xt=urn:btih:test", None, None)
            .await;

        assert!(result.is_ok());
        let transfer = result.unwrap();
        assert_eq!(transfer.id, 999);
        assert_eq!(transfer.name, "New Transfer");
        assert_eq!(transfer.status, "WAITING");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_add_transfer_url_with_parent() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/transfers/add")
            .match_header("authorization", "Bearer test_token")
            .match_header("content-type", "application/json")
            .match_body(mockito::Matcher::JsonString(
                r#"{"url":"magnet:?xt=urn:btih:test","save_parent_id":12345}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "status": "OK",
                    "transfer": {
                        "id": 1000,
                        "name": "New Transfer in Folder",
                        "size": 0,
                        "status": "WAITING",
                        "down_speed": 0,
                        "up_speed": 0,
                        "downloaded": 0,
                        "uploaded": 0,
                        "percent_done": 0,
                        "estimated_time": 0,
                        "peers": 0,
                        "availability": 0,
                        "current_ratio": 0.0,
                        "seconds_seeding": 0,
                        "error_message": null,
                        "created_at": "2025-11-15T12:00:00Z",
                        "finished_at": null,
                        "save_parent_id": 12345,
                        "file_id": null,
                        "is_private": false
                    }
                }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let result = client
            .add_transfer_url("magnet:?xt=urn:btih:test", Some(12345), None)
            .await;

        assert!(result.is_ok());
        let transfer = result.unwrap();
        assert_eq!(transfer.id, 1000);
        assert_eq!(transfer.save_parent_id, 12345);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_retry_transfer() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/transfers/retry")
            .match_header("authorization", "Bearer test_token")
            .match_body(mockito::Matcher::JsonString(r#"{"id":42}"#.to_string()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status": "OK", "transfer": {"id": 42}}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        client.retry_transfer(42).await.unwrap();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_cancel_transfers() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/transfers/cancel")
            .match_body(mockito::Matcher::JsonString(
                r#"{"transfer_ids":"1,2"}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status": "OK"}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        client.cancel_transfers(vec![1, 2]).await.unwrap();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_clean_transfers_returns_deleted_ids() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/transfers/clean")
            .match_header("authorization", "Bearer test_token")
            .match_body(mockito::Matcher::JsonString(
                r#"{"transfer_ids":"7,8,9"}"#.to_string(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status": "OK", "deleted_ids": [7, 9]}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let removed = client.clean_transfers(vec![7, 8, 9]).await.unwrap();

        assert_eq!(removed, vec![7, 9]);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_clean_transfers_without_deleted_ids_counts_all() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/transfers/clean")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status": "OK"}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let removed = client.clean_transfers(vec![3, 4]).await.unwrap();

        assert_eq!(removed, vec![3, 4]);
        mock.assert_async().await;
    }

    /// An empty list would make Put.io clean every finished transfer, so nothing is sent
    #[tokio::test]
    async fn test_clean_transfers_empty_sends_nothing() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", mockito::Matcher::Any)
            .expect(0)
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let removed = client.clean_transfers(vec![]).await.unwrap();

        assert!(removed.is_empty());
        mock.assert_async().await;
    }
}
