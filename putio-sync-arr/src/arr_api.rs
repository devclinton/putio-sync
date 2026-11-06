//! The few Radarr/Sonarr v3 API calls putio-sync makes: the connection test, finding an
//! item in the *arr queue by hash, removing it with blocklisting (feature 011 research R9),
//! and managing its download clients and checking its paths (feature 018,
//! `contracts/arr-api.md`)

use std::time::Duration;

use putio_sync_core::config::ArrKind;
use putio_sync_core::media::{imdb_digits, MediaInfo, MediaKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

const TIMEOUT: Duration = Duration::from_secs(10);
/// One download-client test, per candidate host (018 plan, Performance Goals)
const CLIENT_TEST_TIMEOUT: Duration = Duration::from_secs(5);

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
    /// No answer in time
    #[error("timed out")]
    TimedOut,
    /// HTTPS with a certificate this machine doesn't trust
    #[error("untrusted certificate")]
    UntrustedCertificate,
    /// An *arr older than the v3 API (R12)
    #[error("unsupported version {found}: version 3 or later is needed")]
    UnsupportedVersion {
        /// The version the *arr reported
        found: String,
    },
    /// The *arr's own download-client test failed (HTTP 400)
    #[error("the download-client test failed: {}", .messages.join("; "))]
    TestFailed {
        /// The *arr's validation messages
        messages: Vec<String>,
    },
    /// The *arr refused to save the download client (HTTP 400)
    #[error("refused: {}", .messages.join("; "))]
    Refused {
        /// The *arr's validation messages
        messages: Vec<String>,
    },
}

impl ArrApiError {
    /// Short machine-readable kind for the API (`contracts/api.md`, data-model "Failure
    /// reasons")
    pub fn kind(&self) -> &'static str {
        match self {
            ArrApiError::Unreachable(_) => "unreachable",
            ArrApiError::Unauthorized => "unauthorized",
            ArrApiError::NotArr { .. } => "not_arr",
            ArrApiError::TimedOut => "timed_out",
            ArrApiError::UntrustedCertificate => "untrusted_certificate",
            ArrApiError::UnsupportedVersion { .. } => "unsupported_version",
            ArrApiError::TestFailed { .. } => "arr_test_failed",
            ArrApiError::Refused { .. } => "arr_refused",
        }
    }

    fn from_transport(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            return ArrApiError::TimedOut;
        }
        let mut source: Option<&dyn std::error::Error> = Some(&e);
        while let Some(err) = source {
            if err.to_string().to_ascii_lowercase().contains("certificate") {
                return ArrApiError::UntrustedCertificate;
            }
            source = err.source();
        }
        // Without the URL: keep messages short for the UI
        ArrApiError::Unreachable(e.without_url().to_string())
    }
}

/// The messages of a Servarr 400 body, `[{propertyName, errorMessage}]`
fn validation_messages(body: &str) -> Vec<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Failure {
        #[serde(default)]
        error_message: String,
    }
    match serde_json::from_str::<Vec<Failure>>(body) {
        Ok(list) => list
            .into_iter()
            .map(|f| f.error_message)
            .filter(|m| !m.is_empty())
            .collect(),
        Err(_) if body.trim().is_empty() => vec![],
        Err(_) => vec![body.chars().take(300).collect()],
    }
}

/// One download client as the *arr returns it. The whole JSON object is kept, so a PUT sends
/// back every field putio-sync doesn't know about.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadClient(pub Value);

impl DownloadClient {
    /// The *arr's id; `None` for a schema template
    pub fn id(&self) -> Option<i64> {
        self.0
            .get("id")
            .and_then(Value::as_i64)
            .filter(|id| *id > 0)
    }

    /// The name shown in the *arr
    pub fn name(&self) -> &str {
        self.0.get("name").and_then(Value::as_str).unwrap_or("")
    }

    /// `Transmission`, `TorrentBlackhole`, …
    pub fn implementation(&self) -> &str {
        self.0
            .get("implementation")
            .and_then(Value::as_str)
            .unwrap_or("")
    }

