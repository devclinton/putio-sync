//! Queue-related request and response models

use serde::{Deserialize, Serialize};

/// Request to add items to queue
#[derive(Debug, Deserialize)]
pub struct AddToQueueRequest {
    /// File IDs to add to queue
    pub file_ids: Vec<i64>,
    /// Priority (higher = download first)
    #[serde(default)]
    pub priority: i32,
}

/// Request to remove items from queue
#[derive(Debug, Deserialize)]
pub struct RemoveFromQueueRequest {
    /// File IDs to remove from queue
    pub file_ids: Vec<i64>,
}

/// Request to reorder queue items
#[derive(Debug, Deserialize)]
pub struct ReorderQueueRequest {
    /// Ordered list of file IDs (first = highest priority)
    pub file_ids: Vec<i64>,
}

/// Request to change priority
#[derive(Debug, Deserialize)]
pub struct ChangePriorityRequest {
    /// New priority value
    pub priority: i32,
}

/// Queue item response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct QueueItemResponse {
    /// File ID
    pub file_id: i64,
    /// File name
    pub name: String,
    /// File size in bytes
    pub size: i64,
    /// Priority (higher = download first)
    pub priority: i32,
    /// Queue status (pending, downloading, paused, etc.)
    pub status: String,
    /// Progress (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<i32>,
}

/// Queue statistics response
#[derive(Debug, Serialize, Deserialize)]
pub struct QueueStatsResponse {
    /// Total items in queue
    pub total_items: usize,
    /// Items currently downloading
    pub downloading: usize,
    /// Items pending
    pub pending: usize,
    /// Items paused
    pub paused: usize,
    /// Total size in bytes
    pub total_size: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_to_queue_request() {
        let json = r#"{"file_ids": [1, 2, 3], "priority": 10}"#;
        let req: AddToQueueRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.file_ids, vec![1, 2, 3]);
        assert_eq!(req.priority, 10);
    }

    #[test]
    fn test_add_to_queue_request_defaults() {
        let json = r#"{"file_ids": [1, 2, 3]}"#;
        let req: AddToQueueRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.priority, 0);
    }

    #[test]
    fn test_remove_from_queue_request() {
        let json = r#"{"file_ids": [1, 2]}"#;
        let req: RemoveFromQueueRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.file_ids, vec![1, 2]);
    }

    #[test]
    fn test_reorder_queue_request() {
        let json = r#"{"file_ids": [3, 1, 2]}"#;
        let req: ReorderQueueRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.file_ids, vec![3, 1, 2]);
    }

    #[test]
    fn test_queue_item_response_serialization() {
        let response = QueueItemResponse {
            file_id: 123,
            name: "test.mkv".to_string(),
            size: 1024000,
            priority: 5,
            status: "pending".to_string(),
            progress: Some(50),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("test.mkv"));
        assert!(json.contains("pending"));
    }

    #[test]
    fn test_queue_stats_response() {
        let stats = QueueStatsResponse {
            total_items: 10,
            downloading: 2,
            pending: 7,
            paused: 1,
            total_size: 10240000,
        };

        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("10"));
        assert!(json.contains("10240000"));
    }
}
