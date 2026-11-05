//! Auto-setup (feature 018): add putio-sync as a Transmission download client inside a
//! Radarr/Sonarr through its API. Candidate hosts are tested with the *arr's own
//! download-client test before anything is written (R5, R7).

use std::net::{Ipv4Addr, UdpSocket};

use putio_sync_core::config::{normalize_arr_category, ArrKind};
use putio_sync_telemetry::arr as metrics;
use putio_sync_telemetry::labels::{ArrSetupKind, ArrSetupResult};
use serde::Serialize;
use serde_json::json;
use tracing::{info, warn};

use crate::arr_api::{ArrApi, ArrApiError, DownloadClient};
use crate::clients::{category_field, points_at_putio_sync, CLIENT_NAME, URL_BASE};
use crate::path_check::PathCheck;

/// The most hosts a setup tests, at 5 s each (plan, Performance Goals)
pub const MAX_HOSTS: usize = 5;

/// What this machine knows about its own addresses (R5)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SystemHosts {
    /// `/etc/hostname` or `HOSTNAME`; the container id in Docker
    pub hostname: Option<String>,
    /// The default gateway, from `/proc/net/route`; the Docker host on a bridge network
    pub gateway: Option<String>,
    /// The address outgoing traffic leaves from
    pub local_ip: Option<String>,
}

impl SystemHosts {
    /// Read them from this machine. Missing sources are skipped.
    pub fn detect() -> Self {
        let hostname = std::fs::read_to_string("/etc/hostname")
            .ok()
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
            .or_else(|| std::env::var("HOSTNAME").ok().filter(|h| !h.is_empty()));
        let gateway = std::fs::read_to_string("/proc/net/route")
            .ok()
            .and_then(|r| parse_default_gateway(&r))
            .map(|ip| ip.to_string());
        // No packet is sent: connecting a UDP socket only picks the outgoing address
        let local_ip = UdpSocket::bind("0.0.0.0:0")
            .and_then(|s| {
                s.connect("192.0.2.1:9")?;
                s.local_addr()
            })
            .ok()
            .map(|a| a.ip())
            .filter(|ip| !ip.is_unspecified() && !ip.is_loopback())
            .map(|ip| ip.to_string());
        Self {
            hostname,
            gateway,
            local_ip,
        }
    }
}

/// The default gateway in a Linux `/proc/net/route` table. The gateway column is hex in
/// little-endian byte order.
pub fn parse_default_gateway(route: &str) -> Option<Ipv4Addr> {
    route.lines().skip(1).find_map(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 3 || cols[1] != "00000000" {
            return None;
        }
        let raw = u32::from_str_radix(cols[2], 16).ok()?;
        let ip = Ipv4Addr::from(raw.to_le_bytes());
        (!ip.is_unspecified()).then_some(ip)
    })
}

/// The host part of a `Host` header: no port, no IPv6 brackets
pub fn strip_port(host: &str) -> &str {
    let host = host.trim();
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    match host.rsplit_once(':') {
        Some((h, port)) if !h.contains(':') && port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => host,
    }
}

/// The hosts an *arr might reach putio-sync at, best first (R5). An override is used alone.
pub fn candidate_hosts(browser_host: Option<&str>, host_override: Option<&str>) -> Vec<String> {
    candidate_hosts_from(browser_host, host_override, &SystemHosts::detect())
}

/// [`candidate_hosts`] with the machine's addresses passed in
pub fn candidate_hosts_from(
    browser_host: Option<&str>,
    host_override: Option<&str>,
    sys: &SystemHosts,
) -> Vec<String> {
    if let Some(h) = host_override.map(str::trim).filter(|h| !h.is_empty()) {
        return vec![h.to_string()];
    }
    let browser = browser_host
        .map(strip_port)
        .filter(|h| *h != "localhost" && !h.starts_with("127.") && *h != "::1");
    let all = [
        Some(CLIENT_NAME),
        sys.hostname.as_deref(),
        browser,
        sys.gateway.as_deref(),
        sys.local_ip.as_deref(),
    ];
    let mut hosts: Vec<String> = Vec::new();
    for h in all.into_iter().flatten() {
        if !h.is_empty() && !hosts.iter().any(|x| x.eq_ignore_ascii_case(h)) {
            hosts.push(h.to_string());
        }
    }
    hosts.truncate(MAX_HOSTS);
    hosts
}

/// One setup's input. The secrets in it are never logged or returned.
#[derive(Clone)]
pub struct SetupInput {
    /// The *arr's address as typed: a URL, or a bare host (see [`base_urls`])
    pub url: String,
    /// Its API key
    pub api_key: String,
    /// `None` uses `radarr` or `sonarr`
    pub category: Option<String>,
    /// Hosts to try in order, from [`candidate_hosts`]
    pub hosts: Vec<String>,
    /// The port putio-sync's API listens on
    pub port: u16,
    /// The download-client username
    pub username: String,
    /// The download-client password
    pub password: String,
    /// Disable enabled Torrent Blackhole clients once putio-sync's is saved (US5)
    pub disable_blackhole: bool,
}

/// `create`, `update` or `unchanged`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Add a new client
    Create,
    /// Change putio-sync's existing client in place
    Update,
    /// Already set up (US5)
    Unchanged,
}

/// A download client the *arr already has
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExistingClient {
    /// Its name in the *arr
    pub name: String,
    /// `Transmission`, `TorrentBlackhole`, …
    pub implementation: String,
    /// Enabled in the *arr
    pub enabled: bool,
    /// It's putio-sync's own client (R6)
    pub points_at_putio_sync: bool,
}

/// What the api layer will do to putio-sync's own download client first (FR-023a)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct DownloadClientChange {
    /// It's off and will be turned on
    pub will_enable: bool,
    /// It has no password and one will be generated
    pub will_generate_password: bool,
}

