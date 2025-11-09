//! Data models for Put.io API responses

use serde::{Deserialize, Serialize};

/// Standard API response wrapper
///
/// Most Put.io API responses follow this structure with a generic data field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    /// Response status (typically "OK")
    pub status: String,
    /// The actual response data
    #[serde(flatten)]
    pub data: T,
}

/// API error response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    /// Error message
    pub error_message: Option<String>,
    /// Error type
    pub error_type: Option<String>,
    /// HTTP status code
    pub status_code: Option<u16>,
}

/// File or folder entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// Unique file ID
    pub id: i64,
    /// File or folder name
    pub name: String,
    /// File size in bytes (0 for folders)
    pub size: i64,
    /// File type ("FILE", "FOLDER", "VIDEO", "AUDIO", etc.)
    pub file_type: String,
    /// Parent folder ID (0 for root)
    pub parent_id: i64,
    /// Content type (MIME type)
    pub content_type: Option<String>,
    /// Creation timestamp
    pub created_at: Option<String>,
    /// First accessed timestamp
    pub first_accessed_at: Option<String>,
    /// Whether this is a folder
    #[serde(default)]
    pub is_folder: bool,
    /// Whether this is a shared file
    #[serde(default)]
    pub is_shared: bool,
    /// Screenshot URL (for videos)
    pub screenshot: Option<String>,
    /// OpenSubtitles hash
    pub opensubtitles_hash: Option<String>,
    /// CRC32 checksum
    pub crc32: Option<String>,
    /// File extension
    pub extension: Option<String>,
}

/// Response for file listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesListResponse {
    /// List of files
    pub files: Vec<FileEntry>,
    /// Parent folder information
    pub parent: FileEntry,
    /// Total number of files
    #[serde(default)]
    pub total: i64,
    /// Cursor for pagination
    pub cursor: Option<String>,
}

/// Single file response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileResponse {
    /// The file entry
    pub file: FileEntry,
}

/// Transfer (torrent/download) information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transfer {
    /// Transfer ID
    pub id: i64,
    /// Transfer name
    pub name: String,
    /// File size in bytes
    pub size: i64,
    /// Transfer status
    pub status: String,
    /// Download speed in bytes/second
    pub down_speed: i64,
    /// Upload speed in bytes/second
    pub up_speed: i64,
    /// Number of bytes downloaded
    pub downloaded: i64,
    /// Number of bytes uploaded
    pub uploaded: i64,
    /// Percentage complete (0-100)
    pub percent_done: i32,
    /// Estimated time remaining in seconds
    pub estimated_time: i64,
    /// Number of peers
    pub peers: i32,
    /// Availability percentage (0-100)
    pub availability: i32,
    /// Current ratio
    pub current_ratio: f64,
    /// Seconds spent seeding
    pub seconds_seeding: i64,
    /// Error message if failed
    pub error_message: Option<String>,
    /// Creation timestamp
    pub created_at: String,
    /// Completion timestamp
    pub finished_at: Option<String>,
    /// Folder ID where files will be saved
    pub save_parent_id: i64,
    /// File ID after transfer completes
    pub file_id: Option<i64>,
    /// Whether transfer is private
    #[serde(default)]
    pub is_private: bool,
}

/// Response for transfer listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransfersListResponse {
    /// List of transfers
    pub transfers: Vec<Transfer>,
}

/// Single transfer response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferResponse {
    /// The transfer
    pub transfer: Transfer,
}

/// Account information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    /// User ID
    pub user_id: i64,
    /// Username
    pub username: String,
    /// Email address
    pub mail: String,
    /// Account type
    pub account_active: bool,
    /// Disk quota in bytes
    pub disk_quota: i64,
    /// Disk usage in bytes
    pub disk_used: i64,
    /// Available disk space in bytes
    pub disk_available: i64,
    /// Whether user can create subusers
    #[serde(default)]
    pub can_create_sub_account: bool,
    /// Avatar URL
    pub avatar_url: Option<String>,
}

