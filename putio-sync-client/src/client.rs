//! HTTP client for Put.io API

use crate::auth::{AuthConfig, AuthMethod};
use crate::error::{ClientError, ClientResult};
use base64::{engine::general_purpose::STANDARD, Engine};
use putio_sync_telemetry::{api, ApiOp, ApiOutcome, RetryReason};
use reqwest::{header, Client, Request, Response, StatusCode};
use serde::de::DeserializeOwned;
use std::time::{Duration, Instant};
use tracing::{debug, trace, warn};

/// Default base URL for Put.io API
pub const DEFAULT_BASE_URL: &str = "https://api.put.io/v2";

/// Put.io takes file uploads on a separate host
pub const DEFAULT_UPLOAD_URL: &str = "https://upload.put.io/v2/files/upload";

/// The upload endpoint for `base_url`: the real upload host for the real API, otherwise
/// `<base_url>/files/upload`, so a mock or test server receives uploads too
fn upload_url_for(base_url: &str) -> String {
    if base_url.trim_end_matches('/') == DEFAULT_BASE_URL {
        DEFAULT_UPLOAD_URL.to_string()
    } else {
        format!("{base_url}/files/upload")
    }
}

/// Idle pooled connections are dropped before a middlebox can silently kill them.
/// On the NAS, the first request after a sync interval failed on a dead socket
/// about 250 times a day with the reqwest default of 90 s.
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(20);

/// TCP keepalive interval for API and download connections
const TCP_KEEPALIVE: Duration = Duration::from_secs(30);

/// Backoff policy for retryable failures (network errors, 5xx and 429)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts, including the first one
    pub max_attempts: u32,
    /// Delay before the first retry; doubled for each further retry
    pub initial_delay: Duration,
    /// Upper bound for a single delay
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 6,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(16),
        }
    }
}

impl RetryPolicy {
    /// Delay before retry number `retry` (1-based), with ±20% jitter
    fn delay(&self, retry: u32) -> Duration {
        let base = self
            .initial_delay
            .saturating_mul(1u32 << (retry - 1).min(16))
            .min(self.max_delay);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        // 0.8..=1.2
        let factor = 0.8 + f64::from(nanos % 401) / 1000.0;
        base.mul_f64(factor)
    }
}

/// Outcome class of one HTTP attempt. Every 4xx, 429 included, is a client error.
pub(crate) fn attempt_outcome(result: &reqwest::Result<Response>) -> ApiOutcome {
    match result {
        Ok(r) if r.status().is_success() => ApiOutcome::Success,
        Ok(r) if r.status().is_server_error() => ApiOutcome::ServerError,
        Ok(_) => ApiOutcome::ClientError,
        Err(_) => ApiOutcome::NetworkError,
    }
}

/// Join an error and all of its sources with `": "`.
///
/// reqwest's `Display` hides the cause ("error sending request for url"), so this is what
/// gets logged.
pub fn error_chain(err: &dyn std::error::Error) -> String {
    let mut out = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        // Wrappers like `ClientError::HttpError` already print their source.
        let text = cause.to_string();
        if !out.ends_with(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        source = cause.source();
    }
    out
}

/// Longest wait honoured from a `Retry-After` header
const MAX_RETRY_AFTER: Duration = Duration::from_secs(120);

/// How long a connection attempt for a download or upload may take
const BULK_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a download connection may sit silent (no response headers, no body bytes)
/// before the request fails. There is deliberately no total timeout: a large file takes
/// as long as it takes.
const DOWNLOAD_READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Parse a `Retry-After` value: delay-seconds or an HTTP-date, capped at
/// [`MAX_RETRY_AFTER`]. `None` when it is neither.
fn parse_retry_after(value: &str, now: chrono::DateTime<chrono::Utc>) -> Option<Duration> {
    let value = value.trim();
    let wait = if let Ok(seconds) = value.parse::<u64>() {
        Duration::from_secs(seconds)
    } else {
        let date = chrono::DateTime::parse_from_rfc2822(value).ok()?;
        (date.with_timezone(&chrono::Utc) - now)
            .to_std()
            .unwrap_or(Duration::ZERO)
    };
    Some(wait.min(MAX_RETRY_AFTER))
}

