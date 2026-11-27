//! qBittorrent Web API v2 (feature 067): the same download client as the Transmission RPC
//! (011), for tools that only speak qBittorrent. Public routes under `/api/v2/`; every one
//! but `auth/login` needs the `SID` cookie a sign-in with the `[arr]` username and password
//! gives. `specs/067-qbittorrent-api/spec.md` lists the endpoints and the fixed answers.

use crate::middleware::auth::token_matches;
use crate::middleware::ratelimit::client_ip;
use crate::signin::session::is_secure;
use crate::AppState;
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use putio_sync_arr::{qbit, AddSource, ArrError, ArrService, ItemId, Snapshot};
use putio_sync_core::config::{normalize_arr_category, ArrSettings};
use putio_sync_telemetry::arr as metrics;
use putio_sync_telemetry::labels::{ArrMethod, ArrOutcome};
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::hash_map::RandomState;
use std::collections::{BTreeSet, HashMap};
use std::hash::BuildHasher;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// The session cookie, as qBittorrent names it
const COOKIE: &str = "SID";

/// The cookie is only sent to these routes
const COOKIE_PATH: &str = "/api/v2";

/// A session ends after this long without a request (qBittorrent's default)
const IDLE: Duration = Duration::from_secs(3600);

/// A session ends this long after its sign-in, used or not
const LIFETIME: Duration = Duration::from_secs(24 * 3600);

/// Sessions kept at once; a new sign-in beyond it ends the one used longest ago
const MAX_SESSIONS: usize = 64;

/// Categories `createCategory` keeps (they are folder names, created on first use)
const MAX_CATEGORIES: usize = 256;

/// Largest `torrents/add` body: a few large .torrent files
const MAX_ADD_BODY: usize = 16 * 1024 * 1024;

/// Largest body of any other request: a long `hashes` list
const MAX_BODY: usize = 1024 * 1024;

/// Most magnets, URLs and files one `torrents/add` may carry
const MAX_ADDS: usize = 50;

/// Requests that are answered `200` and change nothing: Put.io has no share limits, queue
/// positions, tags, announces, rechecks or speed limits per transfer
const IGNORED: &[&str] = &[
    "torrents/setShareLimits",
    "torrents/topPrio",
    "torrents/bottomPrio",
    "torrents/increasePrio",
    "torrents/decreasePrio",
    "torrents/setForceStart",
    "torrents/reannounce",
    "torrents/recheck",
    "torrents/setAutoManagement",
    "torrents/setDownloadLimit",
    "torrents/setUploadLimit",
    "torrents/toggleSequentialDownload",
    "torrents/toggleFirstLastPiecePrio",
    "torrents/setSuperSeeding",
    "torrents/addTags",
    "torrents/removeTags",
    "torrents/createTags",
    "torrents/deleteTags",
];

struct Session {
    created: Instant,
    last_seen: Instant,
    /// [`Sessions::fingerprint`] of the credentials it signed in with
    credentials: u64,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, Session>,
    categories: BTreeSet<String>,
}

/// The qBittorrent sessions, in memory: a restart or a password change ends them all, and
/// the client signs in again
pub struct Sessions {
    inner: Mutex<Inner>,
    /// Keys the credential fingerprint, new at each start
    key: RandomState,
}

impl Default for Sessions {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            key: RandomState::new(),
        }
    }
}

impl Sessions {
    /// A keyed hash of the credentials in use, so a session ends when they change without a
    /// copy of the password being kept
    fn fingerprint(&self, settings: &ArrSettings) -> u64 {
        self.key
            .hash_one((&settings.username, settings.password.as_deref()))
    }

    /// Start a session and return its id: 244 random bits from the OS, as hex
    fn start(&self, settings: &ArrSettings) -> String {
        let sid = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner
            .sessions
            .retain(|_, s| now.duration_since(s.created) < LIFETIME);
        while inner.sessions.len() >= MAX_SESSIONS {
            let oldest = inner
                .sessions
                .iter()
                .min_by_key(|(_, s)| s.last_seen)
                .map(|(sid, _)| sid.clone());
            match oldest {
                Some(sid) => inner.sessions.remove(&sid),
                None => break,
            };
        }
        inner.sessions.insert(
            sid.clone(),
            Session {
                created: now,
                last_seen: now,
                credentials: self.fingerprint(settings),
            },
        );
        sid
    }

