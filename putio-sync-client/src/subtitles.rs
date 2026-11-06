//! Put.io subtitle endpoints (feature 025): list what Put.io offers for a file and download
//! one as SRT text.
//!
//! Parsing is lenient on purpose: only `key` is required, unknown fields are ignored, so an
//! API change degrades to fewer candidates instead of a hard failure.

use reqwest::header;
use serde::Deserialize;
use tracing::debug;

use crate::client::PutioApiClient;
use crate::error::{ClientError, ClientResult};

/// One subtitle Put.io offers for a file
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct PutioSubtitle {
    /// Key used to download it
    pub key: String,
    /// `srt`, usually
    #[serde(default)]
    pub format: String,
    /// Language name, e.g. `English`
    #[serde(default)]
    pub language: String,
    /// Language code, e.g. `en`
    #[serde(default)]
    pub language_code: String,
    /// Display name, which carries forced and SDH markers
    #[serde(default)]
    pub name: String,
    /// Where Put.io got it: `opensubtitles`, a folder file, an embedded track
    #[serde(default)]
    pub source: String,
    /// Direct URL, used when the key path does not work
    #[serde(default)]
    pub url: String,
}

/// `GET /files/{id}/subtitles`
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct SubtitleList {
    /// Key of Put.io's default pick, if any
    #[serde(default)]
    pub default: Option<String>,
    /// Everything on offer
    #[serde(default)]
    pub subtitles: Vec<PutioSubtitle>,
}

impl ClientError {
    /// Put.io answered 402: the account's plan does not include the feature
    pub fn is_payment_required(&self) -> bool {
        matches!(self, ClientError::ApiError(msg) if msg.starts_with("API error (402"))
    }

    /// Put.io cannot give subtitles for this file: gone (404), no scope (401) or plan (402)
    pub fn is_subtitles_unavailable(&self) -> bool {
        self.is_not_found() || self.is_unauthorized() || self.is_payment_required()
    }
}

/// Percent-encode everything but unreserved characters
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

impl PutioApiClient {
    /// The scheme and host of the base URL, so a subtitle `url` is only followed on Put.io
    /// (or on the mock server a test points the client at)
    fn origin(&self) -> String {
        let base = &self.base_url;
        let after = base.find("://").map(|i| i + 3).unwrap_or(0);
        match base[after..].find('/') {
            Some(i) => base[..after + i].to_string(),
            None => base.clone(),
        }
    }

    fn url_is_ours(&self, url: &str) -> bool {
        let origin = self.origin();
        let ours = |o: &str| url == o || url.starts_with(&format!("{o}/"));
        ours(&origin) || ours("https://api.put.io")
    }

    /// List the subtitles Put.io has for a file. `languages` is sent as a CSV when not empty.
    ///
    /// A 401, 402 or 404 means Put.io cannot help for this file: check
    /// [`ClientError::is_subtitles_unavailable`].
    pub async fn list_subtitles(
        &self,
        file_id: i64,
        languages: &[String],
    ) -> ClientResult<SubtitleList> {
        let mut path = format!("/files/{file_id}/subtitles");
        if !languages.is_empty() {
            path.push_str(&format!("?languages={}", encode(&languages.join(","))));
        }
        self.get::<SubtitleList>(&path).await
    }

    /// Download one subtitle as raw bytes (the text may not be UTF-8; the caller decodes it).
    ///
    /// Tries `GET /files/{id}/subtitles/{key}?format=srt`; when that is a 404 and `url` (the
    /// list item's own URL) points at Put.io, tries that with the same auth header.
    pub async fn download_subtitle(
        &self,
        file_id: i64,
        key: &str,
        url: Option<&str>,
    ) -> ClientResult<Vec<u8>> {
        let path = format!("/files/{file_id}/subtitles/{}?format=srt", encode(key));
        match self.get_bytes(&format!("{}{}", self.base_url, path)).await {
            Ok(body) => Ok(body),
            Err(e) if e.is_not_found() => match url.filter(|u| !u.is_empty()) {
                Some(u) if self.url_is_ours(u) => {
                    debug!("subtitle key path not found, trying the list item's url");
                    self.get_bytes(u).await
                }
                _ => Err(e),
            },
            Err(e) => Err(e),
        }
    }

    /// GET an absolute URL with the auth header and return the body
    async fn get_bytes(&self, url: &str) -> ClientResult<Vec<u8>> {
        let auth = self.get_auth_header()?;
        let request = self
            .client
            .get(url)
            .header(header::AUTHORIZATION, auth)
            .build()
            .map_err(ClientError::HttpError)?;
        let response = self.send_with_retry(request).await?;
        let bytes = response.bytes().await.map_err(ClientError::HttpError)?;
        Ok(bytes.to_vec())
    }
}