/// The client for file downloads, built once and reused: its connection pool survives
/// between files. Connects and reads time out, but there is no total timeout.
fn download_http_client() -> Client {
    Client::builder()
        .connect_timeout(BULK_CONNECT_TIMEOUT)
        .read_timeout(DOWNLOAD_READ_TIMEOUT)
        .pool_idle_timeout(POOL_IDLE_TIMEOUT)
        .tcp_keepalive(TCP_KEEPALIVE)
        .build()
        .expect("Failed to create download HTTP client")
}

/// The client for uploads: the 30 s total timeout of the API client would cut off a
/// large body, so only connecting is bounded
fn upload_http_client() -> Client {
    Client::builder()
        .connect_timeout(BULK_CONNECT_TIMEOUT)
        .pool_idle_timeout(POOL_IDLE_TIMEOUT)
        .tcp_keepalive(TCP_KEEPALIVE)
        .build()
        .expect("Failed to create upload HTTP client")
}

fn http_client(pool_idle_timeout: Duration) -> Client {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .pool_idle_timeout(pool_idle_timeout)
        .tcp_keepalive(TCP_KEEPALIVE)
        .build()
        .expect("Failed to create HTTP client")
}

/// Put.io API client
///
/// Provides HTTP client wrapper with authentication, retry logic,
/// and request/response logging.
///
/// Supports both OAuth token authentication (Bearer token) and
/// app-specific password authentication (HTTP Basic Auth).
pub struct PutioApiClient {
    pub(crate) client: Client,
    /// Shared by every download; see [`download_http_client`]
    download_client: Client,
    /// Shared by every upload; see [`upload_http_client`]
    pub(crate) upload_client: Client,
    pub(crate) base_url: String,
    pub(crate) upload_url: String,
    auth_method: AuthMethod,
    token: Option<String>,
    username: Option<String>,
    password: Option<String>,
    retry_policy: RetryPolicy,
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
        let client = http_client(POOL_IDLE_TIMEOUT);

