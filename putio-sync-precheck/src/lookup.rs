//! Looking a magnet's .torrent up by info hash in public caches (research R5). A lookup
//! sends only the hash in the path: no cookies, no referer.

use std::time::Duration;

use putio_sync_telemetry::precheck as metrics;
use putio_sync_telemetry::LookupResult;
use reqwest::header::{ACCEPT_ENCODING, USER_AGENT};
use reqwest::StatusCode;
use tokio::time::Instant;
use tracing::debug;

use crate::metainfo::{self, Metainfo};

/// Largest cache response read
pub const MAX_CACHE_BYTES: usize = 2 * 1024 * 1024;

/// What a lookup gave
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// A .torrent whose info hash is the magnet's
    Found { meta: Metainfo, cache: String },
    /// No usable .torrent
    Missed(Missed),
}

/// Why a lookup found nothing: the best result across the caches
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missed {
    /// `magnet_lookup = false`; no request was made
    Off,
    /// The caches were asked
    Failed(LookupResult),
}

impl Missed {
    /// As stored in `release_checks.lookup`
    pub fn as_str(self) -> &'static str {
        match self {
            Missed::Off => "off",
            Missed::Failed(r) => r.as_str(),
        }
    }
}

impl Lookup {
    /// As stored in `release_checks.lookup`
    pub fn as_str(&self) -> &'static str {
        match self {
            Lookup::Found { .. } => LookupResult::Found.as_str(),
            Lookup::Missed(m) => m.as_str(),
        }
    }
}

/// A client for cache requests: no referer on redirects, no cookie store
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .referer(false)
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .unwrap_or_default()
}

/// Try `caches` in order for `hash` (lower-case hex), within `limit` in total. A cache
/// whose .torrent has another info hash is skipped.
pub async fn lookup(
    http: &reqwest::Client,
    hash: &str,
    caches: &[String],
    limit: Duration,
) -> Lookup {
    let started = Instant::now();
    let deadline = started + limit;
    let mut best: Option<LookupResult> = None;
    for template in caches {
        let url = template
            .replace("{HASH}", &hash.to_ascii_uppercase())
            .replace("{hash}", &hash.to_ascii_lowercase());
        let host = reqwest::Url::parse(&url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        let result = match tokio::time::timeout_at(deadline, ask(http, &url)).await {
            Ok(Ok(meta)) if meta.hash.eq_ignore_ascii_case(hash) => {
                metrics::cache_lookup(&host, LookupResult::Found);
                metrics::lookup_time(started.elapsed());
                debug!(hash, cache = %host, "Found the .torrent in a cache");
                return Lookup::Found { meta, cache: host };
            }
            Ok(Ok(_)) => LookupResult::WrongHash,
            Ok(Err(result)) => result,
            Err(_) => LookupResult::Timeout,
        };
        metrics::cache_lookup(&host, result);
        debug!(hash, cache = %host, result = result.as_str(), "Cache lookup missed");
        best = Some(match best {
            Some(b) if rank(b) >= rank(result) => b,
            _ => result,
        });
        if result == LookupResult::Timeout {
            break;
        }
    }
    metrics::lookup_time(started.elapsed());
    Lookup::Missed(Missed::Failed(best.unwrap_or(LookupResult::NotFound)))
}

/// Which miss says the most: a wrong hash, then a clean 404, then an error, then a timeout
fn rank(result: LookupResult) -> u8 {
    match result {
        LookupResult::Found => 4,
        LookupResult::WrongHash => 3,
        LookupResult::NotFound => 2,
        LookupResult::Error => 1,
        LookupResult::Timeout => 0,
    }
}

async fn ask(http: &reqwest::Client, url: &str) -> Result<Metainfo, LookupResult> {
    let mut response = http
        .get(url)
        .header(ACCEPT_ENCODING, "identity")
        .header(USER_AGENT, "putio-sync")
        .send()
        .await
        .map_err(|_| LookupResult::Error)?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(LookupResult::NotFound);
    }
    if !response.status().is_success() {
        return Err(LookupResult::Error);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| LookupResult::Error)? {
        if bytes.len() + chunk.len() > MAX_CACHE_BYTES {
            return Err(LookupResult::Error);
        }
        bytes.extend_from_slice(&chunk);
    }
    // gzip despite `identity`, or an HTML page
    if bytes.starts_with(&[0x1f, 0x8b]) || bytes.trim_ascii_start().starts_with(b"<") {
        return Err(LookupResult::Error);
    }
    metainfo::parse(&bytes).ok_or(LookupResult::Error)
}
