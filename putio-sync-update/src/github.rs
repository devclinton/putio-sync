//! The GitHub Releases API (research R1): one request a day, with the ETag of the last
//! answer so an unchanged list costs nothing against the rate limit.

use crate::version::Version;
use chrono::{DateTime, Utc};
use reqwest::{header, StatusCode};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The project's releases
pub const API: &str = "https://api.github.com/repos/devclinton/putio-sync";

/// The longest a check may take
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// Where releases are read from. Debug builds take `PUTIO_SYNC_UPDATE_API`, so the
/// quickstart can point them at a fake release; release builds always use [`API`].
pub fn api_base() -> String {
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var("PUTIO_SYNC_UPDATE_API") {
        return url.trim_end_matches('/').to_string();
    }
    API.to_string()
}

/// The HTTP client for the releases API and its assets. It sends `User-Agent:
/// putio-sync/<version>` and nothing else that identifies the install (FR-003).
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("putio-sync/", env!("CARGO_PKG_VERSION")))
        .timeout(TIMEOUT)
        .build()
        .expect("a client with a user agent and a timeout builds")
}

/// A published release newer than the running one
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub version: Version,
    pub published_at: Option<DateTime<Utc>>,
    pub html_url: String,
    /// The release notes: the CHANGELOG section, as `scripts/changelog-section.sh` writes it
    pub body: String,
    pub assets: Vec<Asset>,
}

impl ReleaseInfo {
    /// The asset called `name`
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// A file attached to a release
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "AssetRepr")]
pub struct Asset {
    pub name: String,
    /// `browser_download_url`
    pub url: String,
    pub size: u64,
}

/// What `update_state.releases_json` holds: a bare name before 055 US3, then the whole asset
#[derive(Deserialize)]
#[serde(untagged)]
enum AssetRepr {
    Name(String),
    Full {
        name: String,
        #[serde(default)]
        url: String,
        #[serde(default)]
        size: u64,
    },
}

impl From<AssetRepr> for Asset {
    fn from(repr: AssetRepr) -> Self {
        match repr {
            AssetRepr::Name(name) => Self {
                name,
                url: String::new(),
                size: 0,
            },
            AssetRepr::Full { name, url, size } => Self { name, url, size },
        }
    }
}

/// What a check found
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    /// `304`: the list is the one the ETag names
    NotModified,
    /// The releases newer than the running version, newest first
    Releases {
        etag: Option<String>,
        releases: Vec<ReleaseInfo>,
    },
}

/// Why a check failed
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// `403` or `429` with no requests left
    RateLimited,
    /// Any other answer than `200` or `304`
    Status(u16),
    /// No answer: DNS, connection, TLS or the timeout
    Network(String),
    /// An answer that isn't a releases list
    Parse(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RateLimited => write!(f, "GitHub rate limit reached"),
            Self::Status(code) => write!(f, "GitHub answered {code}"),
            Self::Network(e) => write!(f, "could not reach GitHub: {e}"),
            Self::Parse(e) => write!(f, "unexpected answer from GitHub: {e}"),
        }
    }
}

