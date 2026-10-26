//! A single, unretried look at the account for the doctor (feature 010)
//!
//! One `GET /account/info` answers several questions at once: is the token accepted, how
//! full is the account, how long does Put.io take to answer, what time does its clock
//! say, and how much of the rate limit is left.

use crate::client::PutioApiClient;
use crate::error::{ClientError, ClientResult};
use crate::models::{AccountInfo, AccountInfoResponse, ApiResponse};
use chrono::{DateTime, Utc};
use reqwest::{header, StatusCode};
use std::time::{Duration, Instant};

/// What one probe of `/account/info` found
#[derive(Debug)]
pub struct AccountProbe {
    /// The account, or why it could not be read
    pub result: ClientResult<AccountInfo>,
    /// HTTP status, when Put.io answered at all
    pub http_status: Option<StatusCode>,
    /// Time from sending the request to reading the whole answer
    pub elapsed: Duration,
    /// Put.io's clock, from the `Date` header
    pub server_date: Option<DateTime<Utc>>,
    /// `(limit, remaining)` from `X-RateLimit-Limit` / `X-RateLimit-Remaining`, when sent
    pub rate_limit: Option<(u64, u64)>,
}

impl AccountProbe {
    /// Put.io refused the token
    pub fn unauthorized(&self) -> bool {
        matches!(
            self.http_status,
            Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        )
    }

    /// Put.io answered 429
    pub fn rate_limited(&self) -> bool {
        self.http_status == Some(StatusCode::TOO_MANY_REQUESTS)
    }

    /// Put.io could not be reached (no HTTP answer)
    pub fn unreachable(&self) -> bool {
        self.http_status.is_none()
    }
}

fn header_u64(headers: &header::HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.trim().parse().ok()
}

impl PutioApiClient {
    /// Fetch the account once, without retries, keeping the response metadata
    ///
    /// A 429 or a 5xx is reported as it is instead of being retried away, so the doctor
    /// sees what Put.io really answered.
    pub async fn probe_account(&self) -> AccountProbe {
        let started = Instant::now();
        let request = match self.authenticated_request(&format!("{}/account/info", self.base_url)) {
            Ok(request) => request,
            Err(e) => {
                return AccountProbe {
                    result: Err(e),
                    http_status: None,
                    elapsed: started.elapsed(),
                    server_date: None,
                    rate_limit: None,
                }
            }
        };
        let response = match request.send().await {
            Ok(response) => response,
            Err(e) => {
                return AccountProbe {
                    result: Err(ClientError::HttpError(e)),
                    http_status: None,
                    elapsed: started.elapsed(),
                    server_date: None,
                    rate_limit: None,
                }
            }
        };

        let status = response.status();
        let headers = response.headers();
        let server_date = headers
            .get(header::DATE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| DateTime::parse_from_rfc2822(v).ok())
            .map(|d| d.with_timezone(&Utc));
        let rate_limit = header_u64(headers, "x-ratelimit-limit")
            .zip(header_u64(headers, "x-ratelimit-remaining"));

        let result = if status.is_success() {
            self.handle_success_response::<ApiResponse<AccountInfoResponse>>(response)
                .await
                .map(|r| r.data.info)
        } else {
            self.handle_error_response(response).await
        };
        AccountProbe {
            result,
            http_status: Some(status),
            elapsed: started.elapsed(),
            server_date,
            rate_limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    const INFO: &str = r#"{"status":"OK","info":{"user_id":1,"username":"alice",
        "mail":"a@example.com","account_active":true,
        "disk":{"size":1000,"used":930,"avail":70}}}"#;

    #[tokio::test]
    async fn ok_probe_reads_account_date_and_rate_limit() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/account/info")
            .with_status(200)
            .with_header("date", "Mon, 26 Oct 2026 12:00:00 GMT")
            .with_header("x-ratelimit-limit", "100")
            .with_header("x-ratelimit-remaining", "7")
            .with_body(INFO)
            .create_async()
            .await;
        let client = PutioApiClient::with_base_url("t".into(), server.url());

        let probe = client.probe_account().await;
        let info = probe.result.as_ref().unwrap();
        assert_eq!(info.username, "alice");
        assert_eq!(info.disk.used, 930);
        assert_eq!(probe.http_status, Some(StatusCode::OK));
        assert_eq!(
            probe.server_date.unwrap().to_rfc3339(),
            "2026-10-26T12:00:00+00:00"
        );
        assert_eq!(probe.rate_limit, Some((100, 7)));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn unauthorized_is_reported_not_retried() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/account/info")
            .with_status(401)
            .expect(1)
            .create_async()
            .await;
        let client = PutioApiClient::with_base_url("t".into(), server.url());

        let probe = client.probe_account().await;
        assert!(probe.unauthorized());
        assert!(probe.result.is_err());
        assert!(probe.rate_limit.is_none());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn rate_limited_is_seen_once() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/account/info")
            .with_status(429)
            .expect(1)
            .create_async()
            .await;
        let client = PutioApiClient::with_base_url("t".into(), server.url());

        let probe = client.probe_account().await;
        assert!(probe.rate_limited());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn unreachable_has_no_status() {
        // Nothing listens on port 9 of localhost
        let client = PutioApiClient::with_base_url("t".into(), "http://127.0.0.1:9".into());
        let probe = client.probe_account().await;
        assert!(probe.unreachable());
        assert!(probe.server_date.is_none());
    }
}