/// The preview (data-model "SetupPlan")
#[derive(Debug, Clone, Serialize)]
pub struct SetupPlan {
    /// `radarr` or `sonarr`
    pub kind: ArrKind,
    /// The *arr's app name
    pub name: String,
    /// The base URL that answered
    pub url: String,
    /// Its version
    pub version: String,
    /// What setup will do
    pub action: Action,
    /// The client's name in the *arr
    pub client_name: String,
    /// The category putio-sync will be given
    pub category: String,
    /// Every download client the *arr has
    pub existing_clients: Vec<ExistingClient>,
    /// An enabled Torrent Blackhole exists
    pub blackhole_enabled: bool,
    /// Filled in by the api layer
    pub download_client: DownloadClientChange,
}

/// `created`, `updated`, `unchanged` or `failed`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// A client was added
    Created,
    /// putio-sync's client was changed
    Updated,
    /// Nothing needed changing
    Unchanged,
    /// See `reason`
    Failed,
}

/// Why a setup failed (data-model "Failure reasons")
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupFailure {
    /// One of the FR-026 reasons
    pub reason: &'static str,
    /// The *arr's message or the address tried; never a secret
    pub detail: String,
    /// The hosts tested, for `arr_cannot_reach`
    pub hosts_tried: Vec<String>,
}

impl SetupFailure {
    fn new(reason: &'static str, detail: impl Into<String>) -> Self {
        Self {
            reason,
            detail: detail.into(),
            hosts_tried: vec![],
        }
    }
}

impl From<ArrApiError> for SetupFailure {
    fn from(e: ArrApiError) -> Self {
        SetupFailure::new(e.kind(), e.to_string())
    }
}

/// The result (data-model "SetupResult")
#[derive(Debug, Clone, Serialize)]
pub struct SetupResult {
    /// What happened
    pub outcome: Outcome,
    /// On failure, one of the FR-026 reasons
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    /// On failure, what went wrong
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// `radarr` or `sonarr`, when known
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<ArrKind>,
    /// The *arr's app name, when known
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// The base URL that answered, when known
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Its version, when known
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The client's name in the *arr
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    /// The category the client was given (049)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// The host the *arr accepted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Every host tested
    pub hosts_tried: Vec<String>,
    /// A Torrent Blackhole was disabled (US5)
    pub blackhole_disabled: bool,
    /// The application was saved with "Remember"; set by the api layer
    pub remembered: bool,
    /// A non-fatal problem, such as "Remember" failing to save; set by the api layer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Can the *arr see the download folder (R10); set by the api layer after a setup
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_check: Option<PathCheck>,
}

impl SetupResult {
    fn failed(failure: SetupFailure, status: Option<&Inspection>) -> Self {
        Self {
            outcome: Outcome::Failed,
            reason: Some(failure.reason),
            detail: Some(failure.detail),
            kind: status.map(|s| s.api.kind()),
            app: status.map(|s| s.app.clone()),
            url: status.map(|s| s.api.url().to_string()),
            version: status.map(|s| s.version.clone()),
            client_name: None,
            category: None,
            host: None,
            hosts_tried: failure.hosts_tried,
            blackhole_disabled: false,
            remembered: false,
            warning: None,
            path_check: None,
        }
    }
}

/// What a preview and a run both start from
struct Inspection {
    api: ArrApi,
    app: String,
    version: String,
    category: String,
    clients: Vec<DownloadClient>,
    /// The body to test and save, without `host` set
    body: DownloadClient,
    action: Action,
    /// putio-sync's client as fetched, to restore if an update fails (R7)
    original: Option<DownloadClient>,
    /// Its settings already match; a passing test of it means "already set up" (R8)
    looks_unchanged: bool,
}

/// `putio-sync`, or `putio-sync (2)` … when another client has that name
fn unique_name(clients: &[DownloadClient]) -> String {
    let taken = |n: &str| clients.iter().any(|c| c.name().eq_ignore_ascii_case(n));
    if !taken(CLIENT_NAME) {
        return CLIENT_NAME.to_string();
    }
    (2..)
        .map(|i| format!("{} ({})", CLIENT_NAME, i))
        .find(|n| !taken(n))
        .expect("an unused name")
}

fn check_category(input: &SetupInput, kind: ArrKind) -> Result<String, SetupFailure> {
    let raw = input.category.as_deref().unwrap_or(kind.as_str());
    normalize_arr_category(raw)
        .filter(|c| !c.is_empty())
        .ok_or_else(|| {
            SetupFailure::new(
                "invalid_category",
                format!("'{}' is not a valid category", raw),
            )
        })
}

/// The base URLs to try for an address (US2). A URL with a scheme, or a host with a port, is
/// used as typed, path included. A bare host such as `nas` or `nas/radarr` is tried on HTTP
/// at Radarr's and then Sonarr's default port.
pub fn base_urls(address: &str) -> Vec<String> {
    let address = address.trim().trim_end_matches('/');
    if address.is_empty() {
        return vec![];
    }
    if address.contains("://") {
        return vec![address.to_string()];
    }
    let (host, path) = match address.find('/') {
        Some(i) => address.split_at(i),
        None => (address, ""),
    };
    // A port, but not the colons of a bare IPv6 address
    let has_port = if host.starts_with('[') {
        host.contains("]:")
    } else {
        host.matches(':').count() == 1
    };
    if has_port {
        return vec![format!("http://{}", address)];
    }
    crate::discovery::PORTS
        .iter()
        .map(|port| format!("http://{}:{}{}", host, port, path))
        .collect()
}

/// The first of `urls` that answers as Radarr or Sonarr. When none does, the most telling
/// error: an answer (wrong key, not an *arr, too old) beats no answer.
async fn detect_first(
    urls: &[String],
    api_key: &str,
    http: reqwest::Client,
) -> Result<(ArrApi, crate::arr_api::SystemStatus), SetupFailure> {
    let silent = |e: &ArrApiError| matches!(e.kind(), "unreachable" | "timed_out");
    let mut best: Option<ArrApiError> = None;
    for url in urls {
        match ArrApi::detect(url, api_key, http.clone()).await {
            Ok(found) => return Ok(found),
            Err(e) => {
                if best.as_ref().is_none_or(|b| silent(b) && !silent(&e)) {
                    best = Some(e);
                }
            }
        }
    }
    Err(best
        .map(|e| {
            let silent = silent(&e);
            let mut failure = SetupFailure::from(e);
            // No answer: say where it looked (US8)
            if silent {
                failure.detail = format!("{} (tried {})", failure.detail, urls.join(", "));
            }
            failure
        })
        .unwrap_or_else(|| SetupFailure::new("unreachable", "no address given")))
}

