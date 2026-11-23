//! Anonymous usage stats (feature 058, part 2)
//!
//! Off by default. Nothing is sent unless `[usage_stats] enabled = true` and `endpoint` names a
//! receiver (there is no project receiver deployed yet, so an empty endpoint sends nothing). The
//! payload ([`payload`]) is built from the config and a few counts, holds only flags and size
//! buckets, and is what `putio-sync usage-stats show` and the web UI print before anything is
//! turned on. It never has names, paths, hosts, addresses, Put.io data or tokens, and plugins
//! appear only by [`BUNDLED_PLUGINS`] name.
//!
//! The random install id and the send times live in `usage-stats.json` next to the config file.
//! A send happens at most once per [`INTERVAL_HOURS`] (an attempt counts, so a failure waits for
//! the next day), after a random delay, through a client that refuses local addresses unless
//! `PUTIO_SYNC_ALLOW_LOCAL_FETCH` is set.

use crate::config::Config;
use crate::error::{CoreError, CoreResult};
use crate::net_guard::{self, GuardedResolver, Reach};
use chrono::{DateTime, Duration as Span, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The payload's version; a receiver rejects any other
pub const SCHEMA: u32 = 1;
/// Hours between two sends
pub const INTERVAL_HOURS: i64 = 24;
/// Longest random delay before a send, in seconds
pub const MAX_JITTER_SECS: u64 = 3600;
/// The file next to the config file
pub const FILE_NAME: &str = "usage-stats.json";
/// Longest accepted endpoint
pub const MAX_ENDPOINT: usize = 2048;
/// Plugins that ship in the binary (`putio-sync-plugins` checks this list against them); any
/// other plugin is never named
pub const BUNDLED_PLUGINS: &[&str] = &[
    "discord", "emby", "jellyfin", "ntfy", "plex", "pushover", "telegram",
];

const SEND_TIMEOUT: Duration = Duration::from_secs(10);
static FILE_LOCK: Mutex<()> = Mutex::new(());

/// `[usage_stats]`
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct UsageStatsConfig {
    /// Send the payload once a day (also needs an `endpoint`)
    pub enabled: bool,
    /// Where to POST it: an http(s) URL; empty means no receiver
    pub endpoint: String,
}

impl UsageStatsConfig {
    /// Both keys at their defaults
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Whether anything can be sent: on, with an endpoint
    pub fn active(&self) -> bool {
        self.enabled && !self.endpoint.trim().is_empty()
    }

    /// Check `endpoint`: empty, or an http(s) URL with a host and no embedded credentials
    pub fn validate(&self) -> CoreResult<()> {
        let endpoint = self.endpoint.trim();
        if endpoint.is_empty() {
            return Ok(());
        }
        let bad = |why: &str| {
            Err(CoreError::ConfigError(format!(
                "invalid usage_stats.endpoint: {why}"
            )))
        };
        if endpoint.len() > MAX_ENDPOINT {
            return bad("too long");
        }
        match reqwest::Url::parse(endpoint) {
            Ok(url)
                if matches!(url.scheme(), "http" | "https")
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none() =>
            {
                Ok(())
            }
            _ => bad("expected an http:// or https:// URL without a user name"),
        }
    }
}

/// What the payload needs besides the config
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    /// How this copy was installed (`Channel::detect().kind()`)
    pub build: String,
    /// Files downloaded in the last 7 days
    pub downloads_7d: u64,
    /// Bytes of the files downloaded so far
    pub library_bytes: u64,
    /// Whether any crash report is kept
    pub crash_reports: bool,
}

/// The JSON that is sent
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Payload {
    /// [`SCHEMA`]
    pub schema: u32,
    /// Random, made here, not derived from anything; reset with one button
    pub install_id: String,
    /// putio-sync's version
    pub version: String,
    /// Install kind: `docker`-style kinds such as `container`, `snap`, `homebrew`, `archive`
    pub build: String,
    /// `std::env::consts::OS`, or `other`
    pub os: String,
    /// `std::env::consts::ARCH`, or `other`
    pub arch: String,
    /// What is turned on
    pub features: Features,
    /// Size buckets
    pub sizes: Sizes,
}

/// Which features are on
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Features {
    /// Radarr/Sonarr download client
    pub arr: bool,
    /// Watch folders
    pub watch_folders: bool,
    /// Pipeline steps by stage
    pub pipeline_stages: Stages,
    /// Event hooks (commands and webhooks run for events)
    pub event_hooks: bool,
    /// Enabled bundled plugins, by name; user plugins are never listed
    pub plugins: Vec<String>,
    /// DLNA media server
    pub dlna: bool,
    /// Tailscale sign-in or tailnet-only listening
    pub tailscale: bool,
    /// Prometheus metrics
    pub metrics: bool,
    /// Subtitles for wanted languages
    pub subtitles: bool,
    /// Whether a crash report is kept (not its contents)
    pub crash_reports: bool,
}

