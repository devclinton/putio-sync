//! File and folder API endpoints

use super::client::ApiClient;
use super::models::{ApiResponse, FileResponse, FilesListResponse};
use crate::error::CoreResult;

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::FileEntry;
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
