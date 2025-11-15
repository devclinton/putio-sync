//! Database models for SQLite storage

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Queue item status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueStatus {
    /// Item is pending download
    Pending,
    /// Item is currently being downloaded
    Downloading,
    /// Item download is paused
    Paused,
    /// Item download completed
    Completed,
    /// Item download failed
    Failed,
}

impl std::fmt::Display for QueueStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueueStatus::Pending => write!(f, "pending"),
            QueueStatus::Downloading => write!(f, "downloading"),
            QueueStatus::Paused => write!(f, "paused"),
            QueueStatus::Completed => write!(f, "completed"),
            QueueStatus::Failed => write!(f, "failed"),
        }
    }
}

impl std::str::FromStr for QueueStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(QueueStatus::Pending),
            "downloading" => Ok(QueueStatus::Downloading),
            "paused" => Ok(QueueStatus::Paused),
            "completed" => Ok(QueueStatus::Completed),
            "failed" => Ok(QueueStatus::Failed),
            _ => Err(format!("Invalid queue status: {}", s)),
        }
    }
}

/// Queue item in the database
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct QueueItem {
    /// Primary key
    pub id: i64,
    /// Put.io file ID
    pub file_id: i64,
    /// Original filename
    pub filename: String,
    /// File size in bytes
    pub file_size: i64,
    /// Download URL
    pub download_url: String,
    /// Priority (higher = downloaded first)
    pub priority: i64,
    /// Queue status
    pub status: String,
    /// Timestamp when item was added
    pub created_at: String,
    /// Timestamp when item was last updated
    pub updated_at: String,
}

impl QueueItem {
    /// Get the queue status as enum
    pub fn status_enum(&self) -> Result<QueueStatus, String> {
        self.status.parse()
    }
}


/// Download status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    /// Download completed successfully
    Completed,
    /// Download is in progress
    InProgress,
    /// Download failed
    Failed,
}

impl std::fmt::Display for DownloadStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DownloadStatus::Completed => write!(f, "completed"),
            DownloadStatus::InProgress => write!(f, "in_progress"),
            DownloadStatus::Failed => write!(f, "failed"),
        }
    }
}

impl std::str::FromStr for DownloadStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "completed" => Ok(DownloadStatus::Completed),
            "in_progress" => Ok(DownloadStatus::InProgress),
            "failed" => Ok(DownloadStatus::Failed),
            _ => Err(format!("Invalid download status: {}", s)),
        }
    }
}

/// Download record in the database
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Download {
    /// Primary key
    pub id: i64,
    /// Put.io file ID
    pub file_id: i64,
    /// Original filename
    pub filename: String,
    /// File size in bytes
    pub file_size: i64,
    /// Local filesystem path
    pub local_path: String,
    /// Timestamp when download completed
    pub downloaded_at: String, // SQLite datetime as string
    /// Download status (completed, in_progress, failed)
    pub status: String,
    /// Number of retry attempts
    pub retry_count: i64,
}

impl Download {
    /// Get the download status as enum
    pub fn status_enum(&self) -> Result<DownloadStatus, String> {
        self.status.parse()
    }
}

/// Chunk status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChunkStatus {
    /// Chunk download is pending
    Pending,
    /// Chunk is being downloaded
    Downloading,
    /// Chunk download completed
    Completed,
}

impl std::fmt::Display for ChunkStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChunkStatus::Pending => write!(f, "pending"),
            ChunkStatus::Downloading => write!(f, "downloading"),
            ChunkStatus::Completed => write!(f, "completed"),
        }
    }
}

impl std::str::FromStr for ChunkStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(ChunkStatus::Pending),
            "downloading" => Ok(ChunkStatus::Downloading),
            "completed" => Ok(ChunkStatus::Completed),
            _ => Err(format!("Invalid chunk status: {}", s)),
        }
    }
}

/// Download chunk record
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DownloadChunk {
    /// Primary key
    pub id: i64,
    /// Foreign key to downloads table
    pub download_id: i64,
    /// Chunk index (0-based)
    pub chunk_index: i64,
    /// Starting byte position (inclusive)
    pub start_byte: i64,
    /// Ending byte position (inclusive)
    pub end_byte: i64,
    /// Chunk status (pending, downloading, completed)
    pub status: String,
}

