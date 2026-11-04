//! Put.io's device link (feature 013): putio-sync shows a short code, the user enters it
//! at put.io/link, and Put.io hands over an OAuth token. Needs only a public app ID.
//!
//! `GET /oauth2/oob/code?app_id=<id>` returns the code; `GET /oauth2/oob/code/<code>`
//! returns `oauth_token`, null until the user has linked.

use crate::client::DEFAULT_BASE_URL;
use crate::error::{ClientError, ClientResult};
use serde::Deserialize;
use std::fmt;
use std::time::Duration;

/// Where the user enters the code
pub const LINK_URL: &str = "https://put.io/link";

/// A code to show the user
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OobCode {
    /// The code the user types at put.io/link
    pub code: String,
    /// A QR code image for the same link, when Put.io sends one
    #[serde(default)]
    pub qr_code_url: Option<String>,
}

/// Where a code stands
#[derive(Clone, PartialEq, Eq)]
pub enum OobCheck {
    /// The user has not linked yet
    Pending,
    /// Linked: the OAuth token
    Linked(String),
}

impl fmt::Debug for OobCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OobCheck::Pending => f.write_str("Pending"),
            OobCheck::Linked(_) => f.write_str("Linked(<token>)"),
        }
    }
}

#[derive(Deserialize)]
struct CheckResponse {
    #[serde(default)]
    oauth_token: Option<String>,
}

/// Talks to Put.io's device-link endpoints
#[derive(Debug, Clone)]
pub struct DeviceLink {
    http: reqwest::Client,
    base_url: String,
}

impl Default for DeviceLink {
    fn default() -> Self {
        Self::with_base_url(DEFAULT_BASE_URL)
    }
}

impl DeviceLink {
    /// Against `base_url` (`https://api.put.io/v2`, or a mock in tests)
    pub fn with_base_url(base_url: &str) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Ask Put.io for a new code for `app_id`
    pub async fn request_code(&self, app_id: &str) -> ClientResult<OobCode> {
        let response = self
            .http
            .get(format!("{}/oauth2/oob/code", self.base_url))
            .query(&[("app_id", app_id)])
            .send()
            .await
            .map_err(|e| ClientError::network_error(&format!("Put.io unreachable: {e}")))?;
        let status = response.status();
        if !status.is_success() {
            return Err(ClientError::ApiError(format!(
                "Put.io refused a link code ({status})"
            )));
        }
        response
            .json()
            .await
            .map_err(|e| ClientError::SerializationError(e.to_string()))
    }

    /// See whether the user has entered `code` yet. A 4xx means the code is no longer
    /// valid.
    pub async fn check_code(&self, code: &str) -> ClientResult<OobCheck> {
        let response = self
            .http
            .get(format!("{}/oauth2/oob/code/{}", self.base_url, code))
            .send()
            .await
            .map_err(|e| ClientError::network_error(&format!("Put.io unreachable: {e}")))?;
        let status = response.status();
        if status.is_client_error() {
            return Err(ClientError::AuthError("The link code expired".to_string()));
        }
        if !status.is_success() {
            return Err(ClientError::ApiError(format!(
                "Put.io could not check the code ({status})"
            )));
        }
        let body: CheckResponse = response
            .json()
            .await
            .map_err(|e| ClientError::SerializationError(e.to_string()))?;
        Ok(match body.oauth_token.filter(|t| !t.is_empty()) {
            Some(token) => OobCheck::Linked(token),
            None => OobCheck::Pending,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn requests_and_checks_a_code() {
        let mut server = mockito::Server::new_async().await;
        let code = server
            .mock("GET", "/oauth2/oob/code")
            .match_query(mockito::Matcher::UrlEncoded("app_id".into(), "42".into()))
            .with_body(r#"{"status":"OK","code":"AB12CD","qr_code_url":"https://x/qr"}"#)
            .create_async()
            .await;
        let link = DeviceLink::with_base_url(&server.url());
        let got = link.request_code("42").await.unwrap();
        assert_eq!(got.code, "AB12CD");
        assert_eq!(got.qr_code_url.as_deref(), Some("https://x/qr"));
        code.assert_async().await;

        let pending = server
            .mock("GET", "/oauth2/oob/code/AB12CD")
            .with_body(r#"{"status":"OK","oauth_token":null}"#)
            .create_async()
            .await;
        assert_eq!(link.check_code("AB12CD").await.unwrap(), OobCheck::Pending);
        pending.remove_async().await;

        server
            .mock("GET", "/oauth2/oob/code/AB12CD")
            .with_body(r#"{"status":"OK","oauth_token":"tok"}"#)
            .create_async()
            .await;
        let linked = link.check_code("AB12CD").await.unwrap();
        assert_eq!(linked, OobCheck::Linked("tok".to_string()));
        assert_eq!(format!("{linked:?}"), "Linked(<token>)");
    }

    #[tokio::test]
    async fn expired_and_refused_codes_are_errors() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/oauth2/oob/code/OLD")
            .with_status(404)
            .create_async()
            .await;
        server
            .mock("GET", "/oauth2/oob/code")
            .match_query(mockito::Matcher::Any)
            .with_status(400)
            .create_async()
            .await;
        let link = DeviceLink::with_base_url(&server.url());
        assert!(matches!(
            link.check_code("OLD").await,
            Err(ClientError::AuthError(_))
        ));
        assert!(link.request_code("bad").await.is_err());
    }
}