/// Find the Radarr or Sonarr at `address` (a URL or bare host, see [`base_urls`]) and check
/// the key (feature 049)
pub async fn connect(
    address: &str,
    api_key: &str,
    http: reqwest::Client,
) -> Result<ArrApi, SetupFailure> {
    detect_first(&base_urls(address), api_key, http)
        .await
        .map(|(api, _)| api)
}

/// The saved client already has the settings setup would give it. The host was matched by
/// [`points_at_putio_sync`], and the masked password can't be compared (R8).
fn same_settings(saved: &DownloadClient, wanted: &DownloadClient, kind: ArrKind) -> bool {
    let top = |c: &DownloadClient, k: &str| c.0.get(k).cloned();
    saved.enabled()
        && top(saved, "priority") == top(wanted, "priority")
        && [
            "port",
            "useSsl",
            "urlBase",
            "username",
            category_field(kind),
        ]
        .iter()
        .all(|f| saved.get_field(f) == wanted.get_field(f))
}

/// The *arr's own test of the saved client passes, so nothing needs writing (R8)
async fn already_set_up(found: &Inspection) -> bool {
    match (&found.original, found.looks_unchanged) {
        (Some(saved), true) => found.api.test_download_client(saved).await.is_ok(),
        _ => false,
    }
}

async fn inspect(input: &SetupInput, http: reqwest::Client) -> Result<Inspection, SetupFailure> {
    // A category that can't be valid for either kind is refused before any request
    if let Some(c) = &input.category {
        if normalize_arr_category(c)
            .filter(|c| !c.is_empty())
            .is_none()
        {
            return Err(SetupFailure::new(
                "invalid_category",
                format!("'{}' is not a valid category", c),
            ));
        }
    }
    let (api, status) = detect_first(&base_urls(&input.url), &input.api_key, http).await?;
    let kind = api.kind();
    let category = check_category(input, kind)?;
    let clients = api.download_clients().await?;

    let existing = clients
        .iter()
        .find(|c| points_at_putio_sync(c, input.port, &input.hosts));
    let original = existing.cloned();
    let (mut body, action) = match existing {
        Some(client) => (client.clone(), Action::Update),
        None => {
            let schema = api.download_client_schema().await?;
            let mut template = schema
                .into_iter()
                .find(|c| c.implementation() == "Transmission")
                .ok_or_else(|| {
                    SetupFailure::new("arr_refused", "this app has no Transmission client type")
                })?;
            template.set("name", json!(unique_name(&clients)));
            if let Some(map) = template.0.as_object_mut() {
                map.remove("id");
            }
            (template, Action::Create)
        }
    };
    body.set("enable", json!(true));
    body.set("priority", json!(1));
    body.set_field("port", json!(input.port));
    body.set_field("useSsl", json!(false));
    body.set_field("urlBase", json!(URL_BASE));
    body.set_field("username", json!(input.username));
    body.set_field("password", json!(input.password));
    body.set_field(category_field(kind), json!(category));
    let looks_unchanged = original
        .as_ref()
        .is_some_and(|o| same_settings(o, &body, kind));

    Ok(Inspection {
        api,
        app: status.app_name,
        version: status.version,
        category,
        clients,
        body,
        action,
        original,
        looks_unchanged,
    })
}

/// Check the address and key and say what setup would change. Changes nothing.
pub async fn preview(input: &SetupInput, http: reqwest::Client) -> Result<SetupPlan, SetupFailure> {
    let mut found = inspect(input, http).await?;
    if already_set_up(&found).await {
        found.action = Action::Unchanged;
    }
    let existing_clients = found
        .clients
        .iter()
        .map(|c| ExistingClient {
            name: c.name().to_string(),
            implementation: c.implementation().to_string(),
            enabled: c.enabled(),
            points_at_putio_sync: points_at_putio_sync(c, input.port, &input.hosts),
        })
        .collect();
    let blackhole_enabled = found
        .clients
        .iter()
        .any(|c| c.implementation() == "TorrentBlackhole" && c.enabled());
    Ok(SetupPlan {
        kind: found.api.kind(),
        name: found.app,
        url: found.api.url().to_string(),
        version: found.version,
        action: found.action,
        client_name: found.body.name().to_string(),
        category: found.category,
        existing_clients,
        blackhole_enabled,
        download_client: DownloadClientChange::default(),
    })
}

/// Messages that mean the *arr couldn't connect at all, as opposed to connecting and
/// being refused
fn looks_unreachable(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    [
        "unable to connect",
        "connection refused",
        "timed out",
        "timeout",
        "no such host",
        "name or service not known",
        "name does not resolve",
        "no route to host",
        "network is unreachable",
    ]
    .iter()
    .any(|p| m.contains(p))
}

/// Remove secrets from a message the *arr sent back
fn scrub(message: &str, input: &SetupInput) -> String {
    let mut out = message.to_string();
    for secret in [&input.api_key, &input.password] {
        if !secret.is_empty() {
            out = out.replace(secret.as_str(), "********");
        }
    }
    out
}