impl std::error::Error for FetchError {}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    published_at: Option<DateTime<Utc>>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    #[serde(default)]
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Fetch `<base_url>/releases` and keep the published releases newer than `current`.
/// Drafts, pre-releases and tags that aren't `vX.Y.Z` are skipped.
pub async fn fetch_releases(
    client: &reqwest::Client,
    base_url: &str,
    etag: Option<&str>,
    current: Version,
) -> Result<Fetched, FetchError> {
    let mut request = client
        .get(format!("{base_url}/releases?per_page=30"))
        .header(header::ACCEPT, "application/vnd.github+json");
    if let Some(etag) = etag {
        request = request.header(header::IF_NONE_MATCH, etag);
    }
    let response = request
        .send()
        .await
        .map_err(|e| FetchError::Network(e.without_url().to_string()))?;

    let status = response.status();
    if status == StatusCode::NOT_MODIFIED {
        return Ok(Fetched::NotModified);
    }
    if status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS {
        let remaining = response
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok());
        if status == StatusCode::TOO_MANY_REQUESTS || remaining == Some("0") {
            return Err(FetchError::RateLimited);
        }
    }
    if status != StatusCode::OK {
        return Err(FetchError::Status(status.as_u16()));
    }

    let etag = response
        .headers()
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let list: Vec<ApiRelease> = response
        .json()
        .await
        .map_err(|e| FetchError::Parse(e.without_url().to_string()))?;

    let mut releases: Vec<ReleaseInfo> = list
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            let version: Version = r.tag_name.parse().ok()?;
            (version > current).then(|| ReleaseInfo {
                version,
                published_at: r.published_at,
                html_url: r.html_url,
                body: r.body.unwrap_or_default(),
                assets: r
                    .assets
                    .into_iter()
                    .map(|a| Asset {
                        name: a.name,
                        url: a.browser_download_url,
                        size: a.size,
                    })
                    .collect(),
            })
        })
        .collect();
    releases.sort_by_key(|r| std::cmp::Reverse(r.version));
    Ok(Fetched::Releases { etag, releases })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    const LIST: &str = r####"[
        {"tag_name": "v0.5.0-rc1", "prerelease": true, "draft": false,
         "published_at": "2026-11-20T10:00:00Z", "html_url": "u5", "body": "", "assets": []},
        {"tag_name": "v0.4.0", "prerelease": false, "draft": false,
         "published_at": "2026-11-15T10:00:00Z", "html_url": "https://example/v0.4.0",
         "body": "### Added\n\n- A thing", "assets": [{"name": "SHA256SUMS", "size": 9,
            "browser_download_url": "https://example/d/SHA256SUMS"}]},
        {"tag_name": "v0.6.0", "prerelease": false, "draft": true,
         "published_at": null, "html_url": "u6", "body": null, "assets": []},
        {"tag_name": "nightly", "prerelease": false, "draft": false,
         "published_at": "2026-11-01T10:00:00Z", "html_url": "un", "body": "", "assets": []},
        {"tag_name": "v0.3.1", "prerelease": false, "draft": false,
         "published_at": "2026-11-10T10:00:00Z", "html_url": "u31", "body": null, "assets": []},
        {"tag_name": "v0.3.0", "prerelease": false, "draft": false,
         "published_at": "2026-11-01T10:00:00Z", "html_url": "u30", "body": "", "assets": []}
    ]"####;

    #[tokio::test]
    async fn keeps_newer_published_releases_newest_first() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/releases")
            .match_query(Matcher::Any)
            .match_header(
                "user-agent",
                concat!("putio-sync/", env!("CARGO_PKG_VERSION")),
            )
            .match_header("authorization", Matcher::Missing)
            .match_header("cookie", Matcher::Missing)
            .match_header("if-none-match", Matcher::Missing)
            .with_header("etag", "\"abc\"")
            .with_body(LIST)
            .create_async()
            .await;

        let got = fetch_releases(&client(), &server.url(), None, v("0.3.0"))
            .await
            .unwrap();
        mock.assert_async().await;
        let Fetched::Releases { etag, releases } = got else {
            panic!("{got:?}")
        };
        assert_eq!(etag.as_deref(), Some("\"abc\""));
        let versions: Vec<String> = releases.iter().map(|r| r.version.to_string()).collect();
        assert_eq!(versions, ["0.4.0", "0.3.1"]);
        assert_eq!(releases[0].html_url, "https://example/v0.4.0");
        assert_eq!(releases[0].body, "### Added\n\n- A thing");
        assert_eq!(
            releases[0].assets,
            [Asset {
                name: "SHA256SUMS".into(),
                url: "https://example/d/SHA256SUMS".into(),
                size: 9,
            }]
        );
        assert_eq!(releases[1].body, "");
        assert_eq!(
            releases[0].published_at.unwrap().to_rfc3339(),
            "2026-11-15T10:00:00+00:00"
        );
    }

    #[tokio::test]
    async fn nothing_newer_is_an_empty_list() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/releases")
            .match_query(Matcher::Any)
            .with_body(LIST)
            .create_async()
            .await;
        let got = fetch_releases(&client(), &server.url(), None, v("0.4.0"))
            .await
            .unwrap();
        assert_eq!(
            got,
            Fetched::Releases {
                etag: None,
                releases: vec![]
            }
        );
    }

    #[tokio::test]
    async fn sends_the_etag_and_maps_304() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/releases")
            .match_query(Matcher::Any)
            .match_header("if-none-match", "\"abc\"")
            .with_status(304)
            .create_async()
            .await;
        let got = fetch_releases(&client(), &server.url(), Some("\"abc\""), v("0.3.0"))
            .await
            .unwrap();
        mock.assert_async().await;
        assert_eq!(got, Fetched::NotModified);
    }

    #[tokio::test]
    async fn rate_limits_and_other_errors() {
        let mut server = mockito::Server::new_async().await;
        let cases = [
            (403, Some("0"), FetchError::RateLimited),
            (429, None, FetchError::RateLimited),
            (403, Some("12"), FetchError::Status(403)),
            (500, None, FetchError::Status(500)),
        ];
        for (status, remaining, want) in cases {
            let mut mock = server
                .mock("GET", "/releases")
                .match_query(Matcher::Any)
                .with_status(status);
            if let Some(r) = remaining {
                mock = mock.with_header("x-ratelimit-remaining", r);
            }
            let mock = mock.create_async().await;
            let got = fetch_releases(&client(), &server.url(), None, v("0.3.0")).await;
            assert_eq!(got, Err(want), "{status} {remaining:?}");
            mock.remove_async().await;
        }

        let mock = server
            .mock("GET", "/releases")
            .match_query(Matcher::Any)
            .with_body("{\"message\": \"not a list\"}")
            .create_async()
            .await;
        let got = fetch_releases(&client(), &server.url(), None, v("0.3.0")).await;
        assert!(matches!(got, Err(FetchError::Parse(_))), "{got:?}");
        mock.remove_async().await;
    }

    #[tokio::test]
    async fn no_server_is_a_network_error() {
        let got = fetch_releases(&client(), "http://127.0.0.1:9", None, v("0.3.0")).await;
        assert!(matches!(got, Err(FetchError::Network(_))), "{got:?}");
    }

    #[test]
    fn the_client_times_out_after_30_seconds() {
        assert_eq!(TIMEOUT, Duration::from_secs(30));
    }

    #[test]
    fn stored_asset_names_still_load() {
        let old = r#"[{"version": "0.4.0", "published_at": null, "html_url": "u", "body": "",
            "assets": ["SHA256SUMS"]}]"#;
        let releases: Vec<ReleaseInfo> = serde_json::from_str(old).unwrap();
        assert_eq!(releases[0].asset("SHA256SUMS").unwrap().url, "");
        let again: Vec<ReleaseInfo> =
            serde_json::from_str(&serde_json::to_string(&releases).unwrap()).unwrap();
        assert_eq!(again, releases);
    }
}