impl DownloadChunk {
    /// Get the chunk status as enum
    pub fn status_enum(&self) -> Result<ChunkStatus, String> {
        self.status.parse()
    }
}

/// History action type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryAction {
    /// File was downloaded
    Downloaded,
    /// File was deleted
    Deleted,
}

impl std::fmt::Display for HistoryAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HistoryAction::Downloaded => write!(f, "downloaded"),
            HistoryAction::Deleted => write!(f, "deleted"),
        }
    }
}

impl std::str::FromStr for HistoryAction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "downloaded" => Ok(HistoryAction::Downloaded),
            "deleted" => Ok(HistoryAction::Deleted),
            _ => Err(format!("Invalid history action: {}", s)),
        }
    }
}

/// Download history record
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DownloadHistory {
    /// Primary key
    pub id: i64,
    /// Put.io file ID
    pub file_id: i64,
    /// Original filename
    pub filename: String,
    /// File size in bytes
    pub file_size: i64,
    /// File extension (normalized to lowercase)
    pub extension: Option<String>,
    /// Action performed (downloaded, deleted)
    pub action: String,
    /// Timestamp when action occurred
    pub timestamp: String, // SQLite datetime as string
    /// Duration in seconds (for downloads)
    pub duration_seconds: Option<i64>,
    /// Average download speed in bytes/sec
    pub average_speed: Option<i64>,
}

impl DownloadHistory {
    /// Get the action as enum
    pub fn action_enum(&self) -> Result<HistoryAction, String> {
        self.action.parse()
    }
}

/// Download statistics
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DownloadStats {
    /// Primary key (always 1)
    pub id: i64,
    /// Total number of downloads
    pub total_downloads: i64,
    /// Total bytes downloaded
    pub total_bytes: i64,
    /// Total time spent downloading in seconds
    pub total_time_seconds: i64,
    /// Last time stats were updated
    pub last_updated: String, // SQLite datetime as string
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_status_display() {
        assert_eq!(DownloadStatus::Completed.to_string(), "completed");
        assert_eq!(DownloadStatus::InProgress.to_string(), "in_progress");
        assert_eq!(DownloadStatus::Failed.to_string(), "failed");
    }

    #[test]
    fn test_download_status_from_str() {
        assert_eq!("completed".parse::<DownloadStatus>().unwrap(), DownloadStatus::Completed);
        assert_eq!("in_progress".parse::<DownloadStatus>().unwrap(), DownloadStatus::InProgress);
        assert_eq!("failed".parse::<DownloadStatus>().unwrap(), DownloadStatus::Failed);
        assert!("invalid".parse::<DownloadStatus>().is_err());
    }

    #[test]
    fn test_chunk_status_display() {
        assert_eq!(ChunkStatus::Pending.to_string(), "pending");
        assert_eq!(ChunkStatus::Downloading.to_string(), "downloading");
        assert_eq!(ChunkStatus::Completed.to_string(), "completed");
    }

    #[test]
    fn test_chunk_status_from_str() {
        assert_eq!("pending".parse::<ChunkStatus>().unwrap(), ChunkStatus::Pending);
        assert_eq!("downloading".parse::<ChunkStatus>().unwrap(), ChunkStatus::Downloading);
        assert_eq!("completed".parse::<ChunkStatus>().unwrap(), ChunkStatus::Completed);
        assert!("invalid".parse::<ChunkStatus>().is_err());
    }

    #[test]
    fn test_history_action_display() {
        assert_eq!(HistoryAction::Downloaded.to_string(), "downloaded");
        assert_eq!(HistoryAction::Deleted.to_string(), "deleted");
    }

    #[test]
    fn test_history_action_from_str() {
        assert_eq!("downloaded".parse::<HistoryAction>().unwrap(), HistoryAction::Downloaded);
        assert_eq!("deleted".parse::<HistoryAction>().unwrap(), HistoryAction::Deleted);
        assert!("invalid".parse::<HistoryAction>().is_err());
    }
}