    /// Whether `sid` is a live session of the current credentials; a live one is renewed
    fn check(&self, sid: &str, settings: &ArrSettings) -> bool {
        let now = Instant::now();
        let fingerprint = self.fingerprint(settings);
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let live = inner.sessions.get(sid).is_some_and(|s| {
            s.credentials == fingerprint
                && now.duration_since(s.created) < LIFETIME
                && now.duration_since(s.last_seen) < IDLE
        });
        if live {
            if let Some(s) = inner.sessions.get_mut(sid) {
                s.last_seen = now;
            }
        } else {
            inner.sessions.remove(sid);
        }
        live
    }

    fn end(&self, sid: &str) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sessions
            .remove(sid);
    }

    /// Age a session, for tests of the expiry
    #[doc(hidden)]
    pub fn age_all(&self, by: Duration) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for s in inner.sessions.values_mut() {
            if let Some(earlier) = s.last_seen.checked_sub(by) {
                s.last_seen = earlier;
            }
        }
    }

    /// The number of live sessions, for tests
    #[doc(hidden)]
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sessions
            .len()
    }

    /// Whether there is no session, for tests
    #[doc(hidden)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn created_categories(&self) -> Vec<String> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.categories.iter().cloned().collect()
    }

    fn create_category(&self, name: String) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.categories.len() < MAX_CATEGORIES {
            inner.categories.insert(name);
        }
    }

    fn remove_category(&self, name: &str) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .categories
            .remove(name);
    }
}

/// The fields and files of a request: its query string, then its form or multipart body
#[derive(Default)]
struct Params {
    fields: Vec<(String, String)>,
    torrents: Vec<Vec<u8>>,
}

impl Params {
    fn get(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn flag(&self, name: &str) -> bool {
        self.get(name)
            .is_some_and(|v| v.eq_ignore_ascii_case("true") || v == "1")
    }

    fn add_form(&mut self, bytes: &[u8]) {
        self.fields.extend(
            url::form_urlencoded::parse(bytes).map(|(k, v)| (k.into_owned(), v.into_owned())),
        );
    }

    /// Read a `multipart/form-data` body: parts named `torrents` with a file name are
    /// .torrent files, every other part is a field
    fn add_multipart(&mut self, body: &[u8], boundary: &str) {
        let delimiter = format!("--{boundary}").into_bytes();
        let mut at = match find(body, &delimiter, 0) {
            Some(i) => i + delimiter.len(),
            None => return,
        };
        while let Some(next) = find(body, &delimiter, at) {
            let part = &body[at..next];
            at = next + delimiter.len();
            let part = part.strip_prefix(b"\r\n").unwrap_or(part);
            let Some(split) = find(part, b"\r\n\r\n", 0) else {
                continue;
            };
            let head = String::from_utf8_lossy(&part[..split]);
            let content = &part[split + 4..];
            let content = content.strip_suffix(b"\r\n").unwrap_or(content);
            let Some(disposition) = head.lines().find(|l| {
                l.get(..20)
                    .is_some_and(|p| p.eq_ignore_ascii_case("content-disposition:"))
            }) else {
                continue;
            };
            let attribute = |wanted: &str| {
                disposition.split(';').skip(1).find_map(|a| {
                    let (k, v) = a.trim().split_once('=')?;
                    (k.trim() == wanted).then(|| v.trim().trim_matches('"').to_string())
                })
            };
            let Some(name) = attribute("name") else {
                continue;
            };
            if name == "torrents" && attribute("filename").is_some() {
                self.torrents.push(content.to_vec());
            } else {
                self.fields
                    .push((name, String::from_utf8_lossy(content).into_owned()));
            }
        }
    }
}

/// The first index of `needle` in `hay` at or after `from`
fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

/// The boundary of a `multipart/form-data` content type
fn boundary(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::CONTENT_TYPE)?.to_str().ok()?;
    let (kind, rest) = value.split_once(';')?;
    if !kind.trim().eq_ignore_ascii_case("multipart/form-data") {
        return None;
    }
    rest.split(';').find_map(|a| {
        let (k, v) = a.trim().split_once('=')?;
        (k.trim().eq_ignore_ascii_case("boundary") && !v.is_empty())
            .then(|| v.trim().trim_matches('"').to_string())
    })
}

