//! Delete file operations for Put.io API

use crate::client::PutioApiClient;
use crate::error::ClientResult;
use futures_util::future::BoxFuture;
use serde::Serialize;
use tracing::{debug, info};

/// Request body for deleting files
#[derive(Debug, Serialize)]
struct DeleteRequest {
    /// Comma-separated list of file IDs to delete
    file_ids: String,
}

impl PutioApiClient {
    /// Delete files from Put.io
    ///
    /// # Arguments
    ///
    /// * `file_ids` - Vector of file IDs to delete
    ///
    /// # Returns
    ///
    /// Result indicating success or failure
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_client::ApiClient;
    /// # async fn example() {
    /// let client = ApiClient::new("your_token".to_string());
    /// client.delete_files(vec![123, 456, 789]).await.unwrap();
    /// # }
    /// ```
    pub async fn delete_files(&self, file_ids: Vec<i64>) -> ClientResult<()> {
        if file_ids.is_empty() {
            info!("No files to delete");
            return Ok(());
        }

        let file_ids_str = file_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");

        debug!("Deleting files: {}", file_ids_str);

        let request = DeleteRequest {
            file_ids: file_ids_str,
        };

        let _response: serde_json::Value = self.post("/files/delete", &request).await?;

        info!("Successfully deleted {} file(s)", file_ids.len());
        Ok(())
    }

    /// Delete a folder and all its contents recursively
    ///
    /// This first lists all files in the folder, then deletes them all.
    ///
    /// # Arguments
    ///
    /// * `folder_id` - ID of the folder to delete
    ///
    /// # Returns
    ///
    /// Result indicating success or failure
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_client::ApiClient;
    /// # async fn example() {
    /// let client = ApiClient::new("your_token".to_string());
    /// client.delete_folder_recursive(123).await.unwrap();
    /// # }
    /// ```
    pub fn delete_folder_recursive(&self, folder_id: i64) -> BoxFuture<'_, ClientResult<()>> {
        Box::pin(async move {
            debug!("Deleting folder recursively: {}", folder_id);

            // Get folder info to check if it's actually a folder
            let file_response = self.get_file_details(folder_id).await?;
            let file = file_response.file;

            if !file.is_folder {
                // If it's a file, just delete it
                return self.delete_files(vec![folder_id]).await;
            }

            // List all contents of the folder
            let response = self.list_files(folder_id).await?;

            // Collect all file IDs (including subfolders)
            let mut all_ids = vec![];
            for file in response.files {
                if file.is_folder {
                    // Recursively delete subfolder
                    self.delete_folder_recursive(file.id).await?;
                } else {
                    all_ids.push(file.id);
                }
            }

            // Delete all files in current folder
            if !all_ids.is_empty() {
                self.delete_files(all_ids).await?;
            }

            // Finally, delete the folder itself
            self.delete_files(vec![folder_id]).await?;

            info!("Successfully deleted folder {}", folder_id);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::PutioApiClient as ApiClient;
    use mockito::Server;

    #[tokio::test]
    async fn test_delete_files() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/files/delete")
            .match_header("authorization", "Bearer test_token")
            .match_header("content-type", "application/json")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "file_ids": "123,456"
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status": "OK"}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let result = client.delete_files(vec![123, 456]).await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_files_empty() {
        let client = ApiClient::new("test_token".to_string());
        let result = client.delete_files(vec![]).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_delete_single_file() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("POST", "/files/delete")
            .match_header("authorization", "Bearer test_token")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "file_ids": "123"
            })))
            .with_status(200)
            .with_body(r#"{"status": "OK"}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let result = client.delete_files(vec![123]).await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }
}