    /// `enable` in the *arr
    pub fn enabled(&self) -> bool {
        self.0
            .get("enable")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// A top-level property
    pub fn set(&mut self, key: &str, value: Value) {
        if let Some(map) = self.0.as_object_mut() {
            map.insert(key.to_string(), value);
        }
    }

    /// The value of one of `fields[]`
    pub fn get_field(&self, name: &str) -> Option<&Value> {
        self.0
            .get("fields")?
            .as_array()?
            .iter()
            .find(|f| f.get("name").and_then(Value::as_str) == Some(name))?
            .get("value")
    }

    /// A string field, or `""`
    pub fn field_str(&self, name: &str) -> &str {
        self.get_field(name).and_then(Value::as_str).unwrap_or("")
    }

    /// Set one of `fields[]`, adding it when missing
    pub fn set_field(&mut self, name: &str, value: Value) {
        let Some(map) = self.0.as_object_mut() else {
            return;
        };
        let fields = map.entry("fields").or_insert_with(|| Value::Array(vec![]));
        if !fields.is_array() {
            *fields = Value::Array(vec![]);
        }
        let list = fields.as_array_mut().expect("just made an array");
        match list
            .iter_mut()
            .find(|f| f.get("name").and_then(Value::as_str) == Some(name))
        {
            Some(field) => {
                if let Some(f) = field.as_object_mut() {
                    f.insert("value".into(), value);
                }
            }
            None => list.push(serde_json::json!({"name": name, "value": value})),
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

impl SystemStatus {
    /// The major version, when it parses
    pub fn major(&self) -> Option<u32> {
        self.version.split('.').next()?.parse().ok()
    }
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

    /// Which *arr is at `url`, from its own `system/status` (feature 018). Fails with
    /// `not_arr` for anything but Radarr or Sonarr, and `unsupported_version` below v3.
    pub async fn detect(
        url: &str,
        api_key: &str,
        http: reqwest::Client,
    ) -> Result<(ArrApi, SystemStatus), ArrApiError> {
        let mut api = ArrApi::new(ArrKind::Radarr, url, api_key, http);
        let not_arr = |detail: String| ArrApiError::NotArr {
            expected: "Radarr or Sonarr",
            detail,
        };
        let response = api
            .send(api.request(reqwest::Method::GET, "/api/v3/system/status"))
            .await
            .map_err(|e| match e {
                ArrApiError::NotArr { detail, .. } => not_arr(detail),
                other => other,
            })?;
        let status: SystemStatus = response
            .json()
            .await
            .map_err(|_| not_arr("unexpected response".into()))?;
        api.kind = match status.app_name.to_ascii_lowercase().as_str() {
            "radarr" => ArrKind::Radarr,
            "sonarr" => ArrKind::Sonarr,
            _ => return Err(not_arr(format!("found {}", status.app_name))),
        };
        if status.major().is_some_and(|m| m < 3) {
            return Err(ArrApiError::UnsupportedVersion {
                found: status.version,
            });
        }
        Ok((api, status))
    }

    /// The kind this API talks to
    pub fn kind(&self) -> ArrKind {
        self.kind
    }

    /// The base URL, without a trailing slash
    pub fn url(&self) -> &str {
        &self.url
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
        let response = builder.send().await.map_err(ArrApiError::from_transport)?;
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
        if status.major().is_some_and(|m| m < 3) {
            return Err(ArrApiError::UnsupportedVersion {
                found: status.version,
            });
        }
        Ok(status)
    }

    /// Like `send`, but a 400 carries the *arr's validation messages
    async fn send_validated(
        &self,
        builder: reqwest::RequestBuilder,
        on_400: fn(Vec<String>) -> ArrApiError,
    ) -> Result<reqwest::Response, ArrApiError> {
        let response = builder.send().await.map_err(ArrApiError::from_transport)?;
        if response.status().as_u16() == 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(on_400(validation_messages(&body)));
        }
        match response.status().as_u16() {
            401 | 403 => Err(ArrApiError::Unauthorized),
            s if !(200..300).contains(&s) => Err(self.not_arr(format!("HTTP {}", s))),
            _ => Ok(response),
        }
    }

    async fn clients_from(
        &self,
        response: reqwest::Response,
    ) -> Result<Vec<DownloadClient>, ArrApiError> {
        let list: Vec<Value> = response
            .json()
            .await
            .map_err(|_| self.not_arr("unexpected download client response"))?;
        Ok(list.into_iter().map(DownloadClient).collect())
    }

    /// `GET /api/v3/downloadclient`
    pub async fn download_clients(&self) -> Result<Vec<DownloadClient>, ArrApiError> {
        let response = self
            .send(self.request(reqwest::Method::GET, "/api/v3/downloadclient"))
            .await?;
        self.clients_from(response).await
    }

    /// `GET /api/v3/downloadclient/schema`: one template per implementation
    pub async fn download_client_schema(&self) -> Result<Vec<DownloadClient>, ArrApiError> {
        let response = self
            .send(self.request(reqwest::Method::GET, "/api/v3/downloadclient/schema"))
            .await?;
        self.clients_from(response).await
    }

    /// `POST /api/v3/downloadclient/test`: the *arr connects to the client itself
    pub async fn test_download_client(&self, body: &DownloadClient) -> Result<(), ArrApiError> {
        self.send_validated(
            self.request(reqwest::Method::POST, "/api/v3/downloadclient/test")
                .timeout(CLIENT_TEST_TIMEOUT)
                .json(&body.0),
            |messages| ArrApiError::TestFailed { messages },
        )
        .await?;
        Ok(())
    }

    /// `POST /api/v3/downloadclient`. Returns the saved client, with its id.
    pub async fn create_download_client(
        &self,
        body: &DownloadClient,
    ) -> Result<DownloadClient, ArrApiError> {
        let response = self
            .send_validated(
                self.request(reqwest::Method::POST, "/api/v3/downloadclient")
                    .json(&body.0),
                |messages| ArrApiError::Refused { messages },
            )
            .await?;
        let saved: Value = response
            .json()
            .await
            .map_err(|_| self.not_arr("unexpected download client response"))?;
        Ok(DownloadClient(saved))
    }

    /// `PUT /api/v3/downloadclient/{id}`. `force_save` skips the *arr's own test.
    pub async fn update_download_client(
        &self,
        id: i64,
        body: &DownloadClient,
        force_save: bool,
    ) -> Result<(), ArrApiError> {
        let path = if force_save {
            format!("/api/v3/downloadclient/{}?forceSave=true", id)
        } else {
            format!("/api/v3/downloadclient/{}", id)
        };
        self.send_validated(
            self.request(reqwest::Method::PUT, &path).json(&body.0),
            |messages| ArrApiError::Refused { messages },
        )
        .await?;
        Ok(())
    }

    /// `DELETE /api/v3/downloadclient/{id}`
    pub async fn delete_download_client(&self, id: i64) -> Result<(), ArrApiError> {
        self.send(self.request(
            reqwest::Method::DELETE,
            &format!("/api/v3/downloadclient/{}", id),
        ))
        .await?;
        Ok(())
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

    /// What the *arr's history says the download `hash` is: the movie, or the series and its
    /// episodes (feature 050). An empty result means the history has no record of it.
    pub async fn history_by_download(&self, hash: &str) -> Result<MediaInfo, ArrApiError> {
        let include = match self.kind {
            ArrKind::Radarr => "includeMovie=true",
            ArrKind::Sonarr => "includeSeries=true&includeEpisode=true",
        };
        let response = self
            .send(self.request(
                reqwest::Method::GET,
                &format!(
                    "/api/v3/history?downloadId={}&pageSize=100&{include}",
                    hash.to_ascii_uppercase()
                ),
            ))
            .await?;
        let body: Value = response
            .json()
            .await
            .map_err(|_| self.not_arr("unexpected history response"))?;
        // Paged (`records`) in v3; a bare list in some versions
        let records = body
            .get("records")
            .and_then(Value::as_array)
            .or_else(|| body.as_array())
            .ok_or_else(|| self.not_arr("unexpected history response"))?;
        let mut info = MediaInfo::default();
        for record in records {
            let same = record
                .get("downloadId")
                .and_then(Value::as_str)
                .is_some_and(|d| d.eq_ignore_ascii_case(hash));
            if !same {
                continue;
            }
            match self.kind {
                ArrKind::Radarr => read_feature(&mut info, record.get("movie"), MediaKind::Movie),
                ArrKind::Sonarr => {
                    read_feature(&mut info, record.get("series"), MediaKind::Episode);
                    let episode = record.get("episode");
                    let number = |key: &str| {
                        episode
                            .and_then(|e| e.get(key))
                            .and_then(Value::as_u64)
                            .and_then(|n| u16::try_from(n).ok())
                    };
                    if let (Some(s), Some(e)) = (number("seasonNumber"), number("episodeNumber")) {
                        if !info.episodes.contains(&(s, e)) {
                            info.episodes.push((s, e));
                        }
                    }
                }
            }
        }
        info.episodes.sort_unstable();
        Ok(info)
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

/// One Remote Path Mapping: a client on `host` reporting `remote_path` is read at
/// `local_path` by the *arr
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all(deserialize = "camelCase"))]
pub struct RemotePathMapping {
    /// The *arr's id; `None` before it is created
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    /// The download client's host
    #[serde(default)]
    pub host: String,
    /// The path the client reports
    #[serde(default)]
    pub remote_path: String,
    /// The path the *arr reads instead
    #[serde(default)]
    pub local_path: String,
}

#[derive(Deserialize)]
struct Listing {
    #[serde(default)]
    directories: Vec<Folder>,
}

/// One folder in a filesystem listing
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Folder {
    /// Its name
    #[serde(default)]
    pub name: String,
    /// Its full path, in the *arr's own style
    #[serde(default)]
    pub path: String,
}

#[derive(Deserialize)]
struct RootFolder {
    #[serde(default)]
    path: String,
}

/// The body the *arr expects for a Remote Path Mapping
fn mapping_body(m: &RemotePathMapping) -> Value {
    let mut body = serde_json::json!({
        "host": m.host,
        "remotePath": m.remote_path,
        "localPath": m.local_path,
    });
    if let Some(id) = m.id {
        body["id"] = serde_json::json!(id);
    }
    body
}

impl ArrApi {
    /// `GET /api/v3/remotepathmapping`
    pub async fn remote_path_mappings(&self) -> Result<Vec<RemotePathMapping>, ArrApiError> {
        let response = self
            .send(self.request(reqwest::Method::GET, "/api/v3/remotepathmapping"))
            .await?;
        response
            .json()
            .await
            .map_err(|_| self.not_arr("unexpected remote path mapping response"))
    }

    /// The folder names in `path`, as the *arr sees its own filesystem
    pub async fn filesystem(&self, path: &str) -> Result<Vec<String>, ArrApiError> {
        Ok(self
            .folders(path)
            .await?
            .into_iter()
            .map(|d| d.name)
            .collect())
    }

    /// The folders in `path`, with their full paths (feature 049)
    pub async fn folders(&self, path: &str) -> Result<Vec<Folder>, ArrApiError> {
        let response = self
            .send(
                self.request(reqwest::Method::GET, "/api/v3/filesystem")
                    .query(&[
                        ("path", path),
                        ("allowFoldersWithoutTrailingSlashes", "true"),
                    ]),
            )
            .await?;
        let listing: Listing = response
            .json()
            .await
            .map_err(|_| self.not_arr("unexpected filesystem response"))?;
        Ok(listing.directories)
    }

    /// `GET /api/v3/rootfolder`: the paths of the *arr's library folders
    pub async fn root_folders(&self) -> Result<Vec<String>, ArrApiError> {
        let response = self
            .send(self.request(reqwest::Method::GET, "/api/v3/rootfolder"))
            .await?;
        let list: Vec<RootFolder> = response
            .json()
            .await
            .map_err(|_| self.not_arr("unexpected root folder response"))?;
        Ok(list.into_iter().map(|r| r.path).collect())
    }

    /// `POST /api/v3/remotepathmapping`. Returns the saved mapping, with its id.
    pub async fn create_remote_path_mapping(
        &self,
        mapping: &RemotePathMapping,
    ) -> Result<RemotePathMapping, ArrApiError> {
        let response = self
            .send_validated(
                self.request(reqwest::Method::POST, "/api/v3/remotepathmapping")
                    .json(&mapping_body(mapping)),
                |messages| ArrApiError::Refused { messages },
            )
            .await?;
        response
            .json()
            .await
            .map_err(|_| self.not_arr("unexpected remote path mapping response"))
    }

    /// `PUT /api/v3/remotepathmapping/{id}`
    pub async fn update_remote_path_mapping(
        &self,
        id: i64,
        mapping: &RemotePathMapping,
    ) -> Result<(), ArrApiError> {
        let mut body = mapping_body(mapping);
        body["id"] = serde_json::json!(id);
        self.send_validated(
            self.request(
                reqwest::Method::PUT,
                &format!("/api/v3/remotepathmapping/{}", id),
            )
            .json(&body),
            |messages| ArrApiError::Refused { messages },
        )
        .await?;
        Ok(())
    }

    /// `DELETE /api/v3/remotepathmapping/{id}`
    pub async fn delete_remote_path_mapping(&self, id: i64) -> Result<(), ArrApiError> {
        self.send(self.request(
            reqwest::Method::DELETE,
            &format!("/api/v3/remotepathmapping/{}", id),
        ))
        .await?;
        Ok(())
    }

    /// `GET /api/v3/config/{section}`, the whole body (feature 049)
    pub async fn config(&self, section: &str) -> Result<Value, ArrApiError> {
        let response = self
            .send(self.request(reqwest::Method::GET, &format!("/api/v3/config/{}", section)))
            .await?;
        response
            .json()
            .await
            .map_err(|_| self.not_arr("unexpected config response"))
    }

    /// `PUT /api/v3/config/{section}/{id}` with the whole body
    pub async fn put_config(&self, section: &str, body: &Value) -> Result<(), ArrApiError> {
        let id = body.get("id").and_then(Value::as_i64).unwrap_or(1);
        self.send_validated(
            self.request(
                reqwest::Method::PUT,
                &format!("/api/v3/config/{}/{}", section, id),
            )
            .json(body),
            |messages| ArrApiError::Refused { messages },
        )
        .await?;
        Ok(())
    }
}

/// Fill `info` from a history record's `movie` or `series` object
fn read_feature(info: &mut MediaInfo, feature: Option<&Value>, kind: MediaKind) {
    let Some(f) = feature else { return };
    info.kind = Some(kind);
    if info.title.is_none() {
        info.title = f.get("title").and_then(Value::as_str).map(str::to_string);
    }
    if info.year.is_none() {
        info.year = f
            .get("year")
            .and_then(Value::as_u64)
            .and_then(|y| u16::try_from(y).ok())
            .filter(|y| *y > 0);
    }
    if info.imdb_id.is_none() {
        info.imdb_id = f
            .get("imdbId")
            .and_then(Value::as_str)
            .and_then(imdb_digits);
    }
    if info.tmdb_id.is_none() {
        info.tmdb_id = f.get("tmdbId").and_then(Value::as_u64).filter(|id| *id > 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn mappings_and_folders_are_read() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v3/remotepathmapping")
            .match_header("x-api-key", KEY)
            .with_body(
                json!([{"id": 1, "host": "putio-sync", "remotePath": "/downloads/",
                        "localPath": "/data/downloads/"}])
                .to_string(),
            )
            .create_async()
            .await;
        server
            .mock("GET", "/api/v3/filesystem")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("path".into(), "/data".into()),
                mockito::Matcher::UrlEncoded(
                    "allowFoldersWithoutTrailingSlashes".into(),
                    "true".into(),
                ),
            ]))
            .with_body(
                json!({"parent": "/", "directories": [
                    {"type": "folder", "name": "downloads", "path": "/data/downloads"},
                    {"type": "folder", "name": "media", "path": "/data/media"}],
                    "files": []})
                .to_string(),
            )
            .create_async()
            .await;
        let api = api(&server);
        assert_eq!(
            api.remote_path_mappings().await.unwrap(),
            vec![RemotePathMapping {
                id: Some(1),
                host: "putio-sync".into(),
                remote_path: "/downloads/".into(),
                local_path: "/data/downloads/".into(),
            }]
        );
        assert_eq!(
            api.filesystem("/data").await.unwrap(),
            vec!["downloads", "media"]
        );
    }

    const KEY: &str = "secret-api-key-123";

    #[tokio::test]
    async fn mappings_are_created_updated_and_deleted() {
        let mut server = mockito::Server::new_async().await;
        let create = server
            .mock("POST", "/api/v3/remotepathmapping")
            .match_header("x-api-key", KEY)
            .match_body(mockito::Matcher::Json(json!({"host": "putio-sync",
                "remotePath": "/downloads/", "localPath": "/data/downloads/"})))
            .with_status(201)
            .with_body(
                r#"{"id": 5, "host": "putio-sync", "remotePath": "/downloads/",
                "localPath": "/data/downloads/"}"#,
            )
            .create_async()
            .await;
        let update = server
            .mock("PUT", "/api/v3/remotepathmapping/5")
            .match_body(mockito::Matcher::PartialJson(
                json!({"id": 5, "localPath": "/mnt/dl/"}),
            ))
            .create_async()
            .await;
        let delete = server
            .mock("DELETE", "/api/v3/remotepathmapping/5")
            .create_async()
            .await;
        server
            .mock("GET", "/api/v3/rootfolder")
            .with_body(r#"[{"id": 1, "path": "/data/media/tv/"}]"#)
            .create_async()
            .await;
        let api = api(&server);
        let mut m = RemotePathMapping {
            id: None,
            host: "putio-sync".into(),
            remote_path: "/downloads/".into(),
            local_path: "/data/downloads/".into(),
        };
        let saved = api.create_remote_path_mapping(&m).await.unwrap();
        assert_eq!(saved.id, Some(5));
        m.local_path = "/mnt/dl/".into();
        api.update_remote_path_mapping(5, &m).await.unwrap();
        api.delete_remote_path_mapping(5).await.unwrap();
        assert_eq!(api.root_folders().await.unwrap(), vec!["/data/media/tv/"]);
        create.assert_async().await;
        update.assert_async().await;
        delete.assert_async().await;
    }

    fn api(server: &mockito::Server) -> ArrApi {
        ArrApi::new(ArrKind::Radarr, &server.url(), KEY, reqwest::Client::new())
    }

    #[tokio::test]
    async fn old_versions_are_refused() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v3/system/status")
            .with_body(r#"{"appName":"Radarr","version":"0.2.0.1504"}"#)
            .create_async()
            .await;
        let err = api(&server).system_status().await.unwrap_err();
        assert_eq!(err.kind(), "unsupported_version");
    }

    #[tokio::test]
    async fn a_timeout_has_its_own_kind() {
        // A listener that accepts and never answers
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let http = reqwest::Client::builder()
            .timeout(Duration::from_millis(200))
            .build()
            .unwrap();
        let api = ArrApi::new(ArrKind::Radarr, &url, KEY, http);
        let err = api
            .send(
                api.http
                    .get(format!("{}/x", url))
                    .timeout(Duration::from_millis(200)),
            )
            .await
            .unwrap_err();
        assert_eq!(err, ArrApiError::TimedOut);
        drop(listener);
    }

    #[tokio::test]
    async fn download_clients_round_trip_unknown_fields() {
        let mut server = mockito::Server::new_async().await;
        let listed = json!([{
            "id": 3, "name": "putio-sync", "implementation": "Transmission", "enable": true,
            "tags": [1], "somethingNew": {"x": 1},
            "fields": [{"name": "host", "value": "putio-sync"}, {"name": "port", "value": 15432}]
        }]);
        server
            .mock("GET", "/api/v3/downloadclient")
            .match_header("x-api-key", KEY)
            .with_body(listed.to_string())
            .create_async()
            .await;
        let mut clients = api(&server).download_clients().await.unwrap();
        let mut client = clients.remove(0);
        assert_eq!(client.id(), Some(3));
        assert_eq!(client.name(), "putio-sync");
        assert_eq!(client.implementation(), "Transmission");
        assert!(client.enabled());
        assert_eq!(client.field_str("host"), "putio-sync");
        assert_eq!(client.get_field("port"), Some(&json!(15432)));

        client.set_field("host", json!("10.0.0.2"));
        client.set_field("movieCategory", json!("radarr"));
        let mut expected = listed[0].clone();
        expected["fields"][0]["value"] = json!("10.0.0.2");
        expected["fields"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "movieCategory", "value": "radarr"}));
        let put = server
            .mock("PUT", "/api/v3/downloadclient/3?forceSave=true")
            .match_body(mockito::Matcher::Json(expected))
            .with_body("{}")
            .create_async()
            .await;
        api(&server)
            .update_download_client(3, &client, true)
            .await
            .unwrap();
        put.assert_async().await;
    }

