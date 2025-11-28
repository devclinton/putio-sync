//! File and folder API endpoints

use super::client::ApiClient;
use super::models::{ApiResponse, FileEntry, FileResponse, FilesListResponse, Transfer};
use crate::error::CoreResult;
use serde::Deserialize;
use std::path::Path;

/// Response from file upload API
#[derive(Debug, Deserialize)]
pub struct UploadResponse {
    /// File information (if uploaded file was a regular file)
    #[serde(rename = "file")]
    pub file: Option<FileEntry>,
    /// Transfer information (if uploaded file was a torrent)
    #[serde(rename = "transfer")]
    pub transfer: Option<Transfer>,
}

impl ApiClient {
    /// List files in a folder
    ///
    /// # Arguments
    ///
    /// * `parent_id` - Parent folder ID (0 for root)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_core::api::ApiClient;
    /// # async fn example() {
    /// let client = ApiClient::new("my_token".to_string());
    /// let files = client.list_files(0).await.unwrap();
    /// println!("Found {} files", files.files.len());
    /// # }
    /// ```
    pub async fn list_files(&self, parent_id: i64) -> CoreResult<FilesListResponse> {
        let path = format!("/files/list?parent_id={}", parent_id);
        let response: ApiResponse<FilesListResponse> = self.get(&path).await?;
        Ok(response.data)
    }

    /// Get details for a specific file
    ///
    /// # Arguments
    ///
    /// * `id` - File ID
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_core::api::ApiClient;
    /// # async fn example() {
    /// let client = ApiClient::new("my_token".to_string());
    /// let file = client.get_file(12345).await.unwrap();
    /// println!("File name: {}", file.file.name);
    /// # }
    /// ```
    pub async fn get_file(&self, id: i64) -> CoreResult<FileResponse> {
        let path = format!("/files/{}", id);
        let response: ApiResponse<FileResponse> = self.get(&path).await?;
        Ok(response.data)
    }

    /// Get download URL for a file
    ///
    /// Returns the direct download URL for a file.
    /// Note: This returns the URL endpoint, not the actual download stream.
    ///
    /// # Arguments
    ///
    /// * `id` - File ID
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_core::api::ApiClient;
    /// # async fn example() {
    /// let client = ApiClient::new("my_token".to_string());
    /// let url = client.get_download_url(12345);
    /// println!("Download URL: {}", url);
    /// # }
    /// ```
    pub fn get_download_url(&self, id: i64) -> String {
        format!("{}/files/{}/download", self.base_url, id)
    }

    /// Upload a file to Put.io
    ///
    /// This method uploads a file to Put.io. If the uploaded file is a torrent file,
    /// Put.io will interpret it as a transfer and the Transfer field will be present
    /// to represent the status of the transfer.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Path to the file to upload
    /// * `parent_id` - Optional parent folder ID (None uses user's default folder)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_core::api::ApiClient;
    /// # use std::path::Path;
    /// # async fn example() {
    /// let client = ApiClient::new("my_token".to_string());
    ///
    /// // Upload a torrent file
    /// let file_path = Path::new("/path/to/file.torrent");
    /// let response = client.upload_file(file_path, None).await.unwrap();
    /// 
    /// if let Some(transfer) = response.transfer {
    ///     println!("Torrent uploaded: {} (ID: {})", transfer.name, transfer.id);
    /// }
    /// # }
    /// ```
    pub async fn upload_file(
        &self,
        file_path: &Path,
        parent_id: Option<i64>,
    ) -> CoreResult<UploadResponse> {
        use reqwest::multipart::{Form, Part};
        use tokio::fs::File as TokioFile;
        use tokio::io::AsyncReadExt;

        // Read the file
        let mut file = TokioFile::open(file_path)
            .await
            .map_err(crate::error::CoreError::IoError)?;

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .await
            .map_err(crate::error::CoreError::IoError)?;

        // Get the filename
        let filename = file_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("file")
            .to_string();

        // Determine MIME type based on file extension
        let mime_type = if filename.ends_with(".torrent") {
            "application/x-bittorrent"
        } else {
            "application/octet-stream"
        };

        // Create multipart form
        let file_part = Part::bytes(buffer)
            .file_name(filename)
            .mime_str(mime_type)
            .map_err(|e| {
                crate::error::CoreError::InternalError(format!(
                    "Failed to set MIME type: {}",
                    e
                ))
            })?;

        let mut form = Form::new().part("file", file_part);

        if let Some(pid) = parent_id {
            form = form.text("parent_id", pid.to_string());
        }

        // Send the request with multipart form
        // Put.io uses a separate upload domain for file uploads
        let url = "https://upload.put.io/v2/files/upload";
        let auth_header = self.get_auth_header()?;

        let response = self
            .client
            .post(url)
            .header(reqwest::header::AUTHORIZATION, auth_header)
            .multipart(form)
            .send()
            .await
            .map_err(crate::error::CoreError::HttpError)?;

        let status = response.status();
        
        // Get response headers for debugging
        let headers = response.headers().clone();
        tracing::debug!("Upload response status: {}", status);
        tracing::debug!("Upload response headers: {:?}", headers);
        
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            tracing::error!("Upload failed with status {}: {}", status, error_text);
            return Err(crate::error::CoreError::ApiError(format!(
                "Failed to upload file: {} - {}",
                status, error_text
            )));
        }

