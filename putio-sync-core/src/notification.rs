//! Notification system for download events
//!
//! This module provides notification support for download completions,
//! supporting multiple backends including webhooks and email.

use crate::config::{NotificationBackend, NotificationConfig};
use crate::error::{CoreError, CoreResult};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

/// Download completion event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadCompletionEvent {
    /// File ID from Put.io
    pub file_id: i64,
    /// Filename
    pub filename: String,
    /// File size in bytes
    pub file_size: i64,
    /// Local path where file was saved
    pub local_path: String,
    /// Download duration in seconds
    pub duration_seconds: i64,
    /// Average download speed in bytes/sec
    pub average_speed: i64,
    /// Timestamp of completion (ISO 8601 format)
    pub completed_at: String,
}

/// Notification service
pub struct NotificationService {
    config: NotificationConfig,
    http_client: Client,
}

impl NotificationService {
    /// Create a new notification service
    pub fn new(config: NotificationConfig) -> Self {
        Self {
            config,
            http_client: Client::new(),
        }
    }

    /// Send a download completion notification
    pub async fn notify_download_complete(&self, event: DownloadCompletionEvent) -> CoreResult<()> {
        if !self.config.enabled {
            debug!("Notifications disabled, skipping");
            return Ok(());
        }

        match self.config.backend {
            NotificationBackend::None => {
                debug!("Notification backend is None, skipping");
                Ok(())
            }
            NotificationBackend::Webhook => self.send_webhook_notification(event).await,
            NotificationBackend::Email => {
                warn!("Email notifications not yet implemented");
                Ok(())
            }
        }
    }

    /// Send webhook notification
    async fn send_webhook_notification(&self, event: DownloadCompletionEvent) -> CoreResult<()> {
        let webhook_url = self
            .config
            .webhook_url
            .as_ref()
            .ok_or_else(|| CoreError::ConfigError("Webhook URL not configured".to_string()))?;

        info!("Sending webhook notification to: {}", webhook_url);
        debug!("Webhook payload: {:?}", event);

        let response = self
            .http_client
            .post(webhook_url)
            .json(&event)
            .send()
            .await
            .map_err(|e| CoreError::network_error(&format!("Failed to send webhook: {}", e)))?;

        if response.status().is_success() {
            info!("Webhook notification sent successfully");
            Ok(())
        } else {
            let status = response.status();
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read response".to_string());
            error!(
                "Webhook notification failed with status {}: {}",
                status, error_body
            );
            Err(CoreError::network_error(&format!(
                "Webhook returned status {}: {}",
                status, error_body
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NotificationBackend;

    #[test]
    fn test_notification_service_creation() {
        let config = NotificationConfig {
            enabled: true,
            backend: NotificationBackend::Webhook,
            webhook_url: Some("https://example.com/webhook".to_string()),
            email: None,
        };

        let service = NotificationService::new(config);
        assert!(service.config.enabled);
        assert_eq!(service.config.backend, NotificationBackend::Webhook);
    }

    #[test]
    fn test_download_completion_event_serialization() {
        let event = DownloadCompletionEvent {
            file_id: 12345,
            filename: "test.mp4".to_string(),
            file_size: 1024 * 1024 * 100, // 100 MB
            local_path: "/downloads/test.mp4".to_string(),
            duration_seconds: 60,
            average_speed: 1024 * 1024, // 1 MB/s
            completed_at: "2025-11-15T10:30:00Z".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("test.mp4"));
        assert!(json.contains("12345"));
    }

    #[tokio::test]
    async fn test_notification_disabled() {
        let config = NotificationConfig {
            enabled: false,
            backend: NotificationBackend::Webhook,
            webhook_url: Some("https://example.com/webhook".to_string()),
            email: None,
        };

        let service = NotificationService::new(config);
        let event = DownloadCompletionEvent {
            file_id: 12345,
            filename: "test.mp4".to_string(),
            file_size: 1024,
            local_path: "/test.mp4".to_string(),
            duration_seconds: 1,
            average_speed: 1024,
            completed_at: "2025-11-15T10:30:00Z".to_string(),
        };

        let result = service.notify_download_complete(event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_webhook_missing_url() {
        let config = NotificationConfig {
            enabled: true,
            backend: NotificationBackend::Webhook,
            webhook_url: None, // No URL configured
            email: None,
        };

        let service = NotificationService::new(config);
        let event = DownloadCompletionEvent {
            file_id: 12345,
            filename: "test.mp4".to_string(),
            file_size: 1024,
            local_path: "/test.mp4".to_string(),
            duration_seconds: 1,
            average_speed: 1024,
            completed_at: "2025-11-15T10:30:00Z".to_string(),
        };

        let result = service.notify_download_complete(event).await;
        assert!(result.is_err());
        match result {
            Err(CoreError::ConfigError(msg)) => {
                assert!(msg.contains("Webhook URL not configured"));
            }
            _ => panic!("Expected ConfigError"),
        }
    }
}
