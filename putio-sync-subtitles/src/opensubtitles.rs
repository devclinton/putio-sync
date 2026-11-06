//! The OpenSubtitles REST client (feature 050): search, the two-step download, login, the
//! download quota and a pace of a few requests a second.
//!
//! See `specs/050-subtitle-matching/contracts/opensubtitles-api.md`. The key, the password and
//! the token are sent in headers and bodies only; no error text built here carries any of them.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use putio_sync_core::lang;
use putio_sync_core::media::BoxFut;
use putio_sync_core::subtitles::OpenSubtitlesConfig;
use serde_json::{json, Value};

/// The public API
pub const BASE_URL: &str = "https://api.opensubtitles.com/api/v1";
/// Time between requests: the service allows a few a second
pub const PACE: Duration = Duration::from_millis(250);
/// Longest wait for a `Retry-After`
const MAX_RETRY_WAIT: Duration = Duration::from_secs(60);
/// How long a quota stays used up when the answer does not say
const DEFAULT_RESET: Duration = Duration::from_secs(3600);

/// One search result, with only what the engine reads
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hit {
    /// The file to download
    pub file_id: i64,
    /// Canonical language code
    pub language: String,
    /// The release string the subtitle was made for
    pub release: String,
    /// For the deaf and hard of hearing
    pub hearing_impaired: bool,
    /// Foreign parts only
    pub foreign_parts_only: bool,
    /// Machine or AI translated
    pub machine_translated: bool,
    /// The service says the file fingerprint matches
    pub moviehash_match: bool,
    /// Times downloaded
    pub downloads: u64,
    /// The feature (episode or movie) title
    pub title: String,
    /// The series title of an episode
    pub parent_title: String,
    /// Feature year
    pub year: Option<u16>,
    /// Season of an episode
    pub season: Option<u16>,
    /// Episode number
    pub episode: Option<u16>,
}

/// Why a call gave nothing
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SearchError {
    /// The key (or login) was refused; nothing is tried again until the settings change
    #[error("opensubtitles: key or login rejected")]
    KeyRejected,
    /// The download quota is used up
    #[error("opensubtitles: download quota used up")]
    Quota,
    /// Still rate limited after waiting once
    #[error("opensubtitles: rate limited")]
    RateLimited,
    /// A network error, a 5xx or an answer that makes no sense; may pass later
    #[error("opensubtitles: {0}")]
    Failed(String),
}

/// The download quota as far as known
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Quota {
    /// Used up now
    pub exhausted: bool,
    /// Downloads left, when the service said
    pub remaining: Option<u32>,
    /// Downloads allowed a day, when the service said (logged in)
    pub allowed: Option<u32>,
}

/// The engine's seam to OpenSubtitles, so it is tested without HTTP
pub trait SubtitleSearch: Send + Sync {
    /// Search with query parameters, already sorted and lowercase
    fn search<'a>(
        &'a self,
        params: &'a [(String, String)],
    ) -> BoxFut<'a, Result<Vec<Hit>, SearchError>>;

    /// One subtitle's bytes (the quota is spent here)
    fn download<'a>(&'a self, file_id: i64) -> BoxFut<'a, Result<Vec<u8>, SearchError>>;

    /// What is known of the quota
    fn quota(&self) -> Quota;
}

#[derive(Default)]
struct State {
    token: Option<String>,
    key_rejected: bool,
    next_slot: Option<Instant>,
    exhausted_until: Option<Instant>,
    remaining: Option<u32>,
    allowed: Option<u32>,
}

/// A client for one key
pub struct OpenSubtitles {
    base: String,
    key: String,
    username: String,
    password: String,
    user_agent: String,
    http: reqwest::Client,
    pace: Duration,
    state: Mutex<State>,
}

impl std::fmt::Debug for OpenSubtitles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenSubtitles")
            .field("base", &self.base)
            .finish_non_exhaustive()
    }
}

