//! Fetching a .torrent from an indexer URL (research R6). The URL is never logged: indexer
//! URLs carry API keys.

use reqwest::header::LOCATION;
use reqwest::redirect;
use thiserror::Error;
use tokio::time::Instant;
use tracing::warn;

/// Largest .torrent fetched from a URL
pub const MAX_TORRENT_BYTES: usize = 10 * 1024 * 1024;

/// Redirects followed before giving up
pub const MAX_REDIRECTS: usize = 5;

/// What an indexer URL gave
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// The body, which may or may not be a .torrent
    Torrent(Vec<u8>),
    /// The URL redirected to a magnet link
    Magnet(String),
}

/// Why a fetch failed. The text is safe to log: it never holds the URL.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FetchError {
    #[error("not an http(s) URL")]
    Scheme,
    #[error("request failed")]
    Request,
    #[error("bad status")]
    Status,
    #[error("too large")]
    TooLarge,
    #[error("timed out")]
    Timeout,
    #[error("too many redirects")]
    Redirects,
}

/// A client whose redirects stop at a `magnet:` URL and after [`MAX_REDIRECTS`] hops
pub fn client() -> reqwest::Client {
    let policy = redirect::Policy::custom(|attempt| {
        if attempt.url().scheme() == "magnet" {
            attempt.stop()
        } else if attempt.previous().len() > MAX_REDIRECTS {
            attempt.error(FetchError::Redirects)
        } else {
            attempt.follow()
        }
    });
    reqwest::Client::builder()
        .redirect(policy)
        .build()
        .unwrap_or_default()
}

/// Fetch `url` with a [`client`], giving up at `deadline`. Bodies over 10 MB fail.
pub async fn fetch(
    http: &reqwest::Client,
    url: &str,
    deadline: Instant,
) -> Result<Fetched, FetchError> {
    let result = tokio::time::timeout_at(deadline, get(http, url)).await;
    let result = result.unwrap_or(Err(FetchError::Timeout));
    if let Err(e) = &result {
        warn!(reason = %e, "Could not fetch the .torrent URL");
    }
    result
}

async fn get(http: &reqwest::Client, url: &str) -> Result<Fetched, FetchError> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(FetchError::Scheme);
    }
    let mut response = http.get(url).send().await.map_err(|e| {
        if e.is_redirect() {
            FetchError::Redirects
        } else {
            FetchError::Request
        }
    })?;
    if response.status().is_redirection() {
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|v| v.to_str().ok())
            .filter(|l| l.starts_with("magnet:"));
        return match location {
            Some(magnet) => Ok(Fetched::Magnet(magnet.to_string())),
            None => Err(FetchError::Status),
        };
    }
    if !response.status().is_success() {
        return Err(FetchError::Status);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| FetchError::Request)? {
        if bytes.len() + chunk.len() > MAX_TORRENT_BYTES {
            return Err(FetchError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(Fetched::Torrent(bytes))
}