/// Response for account info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfoResponse {
    /// Account information
    pub info: AccountInfo,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_file_entry() {
        let json = r#"{
            "id": 12345,
            "name": "test.mp4",
            "size": 1024000,
            "file_type": "VIDEO",
            "parent_id": 0,
            "content_type": "video/mp4",
            "created_at": "2025-11-08T10:00:00",
            "is_folder": false,
            "is_shared": false,
            "extension": "mp4"
        }"#;

        let file: FileEntry = serde_json::from_str(json).unwrap();
        assert_eq!(file.id, 12345);
        assert_eq!(file.name, "test.mp4");
        assert_eq!(file.size, 1024000);
        assert_eq!(file.file_type, "VIDEO");
        assert_eq!(file.parent_id, 0);
        assert!(!file.is_folder);
    }

    #[test]
    fn test_parse_files_list_response() {
        let json = r#"{
            "files": [
                {
                    "id": 1,
                    "name": "file1.txt",
                    "size": 100,
                    "file_type": "FILE",
                    "parent_id": 0,
                    "is_folder": false,
                    "is_shared": false
                },
                {
                    "id": 2,
                    "name": "folder1",
                    "size": 0,
                    "file_type": "FOLDER",
                    "parent_id": 0,
                    "is_folder": true,
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
            "total": 2
        }"#;

        let response: FilesListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.files.len(), 2);
        assert_eq!(response.total, 2);
        assert_eq!(response.parent.name, "Root");
    }

    #[test]
    fn test_parse_transfer() {
        let json = r#"{
            "id": 123,
            "name": "Test Transfer",
            "size": 5000000,
            "status": "DOWNLOADING",
            "down_speed": 1024000,
            "up_speed": 512000,
            "downloaded": 2500000,
            "uploaded": 1000000,
            "percent_done": 50,
            "estimated_time": 300,
            "peers": 5,
            "availability": 100,
            "current_ratio": 0.4,
            "seconds_seeding": 0,
            "created_at": "2025-11-08T10:00:00",
            "save_parent_id": 0,
            "is_private": false
        }"#;

        let transfer: Transfer = serde_json::from_str(json).unwrap();
        assert_eq!(transfer.id, 123);
        assert_eq!(transfer.name, "Test Transfer");
        assert_eq!(transfer.percent_done, 50);
        assert_eq!(transfer.status, "DOWNLOADING");
    }

    #[test]
    fn test_parse_account_info() {
        let json = r#"{
            "user_id": 12345,
            "username": "testuser",
            "mail": "test@example.com",
            "account_active": true,
            "disk_quota": 10737418240,
            "disk_used": 5368709120,
            "disk_available": 5368709120,
            "can_create_sub_account": false
        }"#;

        let info: AccountInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.user_id, 12345);
        assert_eq!(info.username, "testuser");
        assert_eq!(info.mail, "test@example.com");
        assert!(info.account_active);
    }

    #[test]
    fn test_parse_api_response() {
        let json = r#"{
            "status": "OK",
            "info": {
                "user_id": 12345,
                "username": "testuser",
                "mail": "test@example.com",
                "account_active": true,
                "disk_quota": 10737418240,
                "disk_used": 5368709120,
                "disk_available": 5368709120,
                "can_create_sub_account": false
            }
        }"#;

        let response: ApiResponse<AccountInfoResponse> = serde_json::from_str(json).unwrap();
        assert_eq!(response.status, "OK");
        assert_eq!(response.data.info.username, "testuser");
    }

    #[test]
    fn test_file_entry_serialization() {
        let file = FileEntry {
            id: 123,
            name: "test.txt".to_string(),
            size: 1024,
            file_type: "FILE".to_string(),
            parent_id: 0,
            content_type: Some("text/plain".to_string()),
            created_at: None,
            first_accessed_at: None,
            is_folder: false,
            is_shared: false,
            screenshot: None,
            opensubtitles_hash: None,
            crc32: None,
            extension: Some("txt".to_string()),
        };

        let json = serde_json::to_string(&file).unwrap();
        let parsed: FileEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, file.id);
        assert_eq!(parsed.name, file.name);
    }
}