/// Whether a pipeline step is configured in each stage
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Stages {
    /// Before the download
    pub pre_download: bool,
    /// After the download
    pub pre_processing: bool,
    /// After subtitles
    pub post_processing: bool,
}

/// Size buckets
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Sizes {
    /// Files downloaded per day over the last 7 days: `0`, `1-10`, `11-100`, `101-1000`, `1000+`
    pub files_per_day: String,
    /// Bytes downloaded so far: `<100GB`, `100GB-1TB`, `1-10TB`, `10TB+` (1 GB is 10^9 bytes)
    pub library: String,
}

/// The bucket of the average files per day: `files_7d` over 7 days, rounded up so any
/// download counts as at least `1-10`
pub fn files_per_day_bucket(files_7d: u64) -> &'static str {
    match files_7d.div_ceil(7) {
        0 => "0",
        1..=10 => "1-10",
        11..=100 => "11-100",
        101..=1000 => "101-1000",
        _ => "1000+",
    }
}

/// The bucket of a size in bytes
pub fn library_bucket(bytes: u64) -> &'static str {
    const GB: u64 = 1_000_000_000;
    match bytes {
        b if b < 100 * GB => "<100GB",
        b if b < 1000 * GB => "100GB-1TB",
        b if b < 10_000 * GB => "1-10TB",
        _ => "10TB+",
    }
}

/// Operating systems and architectures named as they are; anything else is `other`
const KNOWN_OS: &[&str] = &["linux", "macos", "windows", "freebsd", "netbsd", "openbsd"];
const KNOWN_ARCH: &[&str] = &[
    "x86_64",
    "aarch64",
    "arm",
    "x86",
    "riscv64",
    "powerpc64",
    "s390x",
];

fn pick(value: &str, known: &[&str]) -> String {
    if known.contains(&value) {
        value
    } else {
        "other"
    }
    .to_string()
}

/// The payload for `config` as this copy would send it now
pub fn payload(config: &Config, inputs: &Inputs, install_id: &str) -> Payload {
    use crate::hooks::HookStage;
    let stage = |s: HookStage| config.hooks.iter().any(|h| h.enabled && h.stage == Some(s));
    let mut plugins: Vec<String> = config
        .plugins
        .iter()
        .filter(|p| p.enabled && BUNDLED_PLUGINS.contains(&p.name.as_str()))
        .map(|p| p.name.clone())
        .collect();
    plugins.sort();
    plugins.dedup();
    Payload {
        schema: SCHEMA,
        install_id: install_id.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        build: inputs.build.clone(),
        os: pick(std::env::consts::OS, KNOWN_OS),
        arch: pick(std::env::consts::ARCH, KNOWN_ARCH),
        features: Features {
            arr: config.arr.enabled,
            watch_folders: config.watch.enabled && !config.watch.paths.is_empty(),
            pipeline_stages: Stages {
                pre_download: stage(HookStage::PreDownload),
                pre_processing: stage(HookStage::PreProcessing),
                post_processing: stage(HookStage::PostProcessing),
            },
            event_hooks: config.hooks.iter().any(|h| h.enabled && h.stage.is_none()),
            plugins,
            dlna: config.dlna.enabled,
            tailscale: config.tailscale.tailnet_only || config.tailscale.identity_sign_in,
            metrics: config.metrics.enabled,
            subtitles: !config.subtitles.languages.is_empty(),
            crash_reports: inputs.crash_reports,
        },
        sizes: Sizes {
            files_per_day: files_per_day_bucket(inputs.downloads_7d).to_string(),
            library: library_bucket(inputs.library_bytes).to_string(),
        },
    }
}

// ---- The file: install id and send times ----------------------------------------------

/// What `usage-stats.json` holds
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct State {
    /// A random UUID v4
    pub install_id: String,
    /// The last send the receiver accepted (RFC 3339)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_sent: Option<String>,
    /// The last send tried, accepted or not: the daily schedule counts from here
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt: Option<String>,
}

impl State {
    /// `last_sent` as a time
    pub fn sent_at(&self) -> Option<DateTime<Utc>> {
        parse_time(self.last_sent.as_deref())
    }

    /// `last_attempt` as a time
    pub fn attempted_at(&self) -> Option<DateTime<Utc>> {
        parse_time(self.last_attempt.as_deref())
    }
}