async fn write(found: &mut Inspection, input: &SetupInput) -> Result<SetupResult, SetupFailure> {
    if already_set_up(found).await {
        let host = found
            .original
            .as_ref()
            .map(|o| o.field_str("host").to_string())
            .unwrap_or_default();
        return Ok(finish(found, input, Outcome::Unchanged, host, vec![], None).await);
    }
    let mut tried = Vec::new();
    let mut messages = Vec::new();
    let mut accepted = None;
    for host in &input.hosts {
        found.body.set_field("host", json!(host));
        tried.push(host.clone());
        match found.api.test_download_client(&found.body).await {
            Ok(()) => {
                accepted = Some(host.clone());
                break;
            }
            Err(ArrApiError::TestFailed { messages: m }) => messages.extend(m),
            Err(other) => return Err(other.into()),
        }
    }
    let Some(host) = accepted else {
        let unreachable = messages.iter().all(|m| looks_unreachable(m));
        let detail = scrub(&messages.join("; "), input);
        return Err(SetupFailure {
            reason: if unreachable {
                "arr_cannot_reach"
            } else {
                "arr_test_failed"
            },
            detail,
            hosts_tried: tried,
        });
    };

    let saved = match (found.action, found.body.id()) {
        (Action::Update, Some(id)) => found
            .api
            .update_download_client(id, &found.body, false)
            .await
            .map(|()| (Outcome::Updated, Some(id))),
        _ => found
            .api
            .create_download_client(&found.body)
            .await
            .map(|c| (Outcome::Created, c.id())),
    };
    let (outcome, client_id) = match saved {
        Ok(saved) => saved,
        Err(e) => {
            undo(found).await;
            let mut failure = SetupFailure::from(e);
            failure.detail = scrub(&failure.detail, input);
            failure.hosts_tried = tried;
            return Err(failure);
        }
    };
    Ok(finish(found, input, outcome, host, tried, client_id).await)
}

/// The write failed after a passing test (R7): put the *arr back as it was. A create
/// removes any client it may have added; an update restores the fetched body.
async fn undo(found: &Inspection) {
    let api = &found.api;
    let done = match &found.original {
        Some(original) => match original.id() {
            Some(id) => api.update_download_client(id, original, true).await,
            None => Ok(()),
        },
        None => match api.download_clients().await {
            Ok(now) => {
                let before: Vec<i64> = found.clients.iter().filter_map(|c| c.id()).collect();
                let mut result = Ok(());
                for added in now.iter().filter(|c| {
                    c.name() == found.body.name() && c.id().is_some_and(|id| !before.contains(&id))
                }) {
                    if let Some(id) = added.id() {
                        result = result.and(api.delete_download_client(id).await);
                    }
                }
                result
            }
            Err(e) => Err(e),
        },
    };
    if let Err(e) = done {
        warn!(
            kind = api.kind().as_str(),
            error = e.kind(),
            "arr setup undo failed"
        );
    }
}

/// Disable each enabled Torrent Blackhole (not delete), once putio-sync's client is saved
async fn disable_blackholes(found: &Inspection) -> Result<bool, ArrApiError> {
    let mut any = false;
    for client in found
        .clients
        .iter()
        .filter(|c| c.implementation() == "TorrentBlackhole" && c.enabled())
    {
        let Some(id) = client.id() else { continue };
        let mut off = client.clone();
        off.set("enable", json!(false));
        // Its own folder test isn't putio-sync's concern (R7)
        found.api.update_download_client(id, &off, true).await?;
        any = true;
    }
    Ok(any)
}

async fn finish(
    found: &Inspection,
    input: &SetupInput,
    outcome: Outcome,
    host: String,
    tried: Vec<String>,
    client_id: Option<i64>,
) -> SetupResult {
    let (blackhole_disabled, warning) = if input.disable_blackhole {
        match disable_blackholes(found).await {
            Ok(any) => (any, None),
            Err(e) => (
                false,
                Some(format!(
                    "Set up, but Torrent Blackhole was not disabled: {}",
                    e
                )),
            ),
        }
    } else {
        (false, None)
    };
    info!(
        kind = found.api.kind().as_str(),
        url = found.api.url(),
        client_id,
        result = ?outcome,
        blackhole_disabled,
        "arr setup done"
    );
    let name = found
        .original
        .as_ref()
        .filter(|_| outcome == Outcome::Unchanged)
        .unwrap_or(&found.body)
        .name()
        .to_string();
    SetupResult {
        outcome,
        reason: None,
        detail: None,
        kind: Some(found.api.kind()),
        app: Some(found.app.clone()),
        url: Some(found.api.url().to_string()),
        version: Some(found.version.clone()),
        client_name: Some(name),
        category: Some(found.category.clone()),
        host: Some(host),
        hosts_tried: tried,
        blackhole_disabled,
        remembered: false,
        warning,
        path_check: None,
    }
}

fn metric_result(result: &SetupResult) -> ArrSetupResult {
    match (result.outcome, result.reason) {
        (Outcome::Created, _) => ArrSetupResult::Created,
        (Outcome::Updated, _) => ArrSetupResult::Updated,
        (Outcome::Unchanged, _) => ArrSetupResult::Unchanged,
        (Outcome::Failed, reason) => match reason.unwrap_or("") {
            "unreachable" => ArrSetupResult::Unreachable,
            "timed_out" => ArrSetupResult::TimedOut,
            "untrusted_certificate" => ArrSetupResult::UntrustedCertificate,
            "unauthorized" => ArrSetupResult::Unauthorized,
            "not_arr" => ArrSetupResult::NotArr,
            "unsupported_version" => ArrSetupResult::UnsupportedVersion,
            "invalid_category" => ArrSetupResult::InvalidCategory,
            "arr_cannot_reach" => ArrSetupResult::ArrCannotReach,
            "arr_test_failed" => ArrSetupResult::ArrTestFailed,
            "config_write_failed" => ArrSetupResult::ConfigWriteFailed,
            _ => ArrSetupResult::ArrRefused,
        },
    }
}

/// Count a result. Without a known kind (the *arr never answered) there's no label to use.
pub fn record(result: &SetupResult) {
    let kind = match result.kind {
        Some(ArrKind::Radarr) => ArrSetupKind::Radarr,
        Some(ArrKind::Sonarr) => ArrSetupKind::Sonarr,
        None => return,
    };
    metrics::setup(kind, metric_result(result));
}

