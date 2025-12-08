//! HTTP client for Put.io API

use crate::auth::{AuthConfig, AuthMethod};
use crate::error::{ClientError, ClientResult};
use base64::{engine::general_purpose::STANDARD, Engine};
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
///
/// Supports both OAuth token authentication (Bearer token) and
/// app-specific password authentication (HTTP Basic Auth).
pub struct PutioApiClient {
    pub(crate) client: Client,
    pub(crate) base_url: String,
    auth_method: AuthMethod,
    token: Option<String>,
    username: Option<String>,
    password: Option<String>,
}

impl PutioApiClient {
    /// Create a new API client from an AuthConfig
    ///
    /// # Arguments
    ///
    /// * `auth_config` - Authentication configuration (token or credentials)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_client::{PutioApiClient, AuthConfig};
    ///
    /// let auth = AuthConfig::with_token("my_oauth_token".to_string());
    /// let client = PutioApiClient::from_auth_config(auth);
    /// ```
    pub fn from_auth_config(auth_config: AuthConfig) -> Self {
        Self::from_auth_config_with_base_url(auth_config, DEFAULT_BASE_URL.to_string())
    }

    /// Create a new API client from an AuthConfig with a custom base URL
    ///
    /// # Arguments
    ///
    /// * `auth_config` - Authentication configuration (token or credentials)
    /// * `base_url` - Base URL for the API (e.g., <https://api.put.io/v2>)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_client::{PutioApiClient, AuthConfig};
    ///
    /// let auth = AuthConfig::with_token("my_oauth_token".to_string());
    /// let client = PutioApiClient::from_auth_config_with_base_url(
    ///     auth,
    ///     "https://test.api.put.io/v2".to_string()
    /// );
    /// ```
    pub fn from_auth_config_with_base_url(auth_config: AuthConfig, base_url: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url,
            auth_method: auth_config.auth_method,
            token: auth_config.token,
            username: auth_config.username,
            password: auth_config.password,
        }
    }

    /// Create a new API client with OAuth token authentication
    ///
    /// # Arguments
    ///
    /// * `token` - OAuth2 token for Put.io API authentication
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_client::PutioApiClient;
    ///
    /// let client = PutioApiClient::new("my_oauth_token".to_string());
    /// ```
    pub fn new(token: String) -> Self {
        Self::with_base_url(token, DEFAULT_BASE_URL.to_string())
    }

    /// Create a new API client with OAuth token and custom base URL
    ///
    /// # Arguments
    ///
    /// * `token` - OAuth2 token for Put.io API authentication
    /// * `base_url` - Base URL for the API (e.g., <https://api.put.io/v2>)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_client::PutioApiClient;
    ///
    /// let client = PutioApiClient::with_base_url(
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
            auth_method: AuthMethod::Token,
            token: Some(token),
            username: None,
            password: None,
        }
    }

    /// Create a new API client with app-specific password authentication
    ///
    /// # Arguments
    ///
    /// * `username` - Put.io username or email
    /// * `password` - App-specific password
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_client::PutioApiClient;
    ///
    /// let client = PutioApiClient::with_credentials(
    ///     "user@example.com".to_string(),
    ///     "app_password".to_string()
    /// );
    /// ```
    pub fn with_credentials(username: String, password: String) -> Self {
        Self::with_credentials_and_base_url(username, password, DEFAULT_BASE_URL.to_string())
    }

    /// Create a new API client with app-specific password and custom base URL
    ///
    /// # Arguments
    ///
    /// * `username` - Put.io username or email
    /// * `password` - App-specific password
    /// * `base_url` - Base URL for the API (e.g., <https://api.put.io/v2>)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_client::PutioApiClient;
    ///
    /// let client = PutioApiClient::with_credentials_and_base_url(
    ///     "user@example.com".to_string(),
    ///     "app_password".to_string(),
    ///     "https://test.api.put.io/v2".to_string()
    /// );
    /// ```
    pub fn with_credentials_and_base_url(
        username: String,
        password: String,
        base_url: String,
    ) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url,
            auth_method: AuthMethod::AppPassword,
            token: None,
            username: Some(username),
            password: Some(password),
        }
    }

    /// Get a reference to the underlying HTTP client
    ///
    /// This can be used for direct HTTP operations like downloads
    /// that need the raw client.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_client::PutioApiClient;
    ///
    /// let client = PutioApiClient::new("my_oauth_token".to_string());
    /// let http_client = client.http_client();
    /// ```
    pub fn http_client(&self) -> &Client {
        &self.client
    }

    /// Create an authenticated GET request builder for a download URL
    ///
    /// This adds the necessary authentication headers to the request.
    ///
    /// # Arguments
    ///
    /// * `url` - The download URL
    ///
    /// # Returns
    ///
    /// A request builder with authentication headers set
    pub fn authenticated_request(&self, url: &str) -> ClientResult<reqwest::RequestBuilder> {
        let auth_header = self.get_auth_header()?;
        Ok(self.client.get(url).header("Authorization", auth_header))
    }

    /// Create an authenticated request builder for downloads with extended timeout
    ///
    /// This creates a request with a much longer timeout suitable for file downloads.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to download from
    ///
    /// # Returns
    ///
    /// A request builder with authentication headers and extended timeout
    pub fn authenticated_download_request(
        &self,
        url: &str,
    ) -> ClientResult<reqwest::RequestBuilder> {
        let auth_header = self.get_auth_header()?;
        debug!("Creating authenticated download request for URL: {}", url);
        debug!("Auth method: {:?}", self.auth_method);
        // Note: Not logging auth header value for security

        // Create a client with extended timeout for downloads (10 minutes)
        let download_client = Client::builder()
            .timeout(Duration::from_secs(600)) // 10 minutes
            .build()
            .map_err(ClientError::HttpError)?;

        Ok(download_client
            .get(url)
            .header("Authorization", auth_header))
    }

    /// Create an authenticated HEAD request builder for checking file metadata
    ///
    /// This adds the necessary authentication headers to the request.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL to check
    ///
    /// # Returns
    ///
    /// A request builder with authentication headers set
    pub fn authenticated_head_request(&self, url: &str) -> ClientResult<reqwest::RequestBuilder> {
        let auth_header = self.get_auth_header()?;
        Ok(self.client.head(url).header("Authorization", auth_header))
    }

    /// Generate the appropriate Authorization header based on auth method
    ///
    /// # Returns
    ///
    /// Authorization header value (either "Bearer <token>" or "Basic <base64>")
    pub fn get_auth_header(&self) -> ClientResult<String> {
        match self.auth_method {
            AuthMethod::Token => {
                let token = self.token.as_ref().ok_or_else(|| {
                    ClientError::AuthError("Token is required for Token authentication".to_string())
                })?;
                Ok(format!("Bearer {}", token))
            }
            AuthMethod::AppPassword => {
                let username = self.username.as_ref().ok_or_else(|| {
                    ClientError::AuthError(
                        "Username is required for AppPassword authentication".to_string(),
                    )
                })?;
                let password = self.password.as_ref().ok_or_else(|| {
                    ClientError::AuthError(
                        "Password is required for AppPassword authentication".to_string(),
                    )
                })?;

                // Encode credentials as base64
                let credentials = format!("{}:{}", username, password);
                let encoded = STANDARD.encode(credentials.as_bytes());
                Ok(format!("Basic {}", encoded))
            }
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
    pub(crate) async fn get<T: DeserializeOwned>(&self, path: &str) -> ClientResult<T> {
        let url = format!("{}{}", self.base_url, path);
        debug!("GET request to: {}", url);

        let auth_header = self.get_auth_header()?;

        let request = self
            .client
            .get(&url)
            .header(header::AUTHORIZATION, auth_header)
            .build()
            .map_err(ClientError::HttpError)?;

        self.execute_with_retry(request).await
    }

    /// Execute a POST request with retry logic
    ///
    /// # Arguments
    ///
    /// * `path` - API path (e.g., "/files/delete")
    /// * `body` - Request body to serialize as JSON
    ///
    /// # Returns
    ///
    /// Response body deserialized as type T
    pub(crate) async fn post<T: DeserializeOwned, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> ClientResult<T> {
        let url = format!("{}{}", self.base_url, path);
        debug!("POST request to: {}", url);

        let auth_header = self.get_auth_header()?;

        let request = self
            .client
            .post(&url)
            .header(header::AUTHORIZATION, auth_header)
            .header(header::CONTENT_TYPE, "application/json")
            .json(body)
            .build()
            .map_err(ClientError::HttpError)?;

        self.execute_with_retry(request).await
    }

    /// Execute a request with exponential backoff retry logic
    ///
    /// Retries on network errors and 5xx server errors.
    pub(crate) async fn execute_with_retry<T: DeserializeOwned>(
        &self,
        request: Request,
    ) -> ClientResult<T> {
        let mut attempts = 0;
        let mut delay_ms = INITIAL_RETRY_DELAY_MS;

        loop {
            attempts += 1;
            trace!("Attempt {} for {}", attempts, request.url());

            // Clone the request for retry attempts
            let req = request.try_clone().ok_or_else(|| {
                ClientError::InternalError("Cannot clone request for retry".to_string())
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
                    return Err(ClientError::HttpError(e));
                }
            }

            // Wait before retry with exponential backoff
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            delay_ms *= 2; // Exponential backoff
        }
    }

    /// Handle successful API response
    pub(crate) async fn handle_success_response<T: DeserializeOwned>(
        &self,
        response: Response,
    ) -> ClientResult<T> {
        let body = response.text().await.map_err(ClientError::HttpError)?;
        trace!("Response body: {}", body);

        serde_json::from_str(&body).map_err(|e| {
            debug!("Response body that failed to parse: {}", body);
            ClientError::SerializationError(format!("Failed to parse response: {}", e))
        })
    }

    /// Handle error API response
    pub(crate) async fn handle_error_response<T: DeserializeOwned>(
        &self,
        response: Response,
    ) -> ClientResult<T> {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unable to read error body".to_string());

        trace!("Error response body: {}", body);

        let error_msg = match status {
            StatusCode::UNAUTHORIZED => {
                "Authentication failed. Invalid or expired token.".to_string()
            }
            StatusCode::FORBIDDEN => "Access forbidden. Check your permissions.".to_string(),
            StatusCode::NOT_FOUND => "Resource not found.".to_string(),
            StatusCode::TOO_MANY_REQUESTS => {
                "Rate limit exceeded. Please try again later.".to_string()
            }
            _ => format!("API error ({}): {}", status, body),
        };

        Err(ClientError::ApiError(error_msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthConfig;
    use mockito::{Server, ServerGuard};

    async fn setup_mock_server() -> ServerGuard {
        Server::new_async().await
    }

    #[tokio::test]
    async fn test_new_client() {
        let client = PutioApiClient::new("test_token".to_string());
        assert_eq!(client.base_url, DEFAULT_BASE_URL);
        assert_eq!(client.auth_method, AuthMethod::Token);
        assert_eq!(client.token.as_ref().unwrap(), "test_token");
        assert!(client.username.is_none());
        assert!(client.password.is_none());
    }

    #[tokio::test]
    async fn test_with_base_url() {
        let custom_url = "https://custom.api.put.io/v2";
        let client =
            PutioApiClient::with_base_url("test_token".to_string(), custom_url.to_string());
        assert_eq!(client.base_url, custom_url);
        assert_eq!(client.auth_method, AuthMethod::Token);
        assert_eq!(client.token.as_ref().unwrap(), "test_token");
    }

    #[tokio::test]
    async fn test_with_credentials() {
        let client = PutioApiClient::with_credentials(
            "user@example.com".to_string(),
            "app_password".to_string(),
        );
        assert_eq!(client.base_url, DEFAULT_BASE_URL);
        assert_eq!(client.auth_method, AuthMethod::AppPassword);
        assert_eq!(client.username.as_ref().unwrap(), "user@example.com");
        assert_eq!(client.password.as_ref().unwrap(), "app_password");
        assert!(client.token.is_none());
    }

    #[tokio::test]
    async fn test_from_auth_config_token() {
        let auth = AuthConfig::with_token("test_token".to_string());
        let client = PutioApiClient::from_auth_config(auth);
        assert_eq!(client.base_url, DEFAULT_BASE_URL);
        assert_eq!(client.auth_method, AuthMethod::Token);
        assert_eq!(client.token.as_ref().unwrap(), "test_token");
    }

    #[tokio::test]
    async fn test_from_auth_config_credentials() {
        let auth = AuthConfig::with_app_password(
            "user@example.com".to_string(),
            "app_password".to_string(),
        );
        let client = PutioApiClient::from_auth_config(auth);
        assert_eq!(client.base_url, DEFAULT_BASE_URL);
        assert_eq!(client.auth_method, AuthMethod::AppPassword);
        assert_eq!(client.username.as_ref().unwrap(), "user@example.com");
        assert_eq!(client.password.as_ref().unwrap(), "app_password");
    }

    #[tokio::test]
    async fn test_get_auth_header_token() {
        let client = PutioApiClient::new("test_token".to_string());
        let header = client.get_auth_header().unwrap();
        assert_eq!(header, "Bearer test_token");
    }

    #[tokio::test]
    async fn test_get_auth_header_credentials() {
        let client = PutioApiClient::with_credentials(
            "user@example.com".to_string(),
            "password123".to_string(),
        );
        let header = client.get_auth_header().unwrap();

        // Verify Basic auth header format
        assert!(header.starts_with("Basic "));

        // Decode and verify
        let encoded = header.strip_prefix("Basic ").unwrap();
        let decoded = STANDARD.decode(encoded).unwrap();
        let credentials = String::from_utf8(decoded).unwrap();
        assert_eq!(credentials, "user@example.com:password123");
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

        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url());

        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct TestResponse {
            status: String,
        }

        let response: ClientResult<TestResponse> = client.get("/test").await;
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

        let client = PutioApiClient::with_base_url("bad_token".to_string(), server.url());

        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct TestResponse {
            status: String,
        }

        let response: ClientResult<TestResponse> = client.get("/test").await;
        assert!(response.is_err());
        assert!(matches!(response.unwrap_err(), ClientError::ApiError(_)));

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

        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url());

        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct TestResponse {
            status: String,
        }

        let response: ClientResult<TestResponse> = client.get("/test").await;
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

        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url());

        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct TestResponse {
            status: String,
        }

        let response: ClientResult<TestResponse> = client.get("/test").await;
        assert!(response.is_err());

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_successful_request_with_basic_auth() {
        let mut server = setup_mock_server().await;

        // Encode username:password as base64
        let credentials = "user@example.com:app_password";
        let encoded = STANDARD.encode(credentials.as_bytes());
        let expected_auth = format!("Basic {}", encoded);

        let mock = server
            .mock("GET", "/test")
            .match_header("authorization", expected_auth.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status": "ok"}"#)
            .create_async()
            .await;

        let client = PutioApiClient::with_credentials_and_base_url(
            "user@example.com".to_string(),
            "app_password".to_string(),
            server.url(),
        );

        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct TestResponse {
            status: String,
        }

        let response: ClientResult<TestResponse> = client.get("/test").await;
        assert!(response.is_ok());
        assert_eq!(response.unwrap().status, "ok");

        mock.assert_async().await;
    }
}
