//! Fetching a .torrent from an indexer URL (research R6). The URL is never logged: indexer
//! URLs carry API keys.

use putio_sync_core::net_guard::{self, GuardedResolver, Reach};
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
    #[error("address not allowed")]
    Blocked,
}

/// A client whose redirects stop at a `magnet:` URL and after [`MAX_REDIRECTS`] hops, and that
/// does not reach loopback, link-local or cloud metadata addresses (the private LAN stays
/// open, since indexers live there). `PUTIO_SYNC_ALLOW_LOCAL_FETCH=1` also lets it reach
/// loopback and link-local addresses, for an indexer on this machine.
pub fn client() -> reqwest::Client {
    client_with(Reach::or_opt_in(Reach::Lan))
}

/// [`client`] for an explicit `reach`. Every name is resolved by a guarded resolver and
/// every redirect hop to a literal address is checked, so the hop is as restricted as the
/// first request.
pub fn client_with(reach: Reach) -> reqwest::Client {
    let policy = redirect::Policy::custom(move |attempt| {
        if attempt.url().scheme() == "magnet" {
            attempt.stop()
        } else if attempt.previous().len() > MAX_REDIRECTS {
            attempt.error(FetchError::Redirects)
        } else if net_guard::check_url(attempt.url(), reach).is_err() {
            attempt.error(FetchError::Blocked)
        } else {
            attempt.follow()
        }
    });
    reqwest::Client::builder()
        .redirect(policy)
        .dns_resolver(std::sync::Arc::new(GuardedResolver::new(reach)))
        .build()
        .unwrap_or_default()
}

/// Fetch `url` with a [`client`], giving up at `deadline`. Bodies over 10 MB fail.
pub async fn fetch(
    http: &reqwest::Client,
    url: &str,
    deadline: Instant,
) -> Result<Fetched, FetchError> {
    fetch_with(http, url, deadline, Reach::or_opt_in(Reach::Lan)).await
}

/// [`fetch`] for an explicit `reach`, which should be the one `http` was built with
/// ([`client_with`]); it checks a literal address in `url`, which no resolver sees
pub async fn fetch_with(
    http: &reqwest::Client,
    url: &str,
    deadline: Instant,
    reach: Reach,
) -> Result<Fetched, FetchError> {
    let result = tokio::time::timeout_at(deadline, get(http, url, reach)).await;
    let result = result.unwrap_or(Err(FetchError::Timeout));
    if let Err(e) = &result {
        warn!(reason = %e, "Could not fetch the .torrent URL");
    }
    result
}

async fn get(http: &reqwest::Client, url: &str, reach: Reach) -> Result<Fetched, FetchError> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(FetchError::Scheme);
    }
    if let Ok(parsed) = reqwest::Url::parse(url) {
        if net_guard::check_url(&parsed, reach).is_err() {
            return Err(FetchError::Blocked);
        }
    }
    let mut response = http.get(url).send().await.map_err(|e| {
        // The redirect policy and the resolver say why in the error's chain
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&e);
        while let Some(cause) = source {
            if cause.downcast_ref::<FetchError>() == Some(&FetchError::Blocked)
                || cause.to_string().contains("address not allowed")
            {
                return FetchError::Blocked;
            }
            source = cause.source();
        }
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
