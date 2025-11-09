//! Path resolution for folder navigation
//!
//! This module provides functionality to resolve folder paths to IDs,
//! enabling navigation like `/Movies/Action` to find the corresponding folder ID.

use super::client::ApiClient;
use crate::error::{CoreError, CoreResult};
use tracing::{debug, trace};

impl ApiClient {
    /// Resolve a folder path to its ID
    ///
    /// Resolves a path like `/Movies/Action` to the folder ID by traversing
    /// the folder hierarchy starting from root.
    ///
    /// # Arguments
    ///
    /// * `path` - The folder path to resolve (e.g., "/Movies/Action")
    ///
    /// # Returns
    ///
    /// The folder ID if found, or an error if the path doesn't exist
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use putio_sync_core::api::ApiClient;
    /// # async fn example() {
    /// let client = ApiClient::new("token".to_string());
    /// let folder_id = client.resolve_path("/Movies/Action").await.unwrap();
    /// println!("Folder ID: {}", folder_id);
    /// # }
    /// ```
    pub async fn resolve_path(&self, path: &str) -> CoreResult<i64> {
        // Handle empty or root path
        let path = path.trim();
        if path.is_empty() || path == "/" {
            debug!("Path is root, returning ID 0");
            return Ok(0);
        }

        // Remove leading/trailing slashes and split into components
        let path = path.trim_matches('/');
        let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        if components.is_empty() {
            debug!("No path components, returning root ID 0");
            return Ok(0);
        }

        debug!("Resolving path '{}' with {} components", path, components.len());

        // Start from root (ID 0) and traverse each component
        let mut current_id = 0;

        for (index, component) in components.iter().enumerate() {
            trace!("Looking for component '{}' in folder {}", component, current_id);

            // List files in current folder
            let response = self.list_files(current_id).await?;

            // Find the folder matching this component
            let found = response.files.iter().find(|f| {
                f.file_type == "FOLDER" && f.name.eq_ignore_ascii_case(component)
            });

            match found {
                Some(folder) => {
                    current_id = folder.id;
                    trace!("Found '{}' with ID {}", component, current_id);
                }
                None => {
                    let partial_path = components[..=index].join("/");
                    return Err(CoreError::ApiError(format!(
                        "Path component '{}' not found in /{}\nAvailable folders: {}",
                        component,
                        partial_path,
                        response.files
                            .iter()
                            .filter(|f| f.file_type == "FOLDER")
                            .map(|f| f.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                }
            }
        }

        debug!("Successfully resolved path '{}' to ID {}", path, current_id);
        Ok(current_id)
    }

    /// Check if a given ID is a folder
    ///
    /// # Arguments
    ///
    /// * `id` - The file/folder ID to check
    ///
    /// # Returns
    ///
    /// True if the ID represents a folder, false otherwise
    pub async fn is_folder(&self, id: i64) -> CoreResult<bool> {
        let response = self.get_file(id).await?;
        Ok(response.file.file_type == "FOLDER")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn test_resolve_root_path() {
        let client = ApiClient::new("test_token".to_string());

        // Test various root representations
        assert_eq!(client.resolve_path("").await.unwrap(), 0);
        assert_eq!(client.resolve_path("/").await.unwrap(), 0);
        assert_eq!(client.resolve_path("  /  ").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_resolve_single_folder() {
        let mut server = Server::new_async().await;

        // Mock listing root folder
        let _m = server
            .mock("GET", "/files/list?parent_id=0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "status": "OK",
                    "files": [
                        {
                            "id": 123,
                            "name": "Movies",
                            "size": 0,
                            "file_type": "FOLDER",
                            "content_type": "application/x-directory",
                            "parent_id": 0,
                            "created_at": "2025-11-06T10:00:00",
                            "first_accessed_at": null,
                            "is_mp4_available": false,
                            "is_shared": false
                        }
                    ],
                    "parent": {
                        "id": 0,
                        "name": "Root",
                        "size": 0,
                        "file_type": "FOLDER",
                        "parent_id": -1
                    }
                }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let result = client.resolve_path("/Movies").await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 123);
    }

    #[tokio::test]
    async fn test_resolve_nested_path() {
        let mut server = Server::new_async().await;

        // Mock listing root folder
        let _m1 = server
            .mock("GET", "/files/list?parent_id=0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "status": "OK",
                    "files": [
                        {
                            "id": 123,
                            "name": "Movies",
                            "size": 0,
                            "file_type": "FOLDER",
                            "content_type": "application/x-directory",
                            "parent_id": 0,
                            "created_at": "2025-11-06T10:00:00",
                            "first_accessed_at": null,
                            "is_mp4_available": false,
                            "is_shared": false
                        }
                    ],
                    "parent": {"id": 0, "name": "Root", "size": 0, "file_type": "FOLDER", "parent_id": -1}
                }"#,
            )
            .create_async()
            .await;

        // Mock listing Movies folder
        let _m2 = server
            .mock("GET", "/files/list?parent_id=123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "status": "OK",
                    "files": [
                        {
                            "id": 456,
                            "name": "Action",
                            "size": 0,
                            "file_type": "FOLDER",
                            "content_type": "application/x-directory",
                            "parent_id": 123,
                            "created_at": "2025-11-06T10:00:00",
                            "first_accessed_at": null,
                            "is_mp4_available": false,
                            "is_shared": false
                        }
                    ],
                    "parent": {"id": 123, "name": "Movies", "size": 0, "file_type": "FOLDER", "parent_id": 0}
                }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let result = client.resolve_path("/Movies/Action").await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 456);
    }

    #[tokio::test]
    async fn test_resolve_path_not_found() {
        let mut server = Server::new_async().await;

        let _m = server
            .mock("GET", "/files/list?parent_id=0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "status": "OK",
                    "files": [
                        {
                            "id": 123,
                            "name": "Movies",
                            "size": 0,
                            "file_type": "FOLDER",
                            "content_type": "application/x-directory",
                            "parent_id": 0,
                            "created_at": "2025-11-06T10:00:00",
                            "first_accessed_at": null,
                            "is_mp4_available": false,
                            "is_shared": false
                        }
                    ],
                    "parent": {"id": 0, "name": "Root", "size": 0, "file_type": "FOLDER", "parent_id": -1}
                }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());
        let result = client.resolve_path("/NonExistent").await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error, CoreError::ApiError(_)));
    }

    #[tokio::test]
    async fn test_resolve_case_insensitive() {
        let mut server = Server::new_async().await;

        let _m = server
            .mock("GET", "/files/list?parent_id=0")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "status": "OK",
                    "files": [
                        {
                            "id": 123,
                            "name": "Movies",
                            "size": 0,
                            "file_type": "FOLDER",
                            "content_type": "application/x-directory",
                            "parent_id": 0,
                            "created_at": "2025-11-06T10:00:00",
                            "first_accessed_at": null,
                            "is_mp4_available": false,
                            "is_shared": false
                        }
                    ],
                    "parent": {"id": 0, "name": "Root", "size": 0, "file_type": "FOLDER", "parent_id": -1}
                }"#,
            )
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());

        // Test case-insensitive matching
        assert_eq!(client.resolve_path("/movies").await.unwrap(), 123);
        assert_eq!(client.resolve_path("/MOVIES").await.unwrap(), 123);
        assert_eq!(client.resolve_path("/MoViEs").await.unwrap(), 123);
    }
}