impl OpenSubtitles {
    /// A client for `config` (the environment overrides apply); `None` while no key is set
    pub fn from_config(config: &OpenSubtitlesConfig) -> Option<Self> {
        config.is_active().then(|| {
            Self::new(
                BASE_URL,
                config.effective_api_key().trim(),
                config.username.trim(),
                &config.effective_password(),
                &config.effective_user_agent(),
                PACE,
            )
        })
    }

    /// A client for `base` (tests point it at a mock server with no pace)
    pub fn new(
        base: &str,
        key: &str,
        username: &str,
        password: &str,
        user_agent: &str,
        pace: Duration,
    ) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            key: key.to_string(),
            username: username.to_string(),
            password: password.to_string(),
            user_agent: user_agent.to_string(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            pace,
            state: Mutex::new(State::default()),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Wait for this request's turn: at most one per pace, in the order asked
    async fn wait_turn(&self) {
        if self.pace.is_zero() {
            return;
        }
        let wait = {
            let mut s = self.state();
            let now = Instant::now();
            let at = s.next_slot.map_or(now, |t| t.max(now));
            s.next_slot = Some(at + self.pace);
            at - now
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }

    fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        token: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let mut r = self
            .http
            .request(method, format!("{}{}", self.base, path))
            .header("Api-Key", &self.key)
            .header("User-Agent", &self.user_agent)
            .header("Accept", "application/json");
        if let Some(t) = token {
            r = r.bearer_auth(t);
        }
        r
    }

    /// Log in when a username is set, once; the token is kept until it is refused
    async fn token(&self) -> Result<Option<String>, SearchError> {
        if self.username.is_empty() {
            return Ok(None);
        }
        if let Some(t) = self.state().token.clone() {
            return Ok(Some(t));
        }
        self.wait_turn().await;
        let sent = self
            .request(reqwest::Method::POST, "/login", None)
            .json(&json!({"username": self.username, "password": self.password}))
            .send()
            .await;
        let response = sent.map_err(|e| {
            record("login", "error");
            SearchError::Failed(describe(&e))
        })?;
        let status = response.status();
        if status == 401 || status == 403 || status == 400 {
            record("login", "error");
            self.state().key_rejected = true;
            return Err(SearchError::KeyRejected);
        }
        if !status.is_success() {
            record("login", "error");
            return Err(SearchError::Failed(format!("login answered HTTP {status}")));
        }
        record("login", "ok");
        let body: Value = response
            .json()
            .await
            .map_err(|_| SearchError::Failed("login answer is not JSON".to_string()))?;
        let token = body["token"].as_str().map(str::to_string);
        let mut s = self.state();
        if let Some(n) = body["user"]["allowed_downloads"].as_u64() {
            s.allowed = Some(n as u32);
        }
        s.token = token.clone();
        Ok(token)
    }

    /// One request with the key (and token); waits once on a 429; a refused token is renewed once
    async fn call(
        &self,
        endpoint: &'static str,
        method: reqwest::Method,
        path: &str,
        query: &[(String, String)],
        body: Option<Value>,
    ) -> Result<reqwest::Response, SearchError> {
        if self.state().key_rejected {
            return Err(SearchError::KeyRejected);
        }
        let mut renewed = false;
        let mut waited = false;
        loop {
            let token = self.token().await?;
            self.wait_turn().await;
            let mut r = self.request(method.clone(), path, token.as_deref());
            if !query.is_empty() {
                r = r.query(query);
            }
            if let Some(b) = &body {
                r = r.json(b);
            }
            let response = r.send().await.map_err(|e| {
                record(endpoint, "error");
                SearchError::Failed(describe(&e))
            })?;
            let status = response.status();
            if status == 401 || status == 403 {
                if token.is_some() && !renewed {
                    renewed = true;
                    self.state().token = None;
                    continue;
                }
                record(endpoint, "error");
                self.state().key_rejected = true;
                return Err(SearchError::KeyRejected);
            }
            if status == 429 {
                record(endpoint, "rate_limited");
                if waited {
                    return Err(SearchError::RateLimited);
                }
                waited = true;
                let secs = response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .unwrap_or(1);
                tokio::time::sleep(Duration::from_secs(secs).min(MAX_RETRY_WAIT)).await;
                continue;
            }
            return Ok(response);
        }
    }

    /// The numbers the download answer carries about the quota
    fn learn_quota(&self, body: &Value) {
        let mut s = self.state();
        if let Some(n) = body["remaining"].as_u64() {
            s.remaining = Some(n as u32);
        }
    }

    fn use_up(&self, reset: Option<&str>) {
        let wait = reset.and_then(parse_reset).unwrap_or(DEFAULT_RESET);
        let mut s = self.state();
        s.exhausted_until = Some(Instant::now() + wait);
        s.remaining = Some(0);
    }

    /// Log in if set and make one search: the Test button. Never returns the key or the token.
    pub async fn check(&self) -> Result<Quota, SearchError> {
        self.token().await?;
        let query = [
            ("query".to_string(), "test".to_string()),
            ("type".to_string(), "movie".to_string()),
        ];
        let response = self
            .call("search", reqwest::Method::GET, "/subtitles", &query, None)
            .await?;
        if !response.status().is_success() {
            record("search", "error");
            return Err(SearchError::Failed(format!(
                "search answered HTTP {}",
                response.status()
            )));
        }
        record("search", "ok");
        Ok(self.quota())
    }
}

/// `23h 12m` or `45s` as a duration
pub fn parse_reset(text: &str) -> Option<Duration> {
    let mut total = 0u64;
    let mut number = String::new();
    let mut any = false;
    for c in text.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let unit = match c {
            'h' => 3600,
            'm' => 60,
            's' => 1,
            _ => continue,
        };
        if let Ok(n) = number.parse::<u64>() {
            total += n * unit;
            any = true;
        }
        number.clear();
    }
    any.then(|| Duration::from_secs(total.max(60)))
}