fn parse_time(text: Option<&str>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text?)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn valid_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok_and(|u| u.get_version_num() == 4)
}

/// The file's path for a config file path (the default path when `None`)
pub fn state_path(config_path: Option<&Path>) -> Option<PathBuf> {
    let default = Config::default_path().ok();
    Some(
        config_path
            .or(default.as_deref())?
            .parent()?
            .join(FILE_NAME),
    )
}

fn write(path: &Path, state: &State) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

fn read(path: &Path) -> State {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<State>(&t).ok())
        .unwrap_or_default()
}

/// The state, with an install id made (and saved, when it can be) if there was none
pub fn load(path: &Path) -> State {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = read(path);
    if !valid_id(&state.install_id) {
        state.install_id = new_id();
        let _ = write(path, &state);
    }
    state
}

/// A new install id; the send times are kept
pub fn reset_id(path: &Path) -> std::io::Result<State> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = read(path);
    state.install_id = new_id();
    write(path, &state)?;
    Ok(state)
}

fn update(path: &Path, change: impl FnOnce(&mut State)) -> std::io::Result<State> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = read(path);
    if !valid_id(&state.install_id) {
        state.install_id = new_id();
    }
    change(&mut state);
    write(path, &state)?;
    Ok(state)
}

// ---- Schedule -------------------------------------------------------------------------

/// A random delay up to [`MAX_JITTER_SECS`]
pub fn random_jitter() -> Duration {
    Duration::from_secs((uuid::Uuid::new_v4().as_u128() % u128::from(MAX_JITTER_SECS + 1)) as u64)
}

/// Decides when a send is due. The caller supplies the clock, so the schedule is testable.
#[derive(Debug, Clone)]
pub struct Schedule {
    armed: Option<DateTime<Utc>>,
    jitter: Duration,
}

impl Schedule {
    /// A schedule that delays each send by `jitter`
    pub fn new(jitter: Duration) -> Self {
        Self {
            armed: None,
            jitter,
        }
    }

    /// Whether to send now: usage stats are on with an endpoint, and [`INTERVAL_HOURS`] plus the
    /// delay have passed since the last attempt and since this schedule saw them turned on
    pub fn due(
        &mut self,
        config: &UsageStatsConfig,
        last_attempt: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> bool {
        if !config.active() {
            self.armed = None;
            return false;
        }
        let armed = *self.armed.get_or_insert(now);
        // A last attempt in the future (the clock moved back) must not stall the schedule
        let last = last_attempt.filter(|t| *t <= now);
        let from = last.map_or(armed, |t| (t + Span::hours(INTERVAL_HOURS)).max(armed));
        let delay = Span::from_std(self.jitter).unwrap_or_default();
        now >= from + delay
    }
}

// ---- Sending --------------------------------------------------------------------------

/// The reach a send gets: public addresses, or any when `PUTIO_SYNC_ALLOW_LOCAL_FETCH` is set
pub fn default_reach() -> Reach {
    Reach::or_opt_in(Reach::Public)
}

/// Why nothing was sent
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// Off, or no endpoint
    NotActive,
    /// The file next to the config can't be written, so the schedule couldn't be kept
    NotSaved(String),
    /// The request failed
    Failed(String),
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::NotActive => write!(f, "usage stats are off or have no endpoint"),
            SendError::NotSaved(e) => write!(f, "cannot save {FILE_NAME}: {e}"),
            SendError::Failed(e) => write!(f, "{e}"),
        }
    }
}

/// POST `payload` as JSON to `endpoint`: 10 seconds, no cookies, no redirects, the User-Agent
/// `putio-sync/<version>`, and only the addresses `reach` allows
pub async fn post(endpoint: &str, payload: &Payload, reach: Reach) -> Result<(), String> {
    let url = reqwest::Url::parse(endpoint).map_err(|_| "endpoint is not a URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("endpoint is not http or https".into());
    }
    net_guard::check_url(&url, reach).map_err(|e| format!("address not allowed: {e}"))?;
    let client = reqwest::Client::builder()
        .timeout(SEND_TIMEOUT)
        .user_agent(concat!("putio-sync/", env!("CARGO_PKG_VERSION")))
        .dns_resolver(Arc::new(GuardedResolver::new(reach)))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.without_url().to_string())?;
    let body = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
    let response = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", response.status().as_u16()))
    }
}