/// The `SID` cookie of a request
fn sid(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == COOKIE)
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Whether a browser sent this from another site: an `Origin` whose host and port are not
/// the `Host` it was sent to. Tools and phone apps send no `Origin`.
fn cross_site(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let authority = origin.split_once("://").map_or(origin, |(_, rest)| rest);
    !authority.eq_ignore_ascii_case(host)
}

fn text(status: StatusCode, body: impl Into<String>) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "text/plain; charset=UTF-8")],
        body.into(),
    )
        .into_response()
}

fn ok() -> Response {
    text(StatusCode::OK, "")
}

fn forbidden() -> Response {
    text(StatusCode::FORBIDDEN, "Forbidden")
}

fn not_found() -> Response {
    text(StatusCode::NOT_FOUND, "Not Found")
}

fn failed(e: &ArrError) -> Response {
    warn!(error = %e, "qBittorrent API request failed");
    text(StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
}

fn cookie(value: &str, max_age: u64, secure: bool) -> String {
    format!(
        "{COOKIE}={value}; HttpOnly; SameSite=Strict; Path={COOKIE_PATH}; Max-Age={max_age}{}",
        if secure { "; Secure" } else { "" }
    )
}

fn with_cookie(mut response: Response, cookie: &str) -> Response {
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

/// The metric label of a path: the Transmission method that does the same
fn method_label(path: &str) -> ArrMethod {
    match path {
        "torrents/add" => ArrMethod::TorrentAdd,
        "torrents/delete" => ArrMethod::TorrentRemove,
        "torrents/info" | "torrents/properties" | "torrents/files" | "sync/maindata" => {
            ArrMethod::TorrentGet
        }
        "app/version" | "app/webapiVersion" | "app/preferences" | "app/buildInfo" => {
            ArrMethod::SessionGet
        }
        _ => ArrMethod::Other,
    }
}

/// `hashes=a|b` as item ids; `None` for `all`
fn hashes(params: &Params, name: &str) -> Option<Vec<ItemId>> {
    let raw = params.get(name).unwrap_or("");
    if raw == "all" {
        return None;
    }
    Some(
        raw.split('|')
            .map(str::trim)
            .filter(|h| !h.is_empty())
            .map(|h| ItemId::Hash(h.to_string()))
            .collect(),
    )
}

/// Any route under `/api/v2/` (public: it has its own sign-in)
pub async fn api(
    State(state): State<AppState>,
    Path(path): Path<String>,
    request: Request,
) -> Response {
    let Some(arr) = state.arr.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Read per request, as the Transmission RPC does (feature 018)
    let settings = arr.service.settings();
    if !settings.enabled {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(password) = settings.password.as_deref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "download client has no password",
        )
            .into_response();
    };
    if cross_site(request.headers()) {
        return forbidden();
    }

    let path = path.trim_matches('/').to_string();
    let method = request.method().clone();
    let headers = request.headers().clone();
    let ip = client_ip(&request);
    let secure = is_secure(&headers, None);
    let session = sid(&headers);

    let signed_in = session
        .as_deref()
        .is_some_and(|sid| arr.qbit.check(sid, &settings));
    if path != "auth/login" && !signed_in {
        return forbidden();
    }
    if path == "auth/login" && method != Method::POST {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let limit = if path == "torrents/add" {
        MAX_ADD_BODY
    } else {
        MAX_BODY
    };
    let mut params = Params::default();
    if let Some(query) = request.uri().query() {
        params.add_form(query.as_bytes());
    }
    let body = match axum::body::to_bytes(request.into_body(), limit).await {
        Ok(body) => body,
        Err(_) => return text(StatusCode::PAYLOAD_TOO_LARGE, "Request too large"),
    };
    match boundary(&headers) {
        Some(boundary) => params.add_multipart(&body, &boundary),
        None => params.add_form(&body),
    }

    if path == "auth/login" {
        if let Err(secs) = state.rpc_limiter.check(ip) {
            warn!(ip = %ip, retry_after_seconds = secs, "Download client rate limited");
            metrics::rpc_request(ArrMethod::Other, ArrOutcome::AuthFailed);
            let mut response = text(
                StatusCode::FORBIDDEN,
                "Your IP address has been banned after too many failed authentication attempts.",
            );
            if let Ok(value) = HeaderValue::from_str(&secs.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
            return response;
        }
        let user = params.get("username").unwrap_or("");
        let pass = params.get("password").unwrap_or("");
        if !(token_matches(user, &settings.username) & token_matches(pass, password)) {
            let failures = state.rpc_limiter.record_failure(ip);
            warn!(ip = %ip, failures, "Download client authentication failed");
            metrics::rpc_request(ArrMethod::Other, ArrOutcome::AuthFailed);
            return text(StatusCode::OK, "Fails.");
        }
        if let Some(old) = session.as_deref() {
            arr.qbit.end(old);
        }
        let sid = arr.qbit.start(&settings);
        debug!(ip = %ip, "qBittorrent API sign-in");
        return with_cookie(
            text(StatusCode::OK, "Ok."),
            &cookie(&sid, LIFETIME.as_secs(), secure),
        );
    }

    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());
    arr.service.contacts().record(user_agent);

    let changes = !matches!(
        path.as_str(),
        "app/version"
            | "app/webapiVersion"
            | "app/buildInfo"
            | "app/preferences"
            | "app/defaultSavePath"
            | "transfer/info"
            | "transfer/speedLimitsMode"
            | "sync/maindata"
            | "torrents/info"
            | "torrents/properties"
            | "torrents/files"
            | "torrents/trackers"
            | "torrents/webseeds"
            | "torrents/categories"
            | "torrents/tags"
    );
    // A change needs a POST, as in qBittorrent, so a link or an image can't make one
    if changes && method != Method::POST {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let service = &arr.service;
    let response = match path.as_str() {
        "auth/logout" => {
            if let Some(sid) = session.as_deref() {
                arr.qbit.end(sid);
            }
            with_cookie(ok(), &cookie("", 0, secure))
        }
        "app/version" => text(StatusCode::OK, qbit::VERSION),
        "app/webapiVersion" => text(StatusCode::OK, qbit::WEBAPI_VERSION),
        "app/buildInfo" => Json(qbit::build_info()).into_response(),
        "app/preferences" => Json(qbit::preferences(
            &service.download_dir(),
            state.api_port().await,
        ))
        .into_response(),
        "app/defaultSavePath" => text(StatusCode::OK, service.download_dir()),
        "transfer/info" => match service.snapshots(None).await {
            Ok(snaps) => Json(qbit::transfer_info(&snaps)).into_response(),
            Err(e) => failed(&e),
        },
        "transfer/speedLimitsMode" => text(StatusCode::OK, "0"),
        "sync/maindata" => maindata(service, &arr.qbit, &params).await,
        "torrents/info" => info(service, &params).await,
        "torrents/properties" | "torrents/files" | "torrents/trackers" | "torrents/webseeds" => {
            one(service, &path, &params).await
        }
        "torrents/categories" => match all_categories(service, &arr.qbit).await {
            Ok(names) => Json(qbit::categories(&names, service.target())).into_response(),
            Err(e) => failed(&e),
        },
        "torrents/tags" => Json(json!([])).into_response(),
        "torrents/createCategory" | "torrents/editCategory" => {
            match params
                .get("category")
                .map(str::trim)
                .filter(|c| !c.is_empty())
                .map(normalize_arr_category)
            {
                None => text(StatusCode::BAD_REQUEST, "Category name is empty"),
                Some(None) => text(StatusCode::CONFLICT, "Category name is invalid"),
                Some(Some(name)) => {
                    arr.qbit.create_category(name);
                    ok()
                }
            }
        }
        "torrents/removeCategories" => {
            for name in params.get("categories").unwrap_or("").lines() {
                arr.qbit.remove_category(name.trim());
            }
            ok()
        }
        "torrents/add" => add(service, &params).await,
        "torrents/delete" => delete(service, &params).await,
        "torrents/pause" | "torrents/stop" => pause(&state, service, &params, true).await,
        "torrents/resume" | "torrents/start" => pause(&state, service, &params, false).await,
        "torrents/setCategory" => set_category(service, &params).await,
        other if IGNORED.contains(&other) => ok(),
        _ => not_found(),
    };
    let outcome = if response.status().is_success() {
        ArrOutcome::Ok
    } else {
        ArrOutcome::Error
    };
    metrics::rpc_request(method_label(&path), outcome);
    response
}

/// The categories in use plus those `createCategory` named
async fn all_categories(
    service: &ArrService,
    sessions: &Sessions,
) -> Result<Vec<String>, ArrError> {
    let mut all: BTreeSet<String> = service.categories().await?.into_iter().collect();
    all.extend(sessions.created_categories());
    Ok(all.into_iter().collect())
}

/// Compare two torrents by one of their fields
fn by_field(a: &Value, b: &Value, key: &str) -> Ordering {
    match (&a[key], &b[key]) {
        (Value::Number(x), Value::Number(y)) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        (Value::String(x), Value::String(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => Ordering::Equal,
    }
}

/// The snapshots a `torrents/info` or `sync/maindata` request selects, fresh from Put.io
async fn selected(service: &ArrService, params: &Params) -> Result<Vec<Snapshot>, ArrError> {
    service.refresh_if_stale().await;
    let ids = match params.get("hashes") {
        Some(_) => hashes(params, "hashes"),
        None => None,
    };
    let mut snaps = service.snapshots(ids.as_deref()).await?;
    if let Some(category) = params.get("category") {
        let wanted = normalize_arr_category(category);
        snaps.retain(|s| wanted.as_deref() == Some(s.item.category.as_str()));
    }
    // There are no tags: only "untagged" (an empty value) matches anything
    if params.get("tag").is_some_and(|t| !t.is_empty()) {
        snaps.clear();
    }
    if let Some(filter) = params.get("filter") {
        snaps.retain(|s| qbit::matches_filter(qbit::state(s), s.local.speed_bps, filter));
    }
    Ok(snaps)
}

async fn info(service: &ArrService, params: &Params) -> Response {
    let snaps = match selected(service, params).await {
        Ok(snaps) => snaps,
        Err(e) => return failed(&e),
    };
    let target = service.target();
    let mut torrents: Vec<Value> = snaps.iter().map(|s| qbit::torrent(s, target)).collect();
    if let Some(key) = params.get("sort").filter(|k| !k.is_empty()) {
        torrents.sort_by(|a, b| by_field(a, b, key));
    }
    if params.flag("reverse") {
        torrents.reverse();
    }
    let number = |name: &str| params.get(name).and_then(|v| v.parse::<i64>().ok());
    let offset = match number("offset") {
        Some(n) if n < 0 => torrents.len().saturating_sub(n.unsigned_abs() as usize),
        Some(n) => (n as usize).min(torrents.len()),
        None => 0,
    };
    let mut torrents: Vec<Value> = torrents.into_iter().skip(offset).collect();
    if let Some(limit) = number("limit").filter(|n| *n > 0) {
        torrents.truncate(limit as usize);
    }
    Json(Value::Array(torrents)).into_response()
}

async fn maindata(service: &ArrService, sessions: &Sessions, params: &Params) -> Response {
    let rid = params
        .get("rid")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);
    service.refresh_if_stale().await;
    let snaps = match service.snapshots(None).await {
        Ok(snaps) => snaps,
        Err(e) => return failed(&e),
    };
    match all_categories(service, sessions).await {
        Ok(names) => Json(qbit::maindata(rid, &snaps, &names, service.target())).into_response(),
        Err(e) => failed(&e),
    }
}

/// The endpoints about one torrent, named by `hash`; an unknown hash is a 404
async fn one(service: &ArrService, path: &str, params: &Params) -> Response {
    let hash = params.get("hash").unwrap_or("").trim().to_string();
    if hash.is_empty() {
        return not_found();
    }
    let snaps = match service.snapshots(Some(&[ItemId::Hash(hash)])).await {
        Ok(snaps) => snaps,
        Err(e) => return failed(&e),
    };
    let Some(snap) = snaps.first() else {
        return not_found();
    };
    match path {
        "torrents/properties" => Json(qbit::properties(snap, service.target())).into_response(),
        "torrents/files" => {
            let on_disk = service.files_on_disk(&snap.item).await;
            Json(qbit::files(snap, &on_disk)).into_response()
        }
        "torrents/trackers" => Json(qbit::trackers()).into_response(),
        _ => Json(json!([])).into_response(),
    }
}

async fn add(service: &ArrService, params: &Params) -> Response {
    let mut sources: Vec<AddSource> = params
        .get("urls")
        .unwrap_or("")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|url| {
            if url.starts_with("magnet:") {
                AddSource::Magnet(url.to_string())
            } else {
                AddSource::Url(url.to_string())
            }
        })
        .collect();
    sources.extend(params.torrents.iter().cloned().map(AddSource::Metainfo));
    if sources.is_empty() {
        return text(StatusCode::BAD_REQUEST, "No torrent or URL given");
    }
    if sources.len() > MAX_ADDS {
        return text(StatusCode::BAD_REQUEST, "Too many torrents in one request");
    }
    let category = params.get("category").unwrap_or("");
    let save_path = params.get("savepath");

    let mut added = 0;
    let mut error = None;
    for source in sources {
        match service.add_in(source, category, save_path).await {
            Ok(_) => added += 1,
            Err(e) => error = Some(e),
        }
    }
    match error {
        // As qBittorrent: one that was added makes the request a success
        _ if added > 0 => text(StatusCode::OK, "Ok."),
        Some(ArrError::InvalidMagnet | ArrError::InvalidTorrent) => {
            text(StatusCode::UNSUPPORTED_MEDIA_TYPE, "Fails.")
        }
        Some(ArrError::Internal(e)) => failed(&ArrError::Internal(e)),
        Some(e) => text(StatusCode::CONFLICT, e.to_string()),
        None => text(StatusCode::OK, "Ok."),
    }
}

async fn delete(service: &ArrService, params: &Params) -> Response {
    // `all` is refused: these are also Radarr's and Sonarr's items
    let Some(ids) = hashes(params, "hashes").filter(|ids| !ids.is_empty()) else {
        return text(
            StatusCode::BAD_REQUEST,
            "Name the torrents to delete by hash",
        );
    };
    let delete_files = params.flag("deleteFiles");
    let found = match service.resolve(&ids).await {
        Ok(found) => found,
        Err(e) => return failed(&e),
    };
    for hash in found {
        if let Err(e) = service.remove(&hash, delete_files).await {
            return failed(&e);
        }
    }
    ok()
}

/// Pause or resume the items in putio-sync (039). Nothing is sent to Put.io: a transfer
/// there goes on, and its files wait.
async fn pause(state: &AppState, service: &ArrService, params: &Params, pause: bool) -> Response {
    let ids = hashes(params, "hashes");
    if ids.as_ref().is_some_and(Vec::is_empty) {
        return ok();
    }
    let snaps = match service.snapshots(ids.as_deref()).await {
        Ok(snaps) => snaps,
        Err(e) => return failed(&e),
    };
    let keys: Vec<String> = snaps
        .iter()
        .filter_map(|s| s.item.transfer_id)
        .map(|id| format!("transfer:{id}"))
        .collect();
    if keys.is_empty() {
        return ok();
    }
    let retention = state.config.read().await.transfers.finished_retention_hours;
    let manager = &state.download_manager;
    let result = if pause {
        manager
            .pause_items(&keys, &state.stall_tracker, retention)
            .await
    } else {
        manager
            .resume_items(&keys, &state.stall_tracker, retention)
            .await
    };
    match result {
        Ok(_) => ok(),
        Err(e) => {
            warn!(error = %e, pause, "qBittorrent API pause or resume failed");
            text(StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
        }
    }
}

/// A category is the item's folder, so it can't change after the add: naming the one the
/// items already have is fine, another is refused
async fn set_category(service: &ArrService, params: &Params) -> Response {
    let Some(category) = normalize_arr_category(params.get("category").unwrap_or("")) else {
        return text(StatusCode::CONFLICT, "Category name is invalid");
    };
    let ids = hashes(params, "hashes");
    let snaps = match service.snapshots(ids.as_deref()).await {
        Ok(snaps) => snaps,
        Err(e) => return failed(&e),
    };
    if snaps.iter().all(|s| s.item.category == category) {
        ok()
    } else {
        text(
            StatusCode::CONFLICT,
            "putio-sync cannot move a torrent to another category",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(password: &str) -> ArrSettings {
        ArrSettings {
            enabled: true,
            username: "putio-sync".into(),
            password: Some(password.into()),
            servers: Vec::new(),
        }
    }

    #[test]
    fn a_session_is_random_and_ends() {
        let sessions = Sessions::default();
        let s = settings("one");
        let a = sessions.start(&s);
        let b = sessions.start(&s);
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert!(sessions.check(&a, &s));
        assert!(!sessions.check("nope", &s));

        sessions.end(&a);
        assert!(!sessions.check(&a, &s));
        assert!(sessions.check(&b, &s));
    }

    #[test]
    fn a_session_ends_when_idle_or_when_the_password_changes() {
        let sessions = Sessions::default();
        let s = settings("one");
        let a = sessions.start(&s);
        assert!(!sessions.check(&a, &settings("two")));
        assert!(sessions.is_empty(), "a refused session is dropped");

        let b = sessions.start(&s);
        sessions.age_all(IDLE - Duration::from_secs(5));
        assert!(sessions.check(&b, &s), "a request renews it");
        sessions.age_all(IDLE + Duration::from_secs(1));
        assert!(!sessions.check(&b, &s));
    }

    #[test]
    fn the_oldest_session_makes_room() {
        let sessions = Sessions::default();
        let s = settings("one");
        let first = sessions.start(&s);
        sessions.age_all(Duration::from_secs(60));
        for _ in 0..MAX_SESSIONS {
            sessions.start(&s);
        }
        assert_eq!(sessions.len(), MAX_SESSIONS);
        assert!(!sessions.check(&first, &s));
    }

    #[test]
    fn multipart_fields_and_files() {
        let body = b"--XyZ\r\nContent-Disposition: form-data; name=\"category\"\r\n\r\nradarr\r\n\
--XyZ\r\ncontent-disposition: form-data; name=\"torrents\"; filename=\"a;b.torrent\"\r\n\
Content-Type: application/x-bittorrent\r\n\r\nd4:infod4:name1:aee\r\n--XyZ--\r\n";
        let mut params = Params::default();
        params.add_multipart(body, "XyZ");
        assert_eq!(params.get("category"), Some("radarr"));
        assert_eq!(params.torrents, vec![b"d4:infod4:name1:aee".to_vec()]);
    }

    #[test]
    fn the_boundary_is_read_quoted_or_bare() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=\"a b\""),
        );
        assert_eq!(boundary(&headers).as_deref(), Some("a b"));
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("Multipart/Form-Data; charset=utf-8; boundary=abc"),
        );
        assert_eq!(boundary(&headers).as_deref(), Some("abc"));
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        assert_eq!(boundary(&headers), None);
    }

    #[test]
    fn another_site_is_refused() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("nas:15432"));
        assert!(!cross_site(&headers));
        headers.insert(header::ORIGIN, HeaderValue::from_static("http://nas:15432"));
        assert!(!cross_site(&headers));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );
        assert!(cross_site(&headers));
    }

    #[test]
    fn hash_lists() {
        let mut params = Params::default();
        params.add_form(b"hashes=AA%7Cbb&deleteFiles=true");
        assert_eq!(
            hashes(&params, "hashes"),
            Some(vec![ItemId::Hash("AA".into()), ItemId::Hash("bb".into())])
        );
        assert!(params.flag("deleteFiles"));
        let mut all = Params::default();
        all.add_form(b"hashes=all");
        assert_eq!(hashes(&all, "hashes"), None);
    }

    #[test]
    fn the_cookie_is_http_only_and_scoped() {
        let c = cookie("abc", 60, false);
        assert_eq!(
            c,
            "SID=abc; HttpOnly; SameSite=Strict; Path=/api/v2; Max-Age=60"
        );
        assert!(cookie("abc", 60, true).ends_with("; Secure"));
    }
}