/// A network error without the URL (which holds nothing secret, but is noise)
fn describe(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "request timed out".to_string()
    } else if e.is_connect() {
        "could not connect".to_string()
    } else {
        "request failed".to_string()
    }
}

fn record(endpoint: &str, result: &str) {
    putio_sync_telemetry::subtitles::opensubtitles_request(endpoint, result);
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().trim().to_string()
}

fn number(v: &Value) -> Option<u16> {
    v.as_u64()
        .and_then(|n| u16::try_from(n).ok())
        .filter(|n| *n > 0)
}

/// Read a search answer leniently: a result without a file is dropped, every other field is optional
pub fn parse_hits(body: &Value) -> Vec<Hit> {
    let Some(data) = body["data"].as_array() else {
        return Vec::new();
    };
    data.iter()
        .filter_map(|d| {
            let a = &d["attributes"];
            let file_id = a["files"].as_array()?.first()?["file_id"].as_i64()?;
            let f = &a["feature_details"];
            Some(Hit {
                file_id,
                language: lang::map_lenient(&text(&a["language"])),
                release: text(&a["release"]),
                hearing_impaired: a["hearing_impaired"].as_bool().unwrap_or(false),
                foreign_parts_only: a["foreign_parts_only"].as_bool().unwrap_or(false),
                machine_translated: a["machine_translated"].as_bool().unwrap_or(false)
                    || a["ai_translated"].as_bool().unwrap_or(false),
                moviehash_match: a["moviehash_match"].as_bool().unwrap_or(false),
                downloads: a["download_count"].as_u64().unwrap_or(0),
                title: text(&f["title"]),
                parent_title: text(&f["parent_title"]),
                year: number(&f["year"]),
                season: number(&f["season_number"]),
                episode: number(&f["episode_number"]),
            })
        })
        .collect()
}