    #[tokio::test]
    async fn a_400_carries_the_arr_messages() {
        let mut server = mockito::Server::new_async().await;
        let body =
            r#"[{"propertyName":"Host","errorMessage":"Unable to connect to Transmission"}]"#;
        server
            .mock("POST", "/api/v3/downloadclient/test")
            .with_status(400)
            .with_body(body)
            .create_async()
            .await;
        server
            .mock("POST", "/api/v3/downloadclient")
            .with_status(400)
            .with_body(body)
            .create_async()
            .await;
        let client = DownloadClient(json!({"name": "putio-sync"}));
        let api = api(&server);
        let err = api.test_download_client(&client).await.unwrap_err();
        assert_eq!(
            err,
            ArrApiError::TestFailed {
                messages: vec!["Unable to connect to Transmission".into()]
            }
        );
        assert_eq!(err.kind(), "arr_test_failed");
        let err = api.create_download_client(&client).await.unwrap_err();
        assert_eq!(err.kind(), "arr_refused");
    }

    #[tokio::test]
    async fn create_returns_the_id_and_delete_removes_it() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/api/v3/downloadclient")
            .with_status(201)
            .with_body(r#"{"id": 9, "name": "putio-sync"}"#)
            .create_async()
            .await;
        let delete = server
            .mock("DELETE", "/api/v3/downloadclient/9")
            .create_async()
            .await;
        let api = api(&server);
        let saved = api
            .create_download_client(&DownloadClient(json!({"name": "putio-sync"})))
            .await
            .unwrap();
        assert_eq!(saved.id(), Some(9));
        api.delete_download_client(9).await.unwrap();
        delete.assert_async().await;
    }

    #[test]
    fn no_error_message_contains_the_key() {
        let errors = [
            ArrApiError::Unreachable("connection refused".into()),
            ArrApiError::Unauthorized,
            ArrApiError::TimedOut,
            ArrApiError::UntrustedCertificate,
            ArrApiError::UnsupportedVersion {
                found: "2.0".into(),
            },
            ArrApiError::TestFailed {
                messages: vec!["x".into()],
            },
            ArrApiError::Refused { messages: vec![] },
        ];
        for e in errors {
            assert!(!e.to_string().contains(KEY));
        }
    }
}