        // Get response text first for debugging
        let response_text = response
            .text()
            .await
            .map_err(crate::error::CoreError::HttpError)?;
        
        tracing::debug!("Upload response body: {}", response_text);

        // Try to parse the JSON response
        let api_response: ApiResponse<UploadResponse> = serde_json::from_str(&response_text)
            .map_err(|e| {
                tracing::error!("Failed to parse upload response JSON: {}", e);
                tracing::error!("Raw response: {}", response_text);
                crate::error::CoreError::ApiError(format!(
                    "Failed to parse upload response: {} - Raw response: {}", 
                    e, 
                    response_text
                ))
            })?;

        Ok(api_response.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn test_list_files() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/files/list?parent_id=0")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "status": "OK",
                    "files": [
                        {
                            "id": 1,
                            "name": "test.txt",
                            "size": 1024,
                            "file_type": "FILE",
                            "parent_id": 0,
                            "is_folder": false,
                            "is_shared": false
                        }
                    ],
                    "parent": {
                        "id": 0,
                        "name": "Root",
                        "size": 0,
                        "file_type": "FOLDER",
                        "parent_id": 0,
                        "is_folder": true,
                        "is_shared": false
                    },
                    "total": 1
                }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let result = client.list_files(0).await;

        assert!(result.is_ok());
        let files = result.unwrap();
        assert_eq!(files.files.len(), 1);
        assert_eq!(files.files[0].name, "test.txt");
        assert_eq!(files.total, 1);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_file() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/files/12345")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "status": "OK",
                    "file": {
                        "id": 12345,
                        "name": "movie.mp4",
                        "size": 5242880,
                        "file_type": "VIDEO",
                        "parent_id": 0,
                        "content_type": "video/mp4",
                        "is_folder": false,
                        "is_shared": false
                    }
                }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let result = client.get_file(12345).await;

        assert!(result.is_ok());
        let file = result.unwrap();
        assert_eq!(file.file.id, 12345);
        assert_eq!(file.file.name, "movie.mp4");
        assert_eq!(file.file.file_type, "VIDEO");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_download_url() {
        let client = ApiClient::with_base_url(
            "test_token".to_string(),
            "https://api.put.io/v2".to_string(),
        );

        let url = client.get_download_url(12345);
        assert_eq!(url, "https://api.put.io/v2/files/12345/download");
    }

    #[tokio::test]
    async fn test_list_files_error() {
        let mut server = Server::new_async().await;

        let mock = server
            .mock("GET", "/files/list?parent_id=999")
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body(r#"{"error": "not found"}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let result = client.list_files(999).await;

        assert!(result.is_err());
        mock.assert_async().await;
    }
}
