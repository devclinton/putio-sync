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
    /// Parent folder ID (None for root parent)
    pub parent_id: Option<i64>,
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
    /// Sort order for folders
    pub sort_by: Option<String>,
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

/// Transfer (torrent/download) information. `Debug` leaves out the magnet link, source
/// and .torrent path, which can hold a private tracker's passkey.
#[derive(Clone, Serialize, Deserialize)]
pub struct Transfer {
    /// Transfer ID
    pub id: i64,
    /// Transfer name
    pub name: String,
    /// File size in bytes
    #[serde(default)]
    pub size: Option<i64>,
    /// Transfer status
    pub status: String,
    /// Download speed in bytes/second
    #[serde(default)]
    pub down_speed: Option<i64>,
    /// Upload speed in bytes/second
    #[serde(default)]
    pub up_speed: Option<i64>,
    /// Number of bytes downloaded
    #[serde(default)]
    pub downloaded: Option<i64>,
    /// Number of bytes uploaded
    #[serde(default)]
    pub uploaded: Option<i64>,
    /// Percentage complete (0-100)
    #[serde(default)]
    pub percent_done: Option<i32>,
    /// Estimated time remaining in seconds
    pub estimated_time: Option<i64>,
    /// Number of peers connected
    #[serde(default)]
    pub peers_connected: Option<i32>,
    /// Number of peers getting from us
    #[serde(default)]
    pub peers_getting_from_us: Option<i32>,
    /// Number of peers sending to us
    #[serde(default)]
    pub peers_sending_to_us: Option<i32>,
    /// Availability percentage (0-100)
    #[serde(default)]
    pub availability: Option<i32>,
    /// Current ratio; Put.io sends it as a number or as text
    #[serde(default, deserialize_with = "number_or_text")]
    pub current_ratio: Option<f64>,
    /// Seconds spent seeding
    pub seconds_seeding: Option<i64>,
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
    /// Magnet link of a torrent transfer (empty or absent for other kinds)
    #[serde(default, skip_serializing)]
    pub magneturi: Option<String>,
    /// Put.io path to the transfer's .torrent file
    #[serde(default, skip_serializing)]
    pub torrent_link: Option<String>,
    /// What the transfer was added from: a magnet link, a URL or a Put.io path
    #[serde(default, skip_serializing)]
    pub source: Option<String>,
    /// Transfer kind (`TORRENT`, `URL`, `PLAYLIST`, ...)
    #[serde(default, rename(deserialize = "type"))]
    pub transfer_type: Option<String>,
    /// Put.io's human-readable status line
    #[serde(default)]
    pub status_message: Option<String>,
    /// Last message from the torrent's tracker
    #[serde(default)]
    pub tracker_message: Option<String>,
    /// The tracker's host name, read from Put.io's `tracker`. The rest of the address
    /// (which can hold a private tracker's passkey) is dropped while parsing.
    #[serde(
        default,
        rename(deserialize = "tracker"),
        deserialize_with = "deserialize_tracker_host"
    )]
    pub tracker_host: Option<String>,
}

impl std::fmt::Debug for Transfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transfer")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("status", &self.status)
            .field("percent_done", &self.percent_done)
            .field("transfer_type", &self.transfer_type)
            .field("tracker_host", &self.tracker_host)
            .field("is_private", &self.is_private)
            .finish_non_exhaustive()
    }
}

impl Transfer {
    /// The magnet link to add this transfer again: `magneturi`, else `source` when it is a
    /// magnet link. `None` for URL downloads and torrents added from a file without one.
    pub fn magnet(&self) -> Option<&str> {
        let magneturi = self.magneturi.as_deref().filter(|m| !m.is_empty());
        magneturi.or_else(|| self.source.as_deref().filter(|s| s.starts_with("magnet:")))
    }

    /// The tracker host from `tracker`, else from the magnet link's first `tr=`
    pub fn any_tracker_host(&self) -> Option<String> {
        self.tracker_host
            .clone()
            .or_else(|| self.magnet().and_then(magnet_tracker_host))
    }
}

/// The host name in a tracker address, lower case: the first of a comma- or
/// space-separated list, from a URL or a bare `host[:port]`. Anything else is `None`.
pub fn tracker_host(value: &str) -> Option<String> {
    let token = value
        .split(|c: char| c.is_whitespace() || c == ',')
        .find(|t| !t.is_empty())?;
    let host = if token.contains("://") {
        reqwest::Url::parse(token).ok()?.host_str()?.to_string()
    } else {
        let (host, port) = token.split_once(':').unwrap_or((token, ""));
        if !port.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        host.to_string()
    };
    let host = host
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    let bare = host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'));
    (!host.is_empty() && bare).then_some(host)
}