impl SubtitleSearch for OpenSubtitles {
    fn search<'a>(
        &'a self,
        params: &'a [(String, String)],
    ) -> BoxFut<'a, Result<Vec<Hit>, SearchError>> {
        Box::pin(async move {
            let response = self
                .call("search", reqwest::Method::GET, "/subtitles", params, None)
                .await?;
            let status = response.status();
            if !status.is_success() {
                record("search", "error");
                // 4xx other than the ones handled: the query was refused, nothing to find
                return if status.is_client_error() {
                    Ok(Vec::new())
                } else {
                    Err(SearchError::Failed(format!(
                        "search answered HTTP {status}"
                    )))
                };
            }
            let body: Value = response.json().await.map_err(|_| {
                record("search", "error");
                SearchError::Failed("search answer is not JSON".to_string())
            })?;
            record("search", "ok");
            Ok(parse_hits(&body))
        })
    }

    fn download<'a>(&'a self, file_id: i64) -> BoxFut<'a, Result<Vec<u8>, SearchError>> {
        Box::pin(async move {
            if self.quota().exhausted {
                return Err(SearchError::Quota);
            }
            let response = self
                .call(
                    "download",
                    reqwest::Method::POST,
                    "/download",
                    &[],
                    Some(json!({"file_id": file_id, "sub_format": "srt"})),
                )
                .await?;
            let status = response.status();
            let body: Value = response.json().await.unwrap_or(Value::Null);
            if status == 406 {
                record("download", "quota");
                self.use_up(body["reset_time"].as_str());
                return Err(SearchError::Quota);
            }
            if !status.is_success() {
                record("download", "error");
                return Err(SearchError::Failed(format!(
                    "download answered HTTP {status}"
                )));
            }
            self.learn_quota(&body);
            let Some(link) = body["link"].as_str().filter(|l| !l.is_empty()) else {
                if body["remaining"].as_u64() == Some(0) {
                    record("download", "quota");
                    self.use_up(body["reset_time"].as_str());
                    return Err(SearchError::Quota);
                }
                record("download", "error");
                return Err(SearchError::Failed(
                    "download answer has no link".to_string(),
                ));
            };
            // The link needs no key
            let file = self.http.get(link).send().await.map_err(|e| {
                record("download", "error");
                SearchError::Failed(describe(&e))
            })?;
            if !file.status().is_success() {
                record("download", "error");
                return Err(SearchError::Failed(format!(
                    "subtitle file answered HTTP {}",
                    file.status()
                )));
            }
            let bytes = file.bytes().await.map_err(|e| {
                record("download", "error");
                SearchError::Failed(describe(&e))
            })?;
            record("download", "ok");
            Ok(bytes.to_vec())
        })
    }

    fn quota(&self) -> Quota {
        let s = self.state();
        Quota {
            exhausted: s.exhausted_until.is_some_and(|t| t > Instant::now()),
            remaining: s.remaining,
            allowed: s.allowed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reset_text_is_read() {
        assert_eq!(
            parse_reset("23h 12m"),
            Some(Duration::from_secs(23 * 3600 + 12 * 60))
        );
        assert_eq!(parse_reset("45s"), Some(Duration::from_secs(60)));
        assert_eq!(parse_reset("soon"), None);
    }

    #[test]
    fn a_result_without_a_file_is_dropped_and_odd_fields_are_tolerated() {
        let body = json!({"data": [
            {"attributes": {"language": "EN", "files": []}},
            {"attributes": {"language": "pt-BR", "ai_translated": true,
                "feature_details": {"year": "bad", "season_number": 0},
                "files": [{"file_id": 5}]}},
        ]});
        let hits = parse_hits(&body);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file_id, 5);
        assert!(hits[0].machine_translated);
        assert_eq!(hits[0].year, None);
        assert_eq!(hits[0].season, None);
    }
}
