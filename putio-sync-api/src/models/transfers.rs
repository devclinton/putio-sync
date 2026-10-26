//! Transfer-related request and response models

use putio_sync_core::stall::Stall;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Request to add a new transfer
#[derive(Debug, Deserialize)]
pub struct AddTransferRequest {
    /// URL (magnet link or torrent URL)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Torrent file data (base64 encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_data: Option<String>,
    /// Parent folder ID (default: 0 for root)
    #[serde(default)]
    pub parent_id: i64,
}

/// Request to cancel transfers
#[derive(Debug, Deserialize)]
pub struct CancelTransfersRequest {
    /// Transfer IDs to cancel
    pub transfer_ids: Vec<i64>,
}

/// Transfer information response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TransferResponse {
    /// Transfer ID
    pub id: i64,
    /// Transfer name
    pub name: String,
    /// Transfer status
    pub status: String,
    /// File size in bytes
    pub size: i64,
    /// Download progress (0-100)
    pub percent_done: i32,
    /// Download speed in bytes/sec
    #[serde(skip_serializing_if = "Option::is_none")]
    pub down_speed: Option<i64>,
    /// Upload speed in bytes/sec
    #[serde(skip_serializing_if = "Option::is_none")]
    pub up_speed: Option<i64>,
    /// Error message (if failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// Creation date
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Put.io transfer type (`TORRENT`, `URL`, ...)
    pub transfer_type: Option<String>,
    /// Tracker host name only, never the tracker address (feature 006)
    pub tracker_host: Option<String>,
    /// Share of the torrent the swarm can supply (0-100); below 100 the last pieces may
    /// never arrive
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability: Option<i32>,
    /// Magnet link, when the transfer has one (feature 007)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub magnet: Option<String>,
    /// Set while the transfer is stalled (feature 007)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stall: Option<StallInfo>,
}

/// A stalled transfer's verdict as the API reports it
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct StallInfo {
    /// Last progress, Unix seconds
    pub since: u64,
    /// Seconds since the last progress, at response time
    pub seconds: u64,
    /// `availability N%`, `no peers` or `no progress`
    pub reason: String,
}

impl StallInfo {
    /// `stall` as seen at `now`
    pub fn new(stall: &Stall, now: SystemTime) -> Self {
        Self {
            since: stall
                .since
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            seconds: now.duration_since(stall.since).map_or(0, |d| d.as_secs()),
            reason: stall.reason.label(),
        }
    }
}

/// Answer to `POST /putio/transfers/{id}/readd`
#[derive(Debug, Serialize, Deserialize)]
pub struct ReaddResponse {
    /// The transfer that was cancelled
    pub cancelled_id: i64,
    /// The transfer Put.io created from the same magnet
    pub transfer: TransferResponse,
}

impl From<putio_sync_client::Transfer> for TransferResponse {
    fn from(transfer: putio_sync_client::Transfer) -> Self {
        let tracker_host = transfer.any_tracker_host();
        Self {
            availability: transfer.availability,
            // A private tracker's magnet can hold its passkey: Re-add uses it server side,
            // but only public magnets leave the API (for Copy magnet)
            magnet: transfer
                .magnet()
                .filter(|_| !transfer.is_private)
                .map(str::to_string),
            stall: None,
            id: transfer.id,
            name: transfer.name,
            status: transfer.status,
            size: transfer.size.unwrap_or(0),
            percent_done: transfer.percent_done.unwrap_or(0),
            down_speed: transfer.down_speed,
            up_speed: transfer.up_speed,
            error_message: transfer.error_message,
            created_at: Some(transfer.created_at),
            transfer_type: transfer.transfer_type,
            tracker_host,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_transfer_request_url() {
        let json = r#"{"url": "magnet:?xt=urn:btih:test", "parent_id": 123}"#;
        let req: AddTransferRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.url, Some("magnet:?xt=urn:btih:test".to_string()));
        assert_eq!(req.parent_id, 123);
        assert!(req.file_data.is_none());
    }

    #[test]
    fn test_add_transfer_request_defaults() {
        let json = r#"{"url": "magnet:?xt=urn:btih:test"}"#;
        let req: AddTransferRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.parent_id, 0);
    }

    #[test]
    fn test_cancel_transfers_request() {
        let json = r#"{"transfer_ids": [1, 2, 3]}"#;
        let req: CancelTransfersRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.transfer_ids, vec![1, 2, 3]);
    }

    #[test]
    fn test_transfer_response_serialization() {
        let response = TransferResponse {
            id: 123,
            name: "test.torrent".to_string(),
            status: "DOWNLOADING".to_string(),
            size: 1024000,
            percent_done: 50,
            down_speed: Some(102400),
            up_speed: Some(10240),
            error_message: None,
            created_at: Some("2024-01-01".to_string()),
            transfer_type: Some("TORRENT".to_string()),
            tracker_host: None,
            availability: None,
            magnet: None,
            stall: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("test.torrent"));
        assert!(json.contains("DOWNLOADING"));
    }
}
