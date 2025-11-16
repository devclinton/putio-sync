//! File-related request and response models

use serde::{Deserialize, Serialize};

/// Request to list files
#[derive(Debug, Deserialize)]
pub struct ListFilesRequest {
    /// Parent folder ID (default: 0 for root)
    #[serde(default)]
    pub parent_id: i64,
    /// Path to list (alternative to parent_id)
    pub path: Option<String>,
    /// Filter pattern (glob)
    pub filter: Option<String>,
    /// Recursive listing
    #[serde(default)]
    pub recursive: bool,
}

/// Request to delete files
#[derive(Debug, Deserialize)]
pub struct DeleteFilesRequest {
    /// File IDs to delete
    pub file_ids: Vec<i64>,
}

/// File information response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileResponse {
    /// File ID
    pub id: i64,
    /// File name
    pub name: String,
    /// File size in bytes
    pub size: i64,
    /// File type (file or folder)
    pub file_type: String,
    /// Content type (MIME type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// Creation date
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Parent folder ID
    pub parent_id: Option<i64>,
}

impl From<putio_sync_core::api::FileEntry> for FileResponse {
    fn from(entry: putio_sync_core::api::FileEntry) -> Self {
        Self {
            id: entry.id,
            name: entry.name,
            size: entry.size,
            file_type: entry.file_type,
            content_type: entry.content_type,
            created_at: entry.created_at,
            parent_id: entry.parent_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_files_request_deserialization() {
        let json = r#"{"parent_id": 123, "recursive": true}"#;
        let req: ListFilesRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.parent_id, 123);
        assert!(req.recursive);
        assert!(req.path.is_none());
        assert!(req.filter.is_none());
    }

    #[test]
    fn test_list_files_request_defaults() {
        let json = r#"{}"#;
        let req: ListFilesRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.parent_id, 0);
        assert!(!req.recursive);
    }

    #[test]
    fn test_delete_files_request() {
        let json = r#"{"file_ids": [1, 2, 3]}"#;
        let req: DeleteFilesRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.file_ids, vec![1, 2, 3]);
    }

    #[test]
    fn test_file_response_serialization() {
        let response = FileResponse {
            id: 123,
            name: "test.txt".to_string(),
            size: 1024,
            file_type: "file".to_string(),
            content_type: Some("text/plain".to_string()),
            created_at: Some("2024-01-01".to_string()),
            parent_id: Some(0),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("test.txt"));
        assert!(json.contains("1024"));
    }
}