/// Set up putio-sync as a download client in the *arr at `input.url`. A failure leaves
/// nothing behind: hosts are tested before anything is written.
pub async fn run(input: &SetupInput, http: reqwest::Client) -> SetupResult {
    let result = match inspect(input, http).await {
        Err(failure) => SetupResult::failed(failure, None),
        Ok(mut found) => match write(&mut found, input).await {
            Ok(result) => result,
            Err(failure) => SetupResult::failed(failure, Some(&found)),
        },
    };
    if result.outcome == Outcome::Failed {
        warn!(
            reason = result.reason.unwrap_or(""),
            hosts_tried = result.hosts_tried.len(),
            "arr setup failed"
        );
    }
    record(&result);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;
    use serde_json::Value;

    const KEY: &str = "arr-key-0123456789";
    const PASSWORD: &str = "dl-client-password";

    #[test]
    fn default_gateway_is_little_endian_hex() {
        let route = "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\n\
                     eth0\t0011A8C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\n\
                     eth0\t00000000\t0101A8C0\t0003\t0\t0\t0\t00000000\n";
        assert_eq!(
            parse_default_gateway(route),
            Some(Ipv4Addr::new(192, 168, 1, 1))
        );
        assert_eq!(parse_default_gateway("Iface\tDestination\n"), None);
        assert_eq!(parse_default_gateway(""), None);
    }

    #[test]
    fn candidate_hosts_order_dedupe_and_cap() {
        let sys = SystemHosts {
            hostname: Some("a1b2c3".into()),
            gateway: Some("172.17.0.1".into()),
            local_ip: Some("172.17.0.5".into()),
        };
        assert_eq!(
            candidate_hosts_from(Some("192.168.1.20:15432"), None, &sys),
            vec![
                "putio-sync",
                "a1b2c3",
                "192.168.1.20",
                "172.17.0.1",
                "172.17.0.5"
            ]
        );
        // The override is used alone
        assert_eq!(
            candidate_hosts_from(Some("nas"), Some(" 10.0.0.9 "), &sys),
            vec!["10.0.0.9"]
        );
        // Duplicates and loopback are dropped
        let sys = SystemHosts {
            hostname: Some("putio-sync".into()),
            gateway: None,
            local_ip: Some("10.0.0.2".into()),
        };
        assert_eq!(
            candidate_hosts_from(Some("localhost:15432"), None, &sys),
            vec!["putio-sync", "10.0.0.2"]
        );
        assert_eq!(
            candidate_hosts_from(Some("[fd00::1]:15432"), None, &SystemHosts::default()),
            vec!["putio-sync", "fd00::1"]
        );
    }

    fn input(server: &mockito::Server) -> SetupInput {
        SetupInput {
            url: server.url(),
            api_key: KEY.into(),
            category: None,
            hosts: vec!["putio-sync".into(), "10.0.0.2".into()],
            port: 15432,
            username: "putio-sync".into(),
            password: PASSWORD.into(),
            disable_blackhole: false,
        }
    }

    fn schema() -> String {
        json!([
            {"implementation": "QBittorrent", "name": "", "fields": []},
            {"implementation": "Transmission", "configContract": "TransmissionSettings",
             "protocol": "torrent", "name": "", "enable": true, "priority": 1, "tags": [],
             "fields": [
                {"name": "host", "value": "localhost"},
                {"name": "port", "value": 9091},
                {"name": "useSsl", "value": false},
                {"name": "urlBase", "value": "/transmission/"},
                {"name": "username"},
                {"name": "password"},
                {"name": "movieCategory", "value": ""},
                {"name": "recentMoviePriority", "value": 0}
             ]}
        ])
        .to_string()
    }

    async fn radarr(server: &mut mockito::Server, clients: &str) {
        server
            .mock("GET", "/api/v3/system/status")
            .match_header("x-api-key", KEY)
            .with_body(r#"{"appName":"Radarr","version":"5.14.0.9383"}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v3/downloadclient")
            .with_body(clients)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v3/downloadclient/schema")
            .with_body(schema())
            .create_async()
            .await;
    }

    fn field(name: &str, value: Value) -> Value {
        json!({"name": name, "value": value})
    }

    #[test]
    fn base_urls_keep_full_urls_and_expand_bare_hosts() {
        assert_eq!(
            base_urls("http://nas:8989/sonarr/"),
            vec!["http://nas:8989/sonarr"]
        );
        assert_eq!(
            base_urls("https://arr.example.com"),
            vec!["https://arr.example.com"]
        );
        assert_eq!(base_urls(" nas:7878 "), vec!["http://nas:7878"]);
        assert_eq!(base_urls("nas"), vec!["http://nas:7878", "http://nas:8989"]);
        assert_eq!(
            base_urls("nas/radarr"),
            vec!["http://nas:7878/radarr", "http://nas:8989/radarr"]
        );
        assert_eq!(base_urls("[::1]:7878"), vec!["http://[::1]:7878"]);
        assert!(base_urls("  ").is_empty());
    }

    #[tokio::test]
    async fn a_url_base_is_kept() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/sonarr/api/v3/system/status")
            .with_body(r#"{"appName":"Sonarr","version":"4.0.10.2544"}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/sonarr/api/v3/downloadclient")
            .with_body("[]")
            .create_async()
            .await;
        server
            .mock("GET", "/sonarr/api/v3/downloadclient/schema")
            .with_body(
                json!([{"implementation": "Transmission", "fields": [{"name": "tvCategory"}]}])
                    .to_string(),
            )
            .create_async()
            .await;
        let mut input = input(&server);
        input.url = format!("{}/sonarr/", server.url());
        let plan = preview(&input, reqwest::Client::new()).await.unwrap();
        assert_eq!(plan.url, format!("{}/sonarr", server.url()));
        assert_eq!(plan.kind, ArrKind::Sonarr);
    }

    #[tokio::test]
    async fn the_first_address_that_answers_as_an_arr_is_used() {
        // Nothing listens on the first; the second is not an *arr; the third is Radarr
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_url = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        let mut other = mockito::Server::new_async().await;
        other
            .mock("GET", "/api/v3/system/status")
            .with_status(404)
            .create_async()
            .await;
        let mut server = mockito::Server::new_async().await;
        radarr(&mut server, "[]").await;
        let urls = vec![closed_url.clone(), other.url(), server.url()];
        let (api, status) = detect_first(&urls, KEY, reqwest::Client::new())
            .await
            .unwrap();
        assert_eq!(api.url(), server.url());
        assert_eq!(status.app_name, "Radarr");

        // With no *arr, the answer that wasn't one is reported, not the silent address
        let failure = detect_first(&[closed_url, other.url()], KEY, reqwest::Client::new())
            .await
            .err()
            .unwrap();
        assert_eq!(failure.reason, "not_arr");
    }

    #[tokio::test]
    async fn a_non_arr_changes_nothing() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v3/system/status")
            .with_body(r#"{"appName":"Lidarr","version":"2.5.0"}"#)
            .create_async()
            .await;
        let writes = server
            .mock("POST", Matcher::Any)
            .expect(0)
            .create_async()
            .await;
        let result = run(&input(&server), reqwest::Client::new()).await;
        assert_eq!(result.outcome, Outcome::Failed);
        assert_eq!(result.reason, Some("not_arr"));
        writes.assert_async().await;
    }

    #[tokio::test]
    async fn preview_plans_a_create() {
        let mut server = mockito::Server::new_async().await;
        radarr(&mut server, "[]").await;
        let plan = preview(&input(&server), reqwest::Client::new())
            .await
            .unwrap();
        assert_eq!(plan.kind, ArrKind::Radarr);
        assert_eq!(plan.action, Action::Create);
        assert_eq!(plan.client_name, "putio-sync");
        assert_eq!(plan.category, "radarr");
        assert_eq!(plan.version, "5.14.0.9383");
        assert!(!plan.blackhole_enabled);
    }

    #[tokio::test]
    async fn setup_tries_hosts_in_order_then_creates_one_client() {
        let mut server = mockito::Server::new_async().await;
        radarr(&mut server, "[]").await;
        let first = server
            .mock("POST", "/api/v3/downloadclient/test")
            .match_body(Matcher::PartialJson(
                json!({"fields": [field("host", json!("putio-sync"))]}),
            ))
            .with_status(400)
            .with_body(
                r#"[{"propertyName":"Host","errorMessage":"Unable to connect to Transmission"}]"#,
            )
            .expect(1)
            .create_async()
            .await;
        let second = server
            .mock("POST", "/api/v3/downloadclient/test")
            .match_body(Matcher::PartialJson(
                json!({"fields": [field("host", json!("10.0.0.2"))]}),
            ))
            .expect(1)
            .create_async()
            .await;
        let create = server
            .mock("POST", "/api/v3/downloadclient")
            .match_body(Matcher::PartialJson(json!({
                "name": "putio-sync",
                "implementation": "Transmission",
                "configContract": "TransmissionSettings",
                "protocol": "torrent",
                "enable": true,
                "priority": 1,
                "fields": [
                    field("host", json!("10.0.0.2")),
                    field("port", json!(15432)),
                    field("useSsl", json!(false)),
                    field("urlBase", json!("/transmission/")),
                    field("username", json!("putio-sync")),
                    field("password", json!(PASSWORD)),
                    field("movieCategory", json!("radarr")),
                    field("recentMoviePriority", json!(0))
                ]
            })))
            .with_status(201)
            .with_body(r#"{"id": 4, "name": "putio-sync"}"#)
            .expect(1)
            .create_async()
            .await;

        let result = run(&input(&server), reqwest::Client::new()).await;
        assert_eq!(result.outcome, Outcome::Created, "{:?}", result);
        assert_eq!(result.host.as_deref(), Some("10.0.0.2"));
        assert_eq!(result.app.as_deref(), Some("Radarr"));
        assert_eq!(result.version.as_deref(), Some("5.14.0.9383"));
        assert_eq!(result.hosts_tried, vec!["putio-sync", "10.0.0.2"]);
        first.assert_async().await;
        second.assert_async().await;
        create.assert_async().await;
    }

    #[tokio::test]
    async fn sonarr_uses_tv_category() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v3/system/status")
            .with_body(r#"{"appName":"Sonarr","version":"4.0.10.2544"}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v3/downloadclient")
            .with_body("[]")
            .create_async()
            .await;
        server
            .mock("GET", "/api/v3/downloadclient/schema")
            .with_body(
                json!([{"implementation": "Transmission", "fields": [{"name": "tvCategory"}]}])
                    .to_string(),
            )
            .create_async()
            .await;
        server
            .mock("POST", "/api/v3/downloadclient/test")
            .create_async()
            .await;
        let create = server
            .mock("POST", "/api/v3/downloadclient")
            .match_body(Matcher::PartialJson(
                json!({"fields": [field("tvCategory", json!("sonarr"))]}),
            ))
            .with_body(r#"{"id": 1}"#)
            .create_async()
            .await;
        let result = run(&input(&server), reqwest::Client::new()).await;
        assert_eq!(result.outcome, Outcome::Created, "{:?}", result);
        assert_eq!(result.kind, Some(ArrKind::Sonarr));
        create.assert_async().await;
    }

    #[tokio::test]
    async fn an_invalid_category_is_refused_before_any_request() {
        let mut server = mockito::Server::new_async().await;
        let any = server
            .mock("GET", Matcher::Any)
            .expect(0)
            .create_async()
            .await;
        let mut input = input(&server);
        input.category = Some("a/../b".into());
        let result = run(&input, reqwest::Client::new()).await;
        assert_eq!(result.reason, Some("invalid_category"));
        any.assert_async().await;
    }

    #[tokio::test]
    async fn no_passing_host_writes_nothing() {
        let mut server = mockito::Server::new_async().await;
        radarr(&mut server, "[]").await;
        server
            .mock("POST", "/api/v3/downloadclient/test")
            .with_status(400)
            .with_body(r#"[{"errorMessage":"Unable to connect to Transmission"}]"#)
            .expect(2)
            .create_async()
            .await;
        let create = server
            .mock("POST", "/api/v3/downloadclient")
            .expect(0)
            .create_async()
            .await;
        let result = run(&input(&server), reqwest::Client::new()).await;
        assert_eq!(result.reason, Some("arr_cannot_reach"));
        assert_eq!(result.hosts_tried.len(), 2);
        create.assert_async().await;

        // Connected but refused: a different reason, and the password is never echoed
        let mut server = mockito::Server::new_async().await;
        radarr(&mut server, "[]").await;
        server
            .mock("POST", "/api/v3/downloadclient/test")
            .with_status(400)
            .with_body(format!(
                r#"[{{"errorMessage":"Authentication failed for {}"}}]"#,
                PASSWORD
            ))
            .create_async()
            .await;
        let result = run(&input(&server), reqwest::Client::new()).await;
        assert_eq!(result.reason, Some("arr_test_failed"));
        let text = serde_json::to_string(&result).unwrap();
        assert!(!text.contains(PASSWORD) && !text.contains(KEY));
    }

    #[tokio::test]
    async fn existing_client_is_updated_and_a_name_clash_is_avoided() {
        let mut server = mockito::Server::new_async().await;
        let ours = json!({"id": 7, "name": "Put.io", "implementation": "Transmission",
            "enable": false, "tags": [3], "fields": [
                field("host", json!("putio-sync")), field("port", json!(15432)),
                field("urlBase", json!("/transmission/"))]});
        radarr(&mut server, &json!([ours]).to_string()).await;
        server
            .mock("POST", "/api/v3/downloadclient/test")
            .create_async()
            .await;
        let put = server
            .mock("PUT", "/api/v3/downloadclient/7")
            .match_body(Matcher::PartialJson(
                json!({"id": 7, "tags": [3], "enable": true}),
            ))
            .create_async()
            .await;
        let result = run(&input(&server), reqwest::Client::new()).await;
        assert_eq!(result.outcome, Outcome::Updated, "{:?}", result);
        put.assert_async().await;

        // Someone else's client called putio-sync is left alone
        let theirs = json!([{"id": 2, "name": "putio-sync", "implementation": "QBittorrent",
            "enable": true, "fields": []}]);
        let mut server = mockito::Server::new_async().await;
        radarr(&mut server, &theirs.to_string()).await;
        let plan = preview(&input(&server), reqwest::Client::new())
            .await
            .unwrap();
        assert_eq!(plan.action, Action::Create);
        assert_eq!(plan.client_name, "putio-sync (2)");
    }

    /// putio-sync's client exactly as setup would leave it
    fn ours_as_set_up(enabled: bool) -> Value {
        json!({"id": 7, "name": "putio-sync", "implementation": "Transmission",
            "enable": enabled, "priority": 1, "tags": [3], "fields": [
                field("host", json!("putio-sync")), field("port", json!(15432)),
                field("useSsl", json!(false)), field("urlBase", json!("/transmission/")),
                field("username", json!("putio-sync")), field("password", json!("********")),
                field("movieCategory", json!("radarr"))]})
    }

    #[tokio::test]
    async fn already_set_up_writes_nothing() {
        let mut server = mockito::Server::new_async().await;
        radarr(&mut server, &json!([ours_as_set_up(true)]).to_string()).await;
        // The saved client is tested as it is, masked password and all
        server
            .mock("POST", "/api/v3/downloadclient/test")
            .match_body(Matcher::AllOf(vec![
                Matcher::PartialJson(json!({"id": 7})),
                Matcher::Regex(r#""value":"\*{8}""#.into()),
            ]))
            .expect(2)
            .create_async()
            .await;
        let writes = server
            .mock("PUT", Matcher::Regex("^/api/v3/downloadclient/".into()))
            .expect(0)
            .create_async()
            .await;
        let creates = server
            .mock("POST", "/api/v3/downloadclient")
            .expect(0)
            .create_async()
            .await;
        let plan = preview(&input(&server), reqwest::Client::new())
            .await
            .unwrap();
        assert_eq!(plan.action, Action::Unchanged);
        let result = run(&input(&server), reqwest::Client::new()).await;
        assert_eq!(result.outcome, Outcome::Unchanged, "{:?}", result);
        assert_eq!(result.host.as_deref(), Some("putio-sync"));
        assert_eq!(result.client_name.as_deref(), Some("putio-sync"));
        writes.assert_async().await;
        creates.assert_async().await;
    }

    #[tokio::test]
    async fn a_disabled_or_failing_saved_client_is_updated() {
        // Same settings but off: updated, not "already set up"
        let mut server = mockito::Server::new_async().await;
        radarr(&mut server, &json!([ours_as_set_up(false)]).to_string()).await;
        server
            .mock("POST", "/api/v3/downloadclient/test")
            .create_async()
            .await;
        let put = server
            .mock("PUT", "/api/v3/downloadclient/7")
            .match_body(Matcher::PartialJson(json!({"enable": true})))
            .create_async()
            .await;
        let result = run(&input(&server), reqwest::Client::new()).await;
        assert_eq!(result.outcome, Outcome::Updated, "{:?}", result);
        put.assert_async().await;

        // Enabled, but the saved password is stale: its test fails, the new body's passes
        let mut server = mockito::Server::new_async().await;
        radarr(&mut server, &json!([ours_as_set_up(true)]).to_string()).await;
        server
            .mock("POST", "/api/v3/downloadclient/test")
            .match_body(Matcher::Regex(r#""value":"\*{8}""#.into()))
            .with_status(400)
            .with_body(r#"[{"errorMessage":"Authentication failure"}]"#)
            .create_async()
            .await;
        server
            .mock("POST", "/api/v3/downloadclient/test")
            .match_body(Matcher::Regex(PASSWORD.into()))
            .create_async()
            .await;
        let put = server
            .mock("PUT", "/api/v3/downloadclient/7")
            .create_async()
            .await;
        let result = run(&input(&server), reqwest::Client::new()).await;
        assert_eq!(result.outcome, Outcome::Updated, "{:?}", result);
        put.assert_async().await;
    }

    fn others() -> Value {
        json!([
            {"id": 3, "name": "Blackhole", "implementation": "TorrentBlackhole",
             "enable": true, "fields": [field("torrentFolder", json!("/watch"))]},
            {"id": 4, "name": "qBit", "implementation": "QBittorrent", "enable": true,
             "fields": [field("host", json!("qbit"))]}
        ])
    }

    #[tokio::test]
    async fn the_blackhole_is_disabled_only_when_asked() {
        for disable in [false, true] {
            let mut server = mockito::Server::new_async().await;
            radarr(&mut server, &others().to_string()).await;
            server
                .mock("POST", "/api/v3/downloadclient/test")
                .create_async()
                .await;
            server
                .mock("POST", "/api/v3/downloadclient")
                .with_body(r#"{"id": 9, "name": "putio-sync"}"#)
                .create_async()
                .await;
            let blackhole = server
                .mock("PUT", "/api/v3/downloadclient/3")
                .match_query(Matcher::UrlEncoded("forceSave".into(), "true".into()))
                .match_body(Matcher::AllOf(vec![
                    Matcher::PartialJson(json!({"id": 3, "enable": false})),
                    Matcher::Regex("/watch".into()),
                ]))
                .expect(usize::from(disable))
                .create_async()
                .await;
            let qbit = server
                .mock("PUT", "/api/v3/downloadclient/4")
                .expect(0)
                .create_async()
                .await;

            let plan = preview(&input(&server), reqwest::Client::new())
                .await
                .unwrap();
            assert!(plan.blackhole_enabled);
            let listed: Vec<_> = plan
                .existing_clients
                .iter()
                .map(|c| (c.name.as_str(), c.points_at_putio_sync))
                .collect();
            assert_eq!(listed, vec![("Blackhole", false), ("qBit", false)]);

            let mut input = input(&server);
            input.disable_blackhole = disable;
            let result = run(&input, reqwest::Client::new()).await;
            assert_eq!(result.outcome, Outcome::Created, "{:?}", result);
            assert_eq!(result.blackhole_disabled, disable);
            blackhole.assert_async().await;
            qbit.assert_async().await;
        }
    }

    #[tokio::test]
    async fn a_failed_setup_leaves_the_blackhole_alone() {
        let mut server = mockito::Server::new_async().await;
        radarr(&mut server, &others().to_string()).await;
        server
            .mock("POST", "/api/v3/downloadclient/test")
            .with_status(400)
            .with_body(r#"[{"errorMessage":"Unable to connect to Transmission"}]"#)
            .create_async()
            .await;
        let blackhole = server
            .mock("PUT", Matcher::Any)
            .expect(0)
            .create_async()
            .await;
        let mut input = input(&server);
        input.disable_blackhole = true;
        let result = run(&input, reqwest::Client::new()).await;
        assert_eq!(result.outcome, Outcome::Failed);
        assert!(!result.blackhole_disabled);
        blackhole.assert_async().await;
    }

    #[tokio::test]
    async fn a_refused_update_restores_the_fetched_client() {
        let mut server = mockito::Server::new_async().await;
        let saved = json!({"id": 7, "name": "Put.io", "implementation": "Transmission",
            "enable": false, "tags": [3], "fields": [
                field("host", json!("putio-sync")), field("port", json!(15432)),
                field("urlBase", json!("/transmission/")),
                field("password", json!("********"))]});
        radarr(&mut server, &json!([saved.clone()]).to_string()).await;
        server
            .mock("POST", "/api/v3/downloadclient/test")
            .create_async()
            .await;
        let update = server
            .mock("PUT", "/api/v3/downloadclient/7")
            .match_query(Matcher::Missing)
            .with_status(400)
            .with_body(format!(r#"[{{"errorMessage":"Rejected {}"}}]"#, PASSWORD))
            .create_async()
            .await;
        let restore = server
            .mock("PUT", "/api/v3/downloadclient/7")
            .match_query(Matcher::UrlEncoded("forceSave".into(), "true".into()))
            .match_body(Matcher::Json(saved))
            .create_async()
            .await;
        let result = run(&input(&server), reqwest::Client::new()).await;
        assert_eq!(result.outcome, Outcome::Failed);
        assert_eq!(result.reason, Some("arr_refused"));
        let detail = result.detail.unwrap();
        assert!(detail.contains("Rejected"), "{}", detail);
        assert!(!detail.contains(PASSWORD), "{}", detail);
        update.assert_async().await;
        restore.assert_async().await;
    }

    #[tokio::test]
    async fn a_failed_create_removes_what_it_added() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v3/system/status")
            .with_body(r#"{"appName":"Radarr","version":"5.14.0.9383"}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v3/downloadclient/schema")
            .with_body(schema())
            .create_async()
            .await;
        // Before setup: the qBittorrent client. After the failed create, a half-saved
        // putio-sync is there too.
        let reads = Arc::new(AtomicUsize::new(0));
        let counter = reads.clone();
        server
            .mock("GET", "/api/v3/downloadclient")
            .with_body_from_request(move |_| {
                let mut list = vec![json!({"id": 4, "name": "qBit",
                    "implementation": "QBittorrent", "enable": true, "fields": []})];
                if counter.fetch_add(1, Ordering::SeqCst) > 0 {
                    list.push(json!({"id": 9, "name": "putio-sync",
                        "implementation": "Transmission", "enable": true, "fields": []}));
                }
                json!(list).to_string().into()
            })
            .create_async()
            .await;
        server
            .mock("POST", "/api/v3/downloadclient/test")
            .create_async()
            .await;
        server
            .mock("POST", "/api/v3/downloadclient")
            .with_status(500)
            .create_async()
            .await;
        let delete = server
            .mock("DELETE", "/api/v3/downloadclient/9")
            .create_async()
            .await;
        let other = server
            .mock("DELETE", "/api/v3/downloadclient/4")
            .expect(0)
            .create_async()
            .await;
        let result = run(&input(&server), reqwest::Client::new()).await;
        assert_eq!(result.outcome, Outcome::Failed, "{:?}", result);
        delete.assert_async().await;
        other.assert_async().await;
    }
}