/// Send the payload now. Returns without a request unless usage stats are on with an endpoint.
/// The attempt is saved before the request, so a failure waits for the next day; when it can't
/// be saved nothing is sent.
pub async fn send_now(
    state_file: &Path,
    config: &Config,
    inputs: &Inputs,
    now: DateTime<Utc>,
    reach: Reach,
) -> Result<(), SendError> {
    if !config.usage_stats.active() {
        return Err(SendError::NotActive);
    }
    let stamp = now.to_rfc3339();
    let state = update(state_file, |s| s.last_attempt = Some(stamp.clone()))
        .map_err(|e| SendError::NotSaved(e.to_string()))?;
    let body = payload(config, inputs, &state.install_id);
    match post(config.usage_stats.endpoint.trim(), &body, reach).await {
        Ok(()) => {
            let _ = update(state_file, |s| s.last_sent = Some(stamp.clone()));
            Ok(())
        }
        Err(e) => {
            tracing::debug!("usage stats not sent: {e}");
            Err(SendError::Failed(e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_per_day_edges() {
        let b = files_per_day_bucket;
        assert_eq!(b(0), "0");
        assert_eq!(b(1), "1-10");
        assert_eq!(b(70), "1-10");
        assert_eq!(b(71), "11-100");
        assert_eq!(b(700), "11-100");
        assert_eq!(b(701), "101-1000");
        assert_eq!(b(7000), "101-1000");
        assert_eq!(b(7001), "1000+");
    }

    #[test]
    fn library_edges() {
        const GB: u64 = 1_000_000_000;
        let b = library_bucket;
        assert_eq!(b(0), "<100GB");
        assert_eq!(b(100 * GB - 1), "<100GB");
        assert_eq!(b(100 * GB), "100GB-1TB");
        assert_eq!(b(1000 * GB - 1), "100GB-1TB");
        assert_eq!(b(1000 * GB), "1-10TB");
        assert_eq!(b(10_000 * GB - 1), "1-10TB");
        assert_eq!(b(10_000 * GB), "10TB+");
    }

    #[test]
    fn endpoint_validation() {
        let ok = |e: &str| {
            UsageStatsConfig {
                enabled: false,
                endpoint: e.into(),
            }
            .validate()
            .is_ok()
        };
        assert!(ok(""));
        assert!(ok("https://stats.example/ingest"));
        assert!(ok("http://127.0.0.1:8787/"));
        assert!(!ok("ftp://stats.example"));
        assert!(!ok("not a url"));
        assert!(!ok("https://user:pw@stats.example/"));
    }

    #[test]
    fn install_id_is_made_kept_and_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        let first = load(&path).install_id;
        assert!(valid_id(&first));
        assert_eq!(load(&path).install_id, first);
        let second = reset_id(&path).unwrap().install_id;
        assert_ne!(second, first);
        assert_eq!(load(&path).install_id, second);
        // A damaged file gets a fresh id
        std::fs::write(&path, "{\"install_id\":\"mine\"}").unwrap();
        assert!(valid_id(&load(&path).install_id));
    }

    fn on() -> UsageStatsConfig {
        UsageStatsConfig {
            enabled: true,
            endpoint: "https://stats.example/".into(),
        }
    }

    fn at(h: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
            + Span::hours(h)
    }

    #[test]
    fn a_send_is_due_once_a_day_after_the_delay() {
        let mut s = Schedule::new(Duration::from_secs(600));
        // Never sent: due once the delay has passed since it was turned on
        assert!(!s.due(&on(), None, at(0)));
        assert!(!s.due(&on(), None, at(0) + Span::seconds(599)));
        assert!(s.due(&on(), None, at(0) + Span::seconds(600)));
        // Sent at hour 1: not due before hour 25 plus the delay
        let sent = Some(at(1));
        assert!(!s.due(&on(), sent, at(24)));
        assert!(!s.due(&on(), sent, at(25)));
        assert!(s.due(&on(), sent, at(25) + Span::seconds(600)));
    }

    #[test]
    fn off_or_no_endpoint_is_never_due_and_forgets_when_it_was_turned_on() {
        let mut s = Schedule::new(Duration::ZERO);
        let off = UsageStatsConfig::default();
        let no_endpoint = UsageStatsConfig {
            enabled: true,
            endpoint: "  ".into(),
        };
        assert!(!s.due(&off, None, at(100)));
        assert!(!s.due(&no_endpoint, None, at(100)));
        // Turned on later: the delay counts from then, not from the start
        let mut s = Schedule::new(Duration::from_secs(60));
        assert!(!s.due(&off, None, at(0)));
        assert!(!s.due(&on(), None, at(48)));
        assert!(s.due(&on(), None, at(48) + Span::seconds(60)));
    }

    #[test]
    fn a_last_attempt_in_the_future_does_not_stall() {
        let mut s = Schedule::new(Duration::ZERO);
        assert!(s.due(&on(), Some(at(500)), at(0)));
    }
}
