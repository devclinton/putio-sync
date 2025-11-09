//! HTTP client for Put.io API

use crate::error::{CoreError, CoreResult};
use reqwest::{header, Client, Request, Response, StatusCode};
use serde::de::DeserializeOwned;
use std::time::Duration;
use tracing::{debug, trace, warn};

/// Default base URL for Put.io API
pub const DEFAULT_BASE_URL: &str = "https://api.put.io/v2";

/// Maximum number of retry attempts
const MAX_RETRIES: u32 = 3;

/// Initial retry delay in milliseconds
const INITIAL_RETRY_DELAY_MS: u64 = 100;

/// Put.io API client
///
/// Provides HTTP client wrapper with authentication, retry logic,
/// and request/response logging.
pub struct ApiClient {
    client: Client,
    base_url: String,
    token: String,
}

impl ApiClient {
    /// Create a new API client with the given OAuth token
    ///
    /// # Arguments
    ///
    /// * `token` - OAuth2 token for Put.io API authentication
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_core::api::ApiClient;
    ///
    /// let client = ApiClient::new("my_oauth_token".to_string());
    /// ```
    pub fn new(token: String) -> Self {
        Self::with_base_url(token, DEFAULT_BASE_URL.to_string())
    }

    /// Create a new API client with a custom base URL
    ///
    /// Useful for testing or using alternative API endpoints.
    ///
    /// # Arguments
    ///
    /// * `token` - OAuth2 token for Put.io API authentication
    /// * `base_url` - Base URL for the API (e.g., "https://api.put.io/v2")
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_core::api::ApiClient;
    ///
    /// let client = ApiClient::with_base_url(
    ///     "my_oauth_token".to_string(),
    ///     "https://test.api.put.io/v2".to_string()
    /// );
    /// ```
    pub fn with_base_url(token: String, base_url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url,
            token,
        }
    }

    /// Execute a GET request with retry logic
    ///
    /// # Arguments
    ///
    /// * `path` - API path (e.g., "/files/list")
    ///
    /// # Returns
    ///
    /// Response body deserialized as type T
    pub(crate) async fn get<T: DeserializeOwned>(&self, path: &str) -> CoreResult<T> {
        let url = format!("{}{}", self.base_url, path);
        debug!("GET request to: {}", url);

        let request = self
            .client
            .get(&url)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.token))
            .build()
            .map_err(|e| CoreError::HttpError(e))?;

        self.execute_with_retry(request).await
    }

    /// Execute a request with exponential backoff retry logic
    ///
    /// Retries on network errors and 5xx server errors.
    async fn execute_with_retry<T: DeserializeOwned>(
        &self,
        request: Request,
    ) -> CoreResult<T> {
        let mut attempts = 0;
        let mut delay_ms = INITIAL_RETRY_DELAY_MS;

        loop {
            attempts += 1;
            trace!("Attempt {} for {}", attempts, request.url());

            // Clone the request for retry attempts
            let req = request.try_clone().ok_or_else(|| {
                CoreError::InternalError("Cannot clone request for retry".to_string())
            })?;

            match self.client.execute(req).await {
                Ok(response) => {
                    let status = response.status();
                    trace!("Response status: {}", status);

                    // Log response details
                    if status.is_success() {
                        return self.handle_success_response(response).await;
                    } else if status.is_client_error() {
                        return self.handle_error_response(response).await;
                    } else if status.is_server_error() && attempts < MAX_RETRIES {
                        // Retry on server errors
                        warn!(
                            "Server error ({}), retrying in {}ms (attempt {}/{})",
                            status, delay_ms, attempts, MAX_RETRIES
                        );
                    } else {
                        // No more retries
                        return self.handle_error_response(response).await;
                    }
                }
                Err(e) if attempts < MAX_RETRIES => {
                    // Retry on network errors
                    warn!(
                        "Network error: {}, retrying in {}ms (attempt {}/{})",
                        e, delay_ms, attempts, MAX_RETRIES
                    );
                }
                Err(e) => {
                    // No more retries
                    return Err(CoreError::HttpError(e));
                }
            }

            // Wait before retry with exponential backoff
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            delay_ms *= 2; // Exponential backoff
        }
    }

    /// Handle successful API response
    async fn handle_success_response<T: DeserializeOwned>(
        &self,
        response: Response,
    ) -> CoreResult<T> {
        let body = response.text().await.map_err(CoreError::HttpError)?;
        trace!("Response body: {}", body);

        serde_json::from_str(&body).map_err(|e| {
            CoreError::SerializationError(format!("Failed to parse response: {}", e))
        })
    }

    /// Handle error API response
    async fn handle_error_response<T: DeserializeOwned>(
        &self,
        response: Response,
    ) -> CoreResult<T> {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read error body".to_string());

        trace!("Error response body: {}", body);

        let error_msg = match status {
            StatusCode::UNAUTHORIZED => "Authentication failed. Invalid or expired token.".to_string(),
            StatusCode::FORBIDDEN => "Access forbidden. Check your permissions.".to_string(),
            StatusCode::NOT_FOUND => "Resource not found.".to_string(),
            StatusCode::TOO_MANY_REQUESTS => "Rate limit exceeded. Please try again later.".to_string(),
            _ => format!("API error ({}): {}", status, body),
        };

        Err(CoreError::ApiError(error_msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Server, ServerGuard};

    async fn setup_mock_server() -> ServerGuard {
        Server::new_async().await
    }

    #[tokio::test]
    async fn test_new_client() {
        let client = ApiClient::new("test_token".to_string());
        assert_eq!(client.base_url, DEFAULT_BASE_URL);
        assert_eq!(client.token, "test_token");
    }

    #[tokio::test]
    async fn test_with_base_url() {
        let custom_url = "https://custom.api.put.io/v2";
        let client = ApiClient::with_base_url("test_token".to_string(), custom_url.to_string());
        assert_eq!(client.base_url, custom_url);
        assert_eq!(client.token, "test_token");
    }

    #[tokio::test]
    async fn test_successful_request() {
        let mut server = setup_mock_server().await;

        let mock = server
            .mock("GET", "/test")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status": "ok"}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());

        #[derive(Debug, serde::Deserialize)]
        struct TestResponse {
            status: String,
        }

        let response: CoreResult<TestResponse> = client.get("/test").await;
        assert!(response.is_ok());
        assert_eq!(response.unwrap().status, "ok");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_unauthorized_request() {
        let mut server = setup_mock_server().await;

        let mock = server
            .mock("GET", "/test")
            .with_status(401)
            .with_header("content-type", "application/json")
            .with_body(r#"{"error": "unauthorized"}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url("bad_token".to_string(), server.url());

        #[derive(Debug, serde::Deserialize)]
        struct TestResponse {
            status: String,
        }

        let response: CoreResult<TestResponse> = client.get("/test").await;
        assert!(response.is_err());
        assert!(matches!(response.unwrap_err(), CoreError::ApiError(_)));

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_retry_on_server_error() {
        let mut server = setup_mock_server().await;

        // First two attempts fail with 500, third succeeds
        let mock_fail1 = server
            .mock("GET", "/test")
            .with_status(500)
            .expect(1)
            .create_async()
            .await;

        let mock_fail2 = server
            .mock("GET", "/test")
            .with_status(500)
            .expect(1)
            .create_async()
            .await;

        let mock_success = server
            .mock("GET", "/test")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status": "ok"}"#)
            .expect(1)
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());

        #[derive(Debug, serde::Deserialize)]
        struct TestResponse {
            status: String,
        }

        let response: CoreResult<TestResponse> = client.get("/test").await;
        assert!(response.is_ok());

        mock_fail1.assert_async().await;
        mock_fail2.assert_async().await;
        mock_success.assert_async().await;
    }

    #[tokio::test]
    async fn test_max_retries_exceeded() {
        let mut server = setup_mock_server().await;

        // All attempts fail
        let mock = server
            .mock("GET", "/test")
            .with_status(500)
            .expect(3) // MAX_RETRIES
            .create_async()
            .await;

        let client = ApiClient::with_base_url("test_token".to_string(), server.url());

        #[derive(Debug, serde::Deserialize)]
        struct TestResponse {
            status: String,
        }

        let response: CoreResult<TestResponse> = client.get("/test").await;
        assert!(response.is_err());

        mock.assert_async().await;
    }
}