        Self {
            client,
            download_client: download_http_client(),
            upload_client: upload_http_client(),
            upload_url: upload_url_for(&base_url),
            base_url,
            auth_method: auth_config.auth_method,
            token: auth_config.token,
            username: auth_config.username,
            password: auth_config.password,
            retry_policy: RetryPolicy::default(),
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
        let client = http_client(POOL_IDLE_TIMEOUT);

        Self {
            client,
            download_client: download_http_client(),
            upload_client: upload_http_client(),
            upload_url: upload_url_for(&base_url),
            base_url,
            auth_method: AuthMethod::Token,
            token: Some(token),
            username: None,
            password: None,
            retry_policy: RetryPolicy::default(),
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
        let client = http_client(POOL_IDLE_TIMEOUT);

        Self {
            client,
            download_client: download_http_client(),
            upload_client: upload_http_client(),
            upload_url: upload_url_for(&base_url),
            base_url,
            auth_method: AuthMethod::AppPassword,
            token: None,
            username: Some(username),
            password: Some(password),
            retry_policy: RetryPolicy::default(),
        }
    }

    /// Override the retry policy (tests use millisecond delays)
    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Rebuild the HTTP client with a different idle-connection timeout
    #[doc(hidden)]
    pub fn with_pool_idle_timeout(mut self, idle: Duration) -> Self {
        self.client = http_client(idle);
        self
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

    /// The HTTP client for file downloads: connect and read timeouts, no total timeout.
    /// Execute requests from [`Self::authenticated_download_request`] on this client.
    pub fn download_client(&self) -> &Client {
        &self.download_client
    }

    /// Create an authenticated request builder for downloads
    ///
    /// The request is built on the shared download client (see [`Self::download_client`]),
    /// which times out a connection that goes silent but not a long transfer.
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

        Ok(self
            .download_client
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

    /// Execute a POST that only reads (it has no effect to repeat), so it retries like a
    /// GET: on network errors, 5xx and 429
    pub(crate) async fn post_idempotent<T: DeserializeOwned, B: serde::Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> ClientResult<T> {
        let url = format!("{}{}", self.base_url, path);
        debug!("POST (read-only) request to: {}", url);

        let auth_header = self.get_auth_header()?;

        let request = self
            .client
            .post(&url)
            .header(header::AUTHORIZATION, auth_header)
            .header(header::CONTENT_TYPE, "application/json")
            .json(body)
            .build()
            .map_err(ClientError::HttpError)?;

        let response = self.send_attempts(request, true).await?;
        self.handle_success_response(response).await
    }

    /// Execute a request with exponential backoff retry logic
    ///
    /// GETs and HEADs retry on network errors, 5xx and 429 responses. Other methods
    /// change something on Put.io (delete, rename, move, create a folder, add a
    /// transfer), so a lost reply must not repeat them: they retry only when the request
    /// certainly never ran (a connect error, 429 or 503). A 429 or 503 with `Retry-After`
    /// waits for that long (at most two minutes) instead of the backoff delay.
    pub(crate) async fn execute_with_retry<T: DeserializeOwned>(
        &self,
        request: Request,
    ) -> ClientResult<T> {
        let response = self.send_with_retry(request).await?;
        self.handle_success_response(response).await
    }

    /// Execute a request with the same retry rules and return the successful response
    /// unread, for bodies that are not JSON. A non-success status becomes the usual
    /// `ApiError`.
    pub(crate) async fn send_with_retry(&self, request: Request) -> ClientResult<Response> {
        let idempotent = matches!(
            *request.method(),
            reqwest::Method::GET | reqwest::Method::HEAD
        );
        self.send_attempts(request, idempotent).await
    }

    async fn send_attempts(&self, request: Request, idempotent: bool) -> ClientResult<Response> {
        let max_attempts = self.retry_policy.max_attempts.max(1);
        let mut url = request.url().clone();
        url.set_query(None);
        let op = ApiOp::from_path(url.path());
        let mut attempt = 0;

        loop {
            attempt += 1;
            trace!("Attempt {} for {}", attempt, url);

            // Clone the request for retry attempts
            let req = request.try_clone().ok_or_else(|| {
                ClientError::InternalError("Cannot clone request for retry".to_string())
            })?;

            let started = Instant::now();
            let result = self.client.execute(req).await;
            api::record_attempt(op, attempt_outcome(&result), started.elapsed());

            let (delay, error, reason) = match result {
                Ok(response) => {
                    let status = response.status();
                    trace!("Response status: {}", status);

                    if status.is_success() {
                        return Ok(response);
                    }
                    let retryable = if idempotent {
                        status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS
                    } else {
                        status == StatusCode::TOO_MANY_REQUESTS
                            || status == StatusCode::SERVICE_UNAVAILABLE
                    };
                    if !retryable || attempt >= max_attempts {
                        return match self.handle_error_response::<()>(response).await {
                            Err(e) => Err(e),
                            Ok(()) => Err(ClientError::InternalError(
                                "error response handled as success".to_string(),
                            )),
                        };
                    }
                    let retry_after = response
                        .headers()
                        .get(header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| parse_retry_after(v, chrono::Utc::now()));
                    let delay = match retry_after {
                        Some(d)
                            if status == StatusCode::TOO_MANY_REQUESTS
                                || status == StatusCode::SERVICE_UNAVAILABLE =>
                        {
                            d
                        }
                        _ => self.retry_policy.delay(attempt),
                    };
                    let reason = if status == StatusCode::TOO_MANY_REQUESTS {
                        RetryReason::RateLimited
                    } else {
                        RetryReason::ServerError
                    };
                    (delay, format!("HTTP {}", status), reason)
                }
                Err(e) if attempt < max_attempts && (idempotent || e.is_connect()) => (
                    self.retry_policy.delay(attempt),
                    error_chain(&e),
                    RetryReason::Network,
                ),
                Err(e) => return Err(ClientError::HttpError(e)),
            };

            warn!(
                url = %url,
                attempt,
                max_attempts,
                delay_ms = delay.as_millis() as u64,
                error = %error,
                "Put.io request failed, retrying"
            );
            api::record_retry(reason);
            tokio::time::sleep(delay).await;
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
            StatusCode::UNAUTHORIZED => crate::error::UNAUTHORIZED_MESSAGE.to_string(),
            StatusCode::FORBIDDEN => "Access forbidden. Check your permissions.".to_string(),
            StatusCode::NOT_FOUND => crate::error::NOT_FOUND_MESSAGE.to_string(),
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

    /// Production clients upload to upload.put.io; custom base URLs upload there instead
    #[test]
    fn test_upload_url() {
        let token = || "test_token".to_string();
        let creds = || ("u".to_string(), "p".to_string());
        assert_eq!(PutioApiClient::new(token()).upload_url, DEFAULT_UPLOAD_URL);
        assert_eq!(
            PutioApiClient::with_credentials(creds().0, creds().1).upload_url,
            DEFAULT_UPLOAD_URL
        );
        assert_eq!(
            PutioApiClient::from_auth_config(AuthConfig::with_token(token())).upload_url,
            DEFAULT_UPLOAD_URL
        );
        assert_eq!(
            PutioApiClient::with_base_url(token(), DEFAULT_BASE_URL.to_string()).upload_url,
            DEFAULT_UPLOAD_URL
        );
        assert_eq!(
            PutioApiClient::with_base_url(token(), "http://127.0.0.1:9".to_string()).upload_url,
            "http://127.0.0.1:9/files/upload"
        );
        assert_eq!(
            PutioApiClient::with_credentials_and_base_url(
                creds().0,
                creds().1,
                "http://127.0.0.1:9".to_string()
            )
            .upload_url,
            "http://127.0.0.1:9/files/upload"
        );
        assert_eq!(
            PutioApiClient::from_auth_config_with_base_url(
                AuthConfig::with_token(token()),
                "http://127.0.0.1:9".to_string()
            )
            .upload_url,
            "http://127.0.0.1:9/files/upload"
        );
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

        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url())
            .with_retry_policy(fast_policy(3));

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
            .expect(6) // RetryPolicy::default().max_attempts
            .create_async()
            .await;

        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url())
            .with_retry_policy(fast_policy(RetryPolicy::default().max_attempts));

        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct TestResponse {
            status: String,
        }

        let response: ClientResult<TestResponse> = client.get("/test").await;
        assert!(response.is_err());

        mock.assert_async().await;
    }

    fn fast_policy(max_attempts: u32) -> RetryPolicy {
        RetryPolicy {
            max_attempts,
            initial_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
        }
    }

    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct StatusResponse {
        status: String,
    }

    #[tokio::test]
    async fn post_is_not_repeated_after_a_server_error() {
        let mut server = setup_mock_server().await;
        let mock = server
            .mock("POST", "/files/delete")
            .with_status(500)
            .expect(1)
            .create_async()
            .await;

        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url())
            .with_retry_policy(fast_policy(3));

        let response: ClientResult<StatusResponse> = client
            .post("/files/delete", &serde_json::json!({"file_ids": "1"}))
            .await;
        assert!(response.is_err());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn post_is_repeated_after_503_and_429() {
        let mut server = setup_mock_server().await;
        let busy = server
            .mock("POST", "/files/move")
            .with_status(503)
            .expect(1)
            .create_async()
            .await;
        let limited = server
            .mock("POST", "/files/move")
            .with_status(429)
            .expect(1)
            .create_async()
            .await;
        let ok = server
            .mock("POST", "/files/move")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status": "ok"}"#)
            .expect(1)
            .create_async()
            .await;

        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url())
            .with_retry_policy(fast_policy(4));

        let response: ClientResult<StatusResponse> =
            client.post("/files/move", &serde_json::json!({})).await;
        assert!(response.is_ok(), "{response:?}");
        busy.assert_async().await;
        limited.assert_async().await;
        ok.assert_async().await;
    }

    #[test]
    fn retry_after_takes_seconds_or_a_date_and_is_capped() {
        let now = chrono::DateTime::parse_from_rfc2822("Wed, 21 Oct 2015 07:28:00 GMT")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(parse_retry_after("7", now), Some(Duration::from_secs(7)));
        assert_eq!(
            parse_retry_after("Wed, 21 Oct 2015 07:28:30 GMT", now),
            Some(Duration::from_secs(30))
        );
        // A date in the past is no wait at all
        assert_eq!(
            parse_retry_after("Wed, 21 Oct 2015 07:00:00 GMT", now),
            Some(Duration::ZERO)
        );
        assert_eq!(parse_retry_after("86400", now), Some(MAX_RETRY_AFTER));
        assert_eq!(parse_retry_after("soon", now), None);
    }

    #[tokio::test]
    async fn test_five_server_errors_then_success() {
        let mut server = setup_mock_server().await;
        let fail = server
            .mock("GET", "/test")
            .with_status(503)
            .expect(4)
            .create_async()
            .await;
        let ok = server
            .mock("GET", "/test")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status": "ok"}"#)
            .expect(1)
            .create_async()
            .await;

        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url())
            .with_retry_policy(fast_policy(RetryPolicy::default().max_attempts));

        let response: ClientResult<StatusResponse> = client.get("/test").await;
        assert!(response.is_ok(), "{response:?}");
        fail.assert_async().await;
        ok.assert_async().await;
    }

    #[tokio::test]
    async fn test_429_honours_retry_after() {
        let mut server = setup_mock_server().await;
        let limited = server
            .mock("GET", "/test")
            .with_status(429)
            .with_header("retry-after", "1")
            .expect(1)
            .create_async()
            .await;
        let ok = server
            .mock("GET", "/test")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status": "ok"}"#)
            .expect(1)
            .create_async()
            .await;

        let client = PutioApiClient::with_base_url("test_token".to_string(), server.url())
            .with_retry_policy(fast_policy(3));

        let started = std::time::Instant::now();
        let response: ClientResult<StatusResponse> = client.get("/test").await;
        assert!(response.is_ok(), "{response:?}");
        assert!(started.elapsed() >= Duration::from_secs(1));
        limited.assert_async().await;
        ok.assert_async().await;
    }

    #[tokio::test]
    async fn test_network_failure_error_chain_has_cause_not_token() {
        // Bind then drop a listener so the port refuses connections.
        let addr = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let client = PutioApiClient::with_base_url(
            "secret_token_value".to_string(),
            format!("http://{addr}"),
        )
        .with_retry_policy(fast_policy(2));

        let err = client.get::<StatusResponse>("/test").await.unwrap_err();
        let chain = error_chain(&err);
        assert!(
            chain.to_lowercase().contains("refused") || chain.contains("connect"),
            "missing cause: {chain}"
        );
        assert!(!chain.contains("secret_token_value"));
    }

    #[test]
    fn test_error_chain_joins_sources() {
        #[derive(Debug)]
        struct Outer(std::io::Error);
        impl std::fmt::Display for Outer {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "outer")
            }
        }
        impl std::error::Error for Outer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let err = Outer(std::io::Error::other("inner"));
        assert_eq!(error_chain(&err), "outer: inner");
    }

    #[test]
    fn test_retry_delay_doubles_with_jitter_and_caps() {
        let policy = RetryPolicy::default();
        let first = policy.delay(1);
        assert!(first >= Duration::from_millis(800) && first <= Duration::from_millis(1200));
        let third = policy.delay(3);
        assert!(third >= Duration::from_millis(3200) && third <= Duration::from_millis(4800));
        assert!(policy.delay(10) <= Duration::from_secs(16).mul_f64(1.2));
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