/// The host of the first `tr=` tracker in a magnet link
pub fn magnet_tracker_host(magnet: &str) -> Option<String> {
    let url = reqwest::Url::parse(magnet).ok()?;
    let tracker = url
        .query_pairs()
        .find(|(key, _)| key == "tr")
        .map(|(_, value)| value.into_owned())?;
    tracker_host(&tracker)
}

fn deserialize_tracker_host<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Option<String> = Option::deserialize(deserializer)?;
    Ok(value.as_deref().and_then(tracker_host))
}

/// `Option<f64>` from a JSON number, a numeric string or null
fn number_or_text<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Number(f64),
        Text(String),
    }

    match Option::<Raw>::deserialize(deserializer)? {
        None => Ok(None),
        Some(Raw::Number(n)) => Ok(Some(n)),
        Some(Raw::Text(t)) => t.trim().parse().map(Some).map_err(serde::de::Error::custom),
    }
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

/// Disk usage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    /// Total disk quota in bytes
    pub size: i64,
    /// Disk usage in bytes
    pub used: i64,
    /// Available disk space in bytes
    pub avail: i64,
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
    /// Disk information
    pub disk: DiskInfo,
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
        assert_eq!(file.parent_id, Some(0));
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
            "peers_connected": 5,
            "peers_getting_from_us": 2,
            "peers_sending_to_us": 3,
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
        assert_eq!(transfer.percent_done, Some(50));
        assert_eq!(transfer.status, "DOWNLOADING");
    }

    #[test]
    fn test_tracker_host() {
        let cases = [
            (
                "https://Tracker.Example.org:443/a1b2c3d4/announce",
                Some("tracker.example.org"),
            ),
            (
                "http://t.example.net/announce?passkey=SECRET",
                Some("t.example.net"),
            ),
            ("udp://open.example.com:6969", Some("open.example.com")),
            (
                "http://user:SECRET@t.example.net/announce",
                Some("t.example.net"),
            ),
            ("tracker.example.org:6969", Some("tracker.example.org")),
            ("tracker.example.org", Some("tracker.example.org")),
            ("udp://a.example, udp://b.example", Some("a.example")),
            ("  udp://a.example udp://b.example", Some("a.example")),
            ("tracker.example.org/SECRET/announce", None),
            ("tracker.example.org:SECRET", None),
            ("bad_host!", None),
            ("", None),
        ];
        for (value, host) in cases {
            assert_eq!(tracker_host(value).as_deref(), host, "{value}");
        }
    }

    #[test]
    fn test_magnet_tracker_host() {
        let magnet = "magnet:?xt=urn:btih:abc&dn=x&tr=https%3A%2F%2Ftracker.example.org%2FSECRET%2Fannounce&tr=udp%3A%2F%2Fb.example";
        assert_eq!(
            magnet_tracker_host(magnet).as_deref(),
            Some("tracker.example.org")
        );
        assert_eq!(magnet_tracker_host("magnet:?xt=urn:btih:abc"), None);
        assert_eq!(magnet_tracker_host("garbage"), None);
    }

    #[test]
    fn test_transfer_keeps_tracker_host_only() {
        let json = r#"{
            "id": 1, "name": "x", "status": "COMPLETED", "created_at": "2025-11-08T10:00:00",
            "save_parent_id": 0, "is_private": true, "type": "TORRENT",
            "tracker": "https://tracker.example.org/SECRET/announce",
            "magneturi": "magnet:?xt=urn:btih:abc&tr=udp%3A%2F%2Fother.example%2FSECRET",
            "source": "magnet:?xt=urn:btih:abc&tr=SECRET"
        }"#;
        let transfer: Transfer = serde_json::from_str(json).unwrap();
        assert_eq!(transfer.transfer_type.as_deref(), Some("TORRENT"));
        assert_eq!(
            transfer.any_tracker_host().as_deref(),
            Some("tracker.example.org")
        );

        let out = serde_json::to_string(&transfer).unwrap();
        assert!(!out.contains("SECRET"), "{out}");
        assert!(!format!("{transfer:?}").contains("SECRET"));
        assert!(!out.contains("magnet"), "{out}");
        assert!(out.contains(r#""transfer_type":"TORRENT""#), "{out}");
        assert!(
            out.contains(r#""tracker_host":"tracker.example.org""#),
            "{out}"
        );
    }

    #[test]
    fn test_transfer_tracker_falls_back_to_magnet() {
        let json = r#"{
            "id": 1, "name": "x", "status": "COMPLETED", "created_at": "2025-11-08T10:00:00",
            "save_parent_id": 0, "tracker": null,
            "magneturi": "magnet:?xt=urn:btih:abc&tr=udp%3A%2F%2Fopen.example.com%3A6969"
        }"#;
        let transfer: Transfer = serde_json::from_str(json).unwrap();
        assert_eq!(transfer.tracker_host, None);
        assert_eq!(
            transfer.any_tracker_host().as_deref(),
            Some("open.example.com")
        );
        assert_eq!(transfer.transfer_type, None);
    }

    #[test]
    fn test_parse_account_info() {
        let json = r#"{
            "user_id": 12345,
            "username": "testuser",
            "mail": "test@example.com",
            "account_active": true,
            "disk": {
                "size": 10737418240,
                "used": 5368709120,
                "avail": 5368709120
            },
            "can_create_sub_account": false
        }"#;

        let info: AccountInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.user_id, 12345);
        assert_eq!(info.username, "testuser");
        assert_eq!(info.mail, "test@example.com");
        assert!(info.account_active);
        assert_eq!(info.disk.size, 10737418240);
        assert_eq!(info.disk.used, 5368709120);
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
                "disk": {
                    "size": 10737418240,
                    "used": 5368709120,
                    "avail": 5368709120
                },
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
            parent_id: Some(0),
            content_type: Some("text/plain".to_string()),
            created_at: None,
            first_accessed_at: None,
            is_folder: false,
            is_shared: false,
            screenshot: None,
            opensubtitles_hash: None,
            crc32: None,
            extension: Some("txt".to_string()),
            sort_by: None,
        };

        let json = serde_json::to_string(&file).unwrap();
        let parsed: FileEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, file.id);
        assert_eq!(parsed.name, file.name);
    }

    /// A transfer as Put.io sends it, with `extra` merged over the defaults
    fn transfer_json(extra: serde_json::Value) -> String {
        let mut t = serde_json::json!({
            "id": 7, "name": "Ubuntu ISO", "status": "DOWNLOADING",
            "created_at": "2026-10-26T10:00:00Z", "save_parent_id": 12,
            "estimated_time": null, "seconds_seeding": null, "error_message": null,
            "finished_at": null, "file_id": null
        });
        for (k, v) in extra.as_object().unwrap() {
            t[k] = v.clone();
        }
        t.to_string()
    }

    #[test]
    fn transfer_reads_magnet_and_messages() {
        let json = transfer_json(serde_json::json!({
            "magneturi": "magnet:?xt=urn:btih:abc", "source": "magnet:?xt=urn:btih:abc",
            "torrent_link": "/v2/transfers/7/torrent", "type": "TORRENT",
            "status_message": "Downloading", "tracker_message": "Peers: 0"
        }));
        let t: Transfer = serde_json::from_str(&json).unwrap();
        assert_eq!(t.magnet(), Some("magnet:?xt=urn:btih:abc"));
        assert_eq!(t.transfer_type.as_deref(), Some("TORRENT"));
        assert_eq!(t.status_message.as_deref(), Some("Downloading"));
        assert_eq!(t.tracker_message.as_deref(), Some("Peers: 0"));
        assert_eq!(t.torrent_link.as_deref(), Some("/v2/transfers/7/torrent"));
    }

    #[test]
    fn transfer_magnet_falls_back_to_a_magnet_source() {
        let from_source = transfer_json(serde_json::json!({
            "magneturi": "", "source": "magnet:?xt=urn:btih:def"
        }));
        let t: Transfer = serde_json::from_str(&from_source).unwrap();
        assert_eq!(t.magnet(), Some("magnet:?xt=urn:btih:def"));

        let url_source = transfer_json(serde_json::json!({
            "source": "https://example.com/file.iso", "type": "URL"
        }));
        let t: Transfer = serde_json::from_str(&url_source).unwrap();
        assert_eq!(t.magnet(), None);
    }

    /// Put.io sends `current_ratio` as a number or as text (go-putio FIXME)
    #[test]
    fn transfer_current_ratio_accepts_number_text_or_null() {
        for (raw, want) in [
            (serde_json::json!(0.5), Some(0.5)),
            (serde_json::json!("0.25"), Some(0.25)),
            (serde_json::json!(null), None),
        ] {
            let json = transfer_json(serde_json::json!({ "current_ratio": raw }));
            let t: Transfer = serde_json::from_str(&json).unwrap();
            assert_eq!(t.current_ratio, want);
        }
        let missing: Transfer =
            serde_json::from_str(&transfer_json(serde_json::json!({}))).unwrap();
        assert_eq!(missing.current_ratio, None);
    }
}
