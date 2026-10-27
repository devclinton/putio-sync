//! The few Radarr/Sonarr v3 API calls putio-sync makes: the connection test, finding an
//! item in the *arr queue by hash, and removing it with blocklisting (research R9)

use std::time::Duration;

use putio_sync_core::config::ArrKind;
use serde::Deserialize;
use thiserror::Error;

const TIMEOUT: Duration = Duration::from_secs(10);

/// Why an *arr call failed. The messages never contain the API key.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ArrApiError {
    /// Connection refused, DNS, timeout, TLS
    #[error("unreachable: {0}")]
    Unreachable(String),
    /// 401 or 403: the API key is wrong
    #[error("unauthorized: check the API key")]
    Unauthorized,
    /// It answered, but not like the expected *arr app
    #[error("not {expected}: {detail}")]
    NotArr {
        /// The app name we expected
        expected: &'static str,
        /// What we got instead
        detail: String,
    },
}

impl ArrApiError {
    /// Short machine-readable kind for the API (`contracts/api.md`)
    pub fn kind(&self) -> &'static str {
        match self {
            ArrApiError::Unreachable(_) => "unreachable",
            ArrApiError::Unauthorized => "unauthorized",
            ArrApiError::NotArr { .. } => "not_arr",
        }
    }
}

/// `GET /api/v3/system/status`
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    /// `Radarr` or `Sonarr`
    pub app_name: String,
    /// The app's version
    pub version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueEntry {
    id: i64,
    #[serde(default)]
    download_id: Option<String>,
}

/// One *arr app's API
pub struct ArrApi {
    kind: ArrKind,
    url: String,
    api_key: String,
    http: reqwest::Client,
}

impl ArrApi {
    /// `url` is the app's base URL without a trailing slash
    pub fn new(kind: ArrKind, url: &str, api_key: &str, http: reqwest::Client) -> Self {
        Self {
            kind,
            url: url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            http,
        }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{}", self.url, path))
            .header("X-Api-Key", &self.api_key)
            .header("Accept", "application/json")
            .timeout(TIMEOUT)
    }

    fn not_arr(&self, detail: impl Into<String>) -> ArrApiError {
        ArrApiError::NotArr {
            expected: self.kind.app_name(),
            detail: detail.into(),
        }
    }

    async fn send(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, ArrApiError> {
        let response = builder.send().await.map_err(|e| {
            // Without the URL: it is in config, and the error adds nothing secret, but keep
            // messages short for the UI
            ArrApiError::Unreachable(e.without_url().to_string())
        })?;
        match response.status().as_u16() {
            401 | 403 => Err(ArrApiError::Unauthorized),
            s if !(200..300).contains(&s) => Err(self.not_arr(format!("HTTP {}", s))),
            _ => Ok(response),
        }
    }

    /// The connection test: reachable, key accepted, and the right app
    pub async fn system_status(&self) -> Result<SystemStatus, ArrApiError> {
        let response = self
            .send(self.request(reqwest::Method::GET, "/api/v3/system/status"))
            .await?;
        let status: SystemStatus = response
            .json()
            .await
            .map_err(|_| self.not_arr("unexpected response"))?;
        if !status.app_name.eq_ignore_ascii_case(self.kind.app_name()) {
            return Err(self.not_arr(format!("found {}", status.app_name)));
        }
        Ok(status)
    }

    /// The *arr's queue id of the item whose download id is `hash`
    pub async fn find_queue_id(&self, hash: &str) -> Result<Option<i64>, ArrApiError> {
        let response = self
            .send(self.request(reqwest::Method::GET, "/api/v3/queue/details"))
            .await?;
        let entries: Vec<QueueEntry> = response
            .json()
            .await
            .map_err(|_| self.not_arr("unexpected queue response"))?;
        Ok(entries
            .into_iter()
            .find(|e| {
                e.download_id
                    .as_deref()
                    .is_some_and(|d| d.eq_ignore_ascii_case(hash))
            })
            .map(|e| e.id))
    }

    /// Remove a queue item, blocklist the release and let the *arr search again
    pub async fn remove_and_blocklist(&self, queue_id: i64) -> Result<(), ArrApiError> {
        self.send(self.request(
            reqwest::Method::DELETE,
            &format!(
                "/api/v3/queue/{}?removeFromClient=true&blocklist=true",
                queue_id
            ),
        ))
        .await?;
        Ok(())
    }
}
