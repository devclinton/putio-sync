//! Request and response models
//!
//! This module contains the data models for API requests and responses.

pub mod files;
pub mod queue;
pub mod transfers;

use serde::{Deserialize, Serialize};

/// Generic success response
#[derive(Debug, Serialize, Deserialize)]
pub struct SuccessResponse {
    /// Success message
    pub message: String,
}

/// Generic list response wrapper
#[derive(Debug, Serialize, Deserialize)]
pub struct ListResponse<T> {
    /// Items in the list
    pub items: Vec<T>,
    /// Total count (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
}

impl<T> ListResponse<T> {
    /// Create a new list response
    pub fn new(items: Vec<T>) -> Self {
        let total = Some(items.len());
        Self { items, total }
    }

    /// Create a list response with custom total
    pub fn with_total(items: Vec<T>, total: usize) -> Self {
        Self {
            items,
            total: Some(total),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_success_response() {
        let response = SuccessResponse {
            message: "Operation completed".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("Operation completed"));
    }

    #[test]
    fn test_list_response() {
        let items = vec![1, 2, 3];
        let response = ListResponse::new(items);
        assert_eq!(response.items.len(), 3);
        assert_eq!(response.total, Some(3));
    }

    #[test]
    fn test_list_response_with_custom_total() {
        let items = vec![1, 2, 3];
        let response = ListResponse::with_total(items, 10);
        assert_eq!(response.items.len(), 3);
        assert_eq!(response.total, Some(10));
    }
}
