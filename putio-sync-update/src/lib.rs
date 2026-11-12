//! Update checker (feature 055).
//!
//! [`UpdateService`] asks GitHub Releases for a newer version a minute after start and
//! then every 24 hours, keeps the answer in the database, and reports it to the API.

pub mod channel;
pub mod fetch;
pub mod github;
pub mod version;

use channel::Channel;
use chrono::{DateTime, Utc};
use github::{FetchError, Fetched, ReleaseInfo};
use putio_sync_core::config::UpdatesConfig;
use putio_sync_db::Database;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use version::Version;

/// The first check, counted from start
pub const FIRST_CHECK: Duration = Duration::from_secs(60);
/// Between scheduled checks
pub const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// The shortest gap before "Check now" asks again
pub const MIN_GAP: Duration = Duration::from_secs(60);
/// How often the schedule looks at the wall clock, so a machine that slept through a
/// check runs it soon after waking
const TICK: Duration = Duration::from_secs(60);

/// Why "Check now" didn't check
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckNowError {
    /// A check ran less than [`MIN_GAP`] ago
    CheckedRecently,
}

/// What the API shows
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub current: Version,
    pub channel: Channel,
    pub settings: UpdatesConfig,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    /// The releases newer than the running one, newest first; empty when up to date
    pub releases: Vec<ReleaseInfo>,
    pub dismissed_version: Option<String>,
    /// `(version, reason)` of a rollback the user hasn't dismissed
    pub rolled_back: Option<(String, String)>,
}

impl Snapshot {
    /// The newest release, if there is one
    pub fn latest(&self) -> Option<&ReleaseInfo> {
        self.releases.first()
    }

    /// The newest release's notice was hidden
    pub fn dismissed(&self) -> bool {
        match (self.latest(), &self.dismissed_version) {
            (Some(latest), Some(d)) => latest.version.to_string() == *d,
            _ => false,
        }
    }
}

type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// Where releases come from
enum Source {
    Github {
        client: reqwest::Client,
        base_url: String,
    },
    /// Answers without a server, for tests on Tokio's paused clock (where a real request
    /// would hit the client's timeout as the clock jumps ahead)
    #[cfg(test)]
    Fake(Arc<dyn Fn() -> Result<Fetched, FetchError> + Send + Sync>),
}

impl Source {
    async fn fetch(&self, etag: Option<&str>, current: Version) -> Result<Fetched, FetchError> {
        match self {
            Self::Github { client, base_url } => {
                github::fetch_releases(client, base_url, etag, current).await
            }
            #[cfg(test)]
            Self::Fake(answer) => answer(),
        }
    }
}

struct Inner {
    db: Option<Arc<Database>>,
    source: Source,
    clock: Clock,
    state: Mutex<Snapshot>,
    etag: Mutex<Option<String>>,
    /// Held for the length of a check, so the schedule and "Check now" never overlap
    checking: tokio::sync::Mutex<()>,
    started: std::sync::atomic::AtomicBool,
}

/// The update checker, shared by the daemon's schedule and the API
#[derive(Clone)]
pub struct UpdateService {
    inner: Arc<Inner>,
}

impl UpdateService {
    /// A checker for the running version, reading releases from [`github::api_base`]
    pub fn new(settings: UpdatesConfig, db: Option<Arc<Database>>, channel: Channel) -> Self {
        Self::with_options(
            settings,
            db,
            channel,
            Version::current(),
            Source::Github {
                client: github::client(),
                base_url: github::api_base(),
            },
            Arc::new(Utc::now),
        )
    }

    fn with_options(
        settings: UpdatesConfig,
        db: Option<Arc<Database>>,
        channel: Channel,
        current: Version,
        source: Source,
        clock: Clock,
    ) -> Self {
        let state = Snapshot {
            current,
            channel,
            settings,
            last_attempt_at: None,
            last_success_at: None,
            error: None,
            releases: Vec::new(),
            dismissed_version: None,
            rolled_back: None,
        };
        Self {
            inner: Arc::new(Inner {
                db,
                source,
                clock,
                state: Mutex::new(state),
                etag: Mutex::new(None),
                checking: tokio::sync::Mutex::new(()),
                started: Default::default(),
            }),
        }
    }

    /// A checker for tests: `current` as the running version, `base_url` for releases
    #[doc(hidden)]
    pub fn for_tests(
        settings: UpdatesConfig,
        db: Option<Arc<Database>>,
        current: Version,
        base_url: String,
    ) -> Self {
        Self::with_options(
            settings,
            db,
            Channel::Unknown,
            current,
            Source::Github {
                client: github::client(),
                base_url,
            },
            Arc::new(Utc::now),
        )
    }

    /// The same checker reporting `channel`, for tests of a channel's notice
    #[doc(hidden)]
    pub fn with_channel(self, channel: Channel) -> Self {
        self.inner.state.lock().unwrap().channel = channel;
        self
    }

    /// Read what the last run stored, then check [`FIRST_CHECK`] from now and every
    /// [`INTERVAL`] after. Needs a Tokio runtime; a second call does nothing.
    pub fn start(&self) {
        use std::sync::atomic::Ordering;
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let this = self.clone();
        tokio::spawn(async move {
            this.load().await;
            this.schedule().await;
        });
    }

    async fn schedule(&self) {
        let first = (self.inner.clock)() + chrono::Duration::from_std(FIRST_CHECK).unwrap();
        let mut due = first;
        loop {
            let now = (self.inner.clock)();
            if now >= due && self.settings().check {
                // One check however long the machine slept, then the next a day later
                due = now + chrono::Duration::from_std(INTERVAL).unwrap();
                self.check().await;
                continue;
            }
            // While checks are off the check stays due, so turning them on checks
            // within a tick
            let wait = match (due - now).to_std() {
                Ok(left) if !left.is_zero() => left.min(TICK),
                _ => TICK,
            };
            tokio::time::sleep(wait).await;
        }
    }

    /// Load the stored state (FR-004), so a restart shows a known update at once
    pub async fn load(&self) {
        let Some(db) = &self.inner.db else { return };
        let stored = match db.updates().get().await {
            Ok(stored) => stored,
            Err(e) => {
                tracing::warn!(error = %e, "Could not read the update state");
                return;
            }
        };
        let current = self.snapshot().current;
        let releases: Vec<ReleaseInfo> = stored
            .releases_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();
        // An upgrade since the last check leaves only what is still newer
        let releases: Vec<ReleaseInfo> = releases
            .into_iter()
            .filter(|r| r.version > current)
            .collect();
        putio_sync_telemetry::update::set_available(!releases.is_empty());
        *self.inner.etag.lock().unwrap() = stored.etag;
        let mut state = self.inner.state.lock().unwrap();
        state.last_attempt_at = stored.last_attempt_at;
        state.last_success_at = stored.last_success_at;
        state.error = stored.last_error;
        state.releases = releases;
        state.dismissed_version = stored.dismissed_version;
        state.rolled_back = stored.last_rolled_back.zip(stored.rollback_reason);
    }

    /// The settings the next check uses
    pub fn settings(&self) -> UpdatesConfig {
        self.inner.state.lock().unwrap().settings.clone()
    }

    /// Apply saved settings; the schedule reads them at its next tick
    pub fn configure(&self, settings: UpdatesConfig) {
        self.inner.state.lock().unwrap().settings = settings;
    }

    /// The state for the API
    pub fn snapshot(&self) -> Snapshot {
        self.inner.state.lock().unwrap().clone()
    }

    /// Check now, whatever the settings say (an explicit user action), unless a check ran
    /// less than [`MIN_GAP`] ago
    pub async fn check_now(&self) -> Result<Snapshot, CheckNowError> {
        let _guard = self.inner.checking.lock().await;
        let last = self.snapshot().last_attempt_at;
        let now = (self.inner.clock)();
        if last.is_some_and(|t| now - t < chrono::Duration::from_std(MIN_GAP).unwrap()) {
            return Err(CheckNowError::CheckedRecently);
        }
        self.check_locked().await;
        Ok(self.snapshot())
    }

    /// Run one check
    pub async fn check(&self) {
        let _guard = self.inner.checking.lock().await;
        self.check_locked().await;
    }

    async fn check_locked(&self) {
        let current = self.snapshot().current;
        let etag = self.inner.etag.lock().unwrap().clone();
        let now = (self.inner.clock)();
        let result = self.inner.source.fetch(etag.as_deref(), current).await;

        let (label, error, new) = match result {
            Ok(Fetched::NotModified) => ("not_modified", None, None),
            Ok(Fetched::Releases { etag, releases }) => ("ok", None, Some((etag, releases))),
            Err(FetchError::RateLimited) => (
                "rate_limited",
                Some(FetchError::RateLimited.to_string()),
                None,
            ),
            Err(e) => ("error", Some(e.to_string()), None),
        };
        putio_sync_telemetry::update::record_check(label);
        match &error {
            Some(e) => tracing::warn!(error = %e, "Update check failed"),
            None => tracing::debug!(result = label, "Update check finished"),
        }

        let mut stored_etag = None;
        let mut stored_json = None;
        {
            let mut state = self.inner.state.lock().unwrap();
            state.last_attempt_at = Some(now);
            state.error = error.clone();
            if error.is_none() {
                state.last_success_at = Some(now);
            }
            if let Some((etag, releases)) = new {
                if let Some(latest) = releases.first() {
                    if state.latest().map(|l| l.version) != Some(latest.version) {
                        tracing::info!(from = %current, to = %latest.version, "A newer version is available");
                    }
                }
                stored_json = serde_json::to_string(&releases).ok();
                state.releases = releases;
                *self.inner.etag.lock().unwrap() = etag.clone();
                stored_etag = etag;
            }
            putio_sync_telemetry::update::set_available(!state.releases.is_empty());
        }

        if let Some(db) = &self.inner.db {
            if let Err(e) = db
                .updates()
                .record_check(
                    now,
                    error.as_deref(),
                    stored_etag.as_deref(),
                    stored_json.as_deref(),
                )
                .await
            {
                tracing::warn!(error = %e, "Could not store the update check");
            }
        }
    }

    /// Hide the notice for `version`
    pub async fn dismiss(&self, version: &str) {
        self.inner.state.lock().unwrap().dismissed_version = Some(version.to_string());
        if let Some(db) = &self.inner.db {
            if let Err(e) = db.updates().dismiss(version).await {
                tracing::warn!(error = %e, "Could not store the dismissed update");
            }
        }
    }

    /// Hide the rollback message
    pub async fn clear_rollback(&self) {
        self.inner.state.lock().unwrap().rolled_back = None;
        if let Some(db) = &self.inner.db {
            if let Err(e) = db.updates().clear_rollback_message().await {
                tracing::warn!(error = %e, "Could not clear the update rollback");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    const ONE_NEWER: &str = r#"[{"tag_name": "v0.4.0", "published_at": "2026-11-15T10:00:00Z",
        "html_url": "u", "body": "", "assets": []}]"#;

    /// A wall clock that follows Tokio's paused clock
    fn tokio_clock() -> Clock {
        let start = tokio::time::Instant::now();
        let base = Utc::now();
        Arc::new(move || base + chrono::Duration::from_std(start.elapsed()).unwrap())
    }

    fn service(url: String, settings: UpdatesConfig, db: Option<Arc<Database>>) -> UpdateService {
        UpdateService::for_tests(settings, db, v("0.3.0"), url)
    }

    /// A service on Tokio's clock whose source counts its calls and lists 0.4.0
    fn counting(settings: UpdatesConfig) -> (UpdateService, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        let source = Source::Fake(Arc::new(move || {
            h.fetch_add(1, Ordering::SeqCst);
            Ok(Fetched::Releases {
                etag: None,
                releases: vec![ReleaseInfo {
                    version: "0.4.0".parse().unwrap(),
                    published_at: None,
                    html_url: "u".into(),
                    body: String::new(),
                    assets: vec![],
                }],
            })
        }));
        let s = UpdateService::with_options(
            settings,
            None,
            Channel::Unknown,
            v("0.3.0"),
            source,
            tokio_clock(),
        );
        (s, hits)
    }

    /// Let the spawned task run
    async fn settle() {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn checks_a_minute_after_start_then_daily() {
        let (s, hits) = counting(UpdatesConfig::default());
        s.start();
        tokio::time::sleep(Duration::from_secs(59)).await;
        settle().await;
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        tokio::time::sleep(Duration::from_secs(2)).await;
        settle().await;
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(s.snapshot().latest().unwrap().version, v("0.4.0"));

        tokio::time::sleep(INTERVAL - Duration::from_secs(120)).await;
        settle().await;
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        tokio::time::sleep(Duration::from_secs(180)).await;
        settle().await;
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn off_makes_no_request_until_turned_on() {
        let (s, hits) = counting(UpdatesConfig {
            check: false,
            ..Default::default()
        });
        s.start();
        tokio::time::sleep(INTERVAL * 2).await;
        settle().await;
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        s.configure(UpdatesConfig::default());
        tokio::time::sleep(TICK * 2).await;
        settle().await;
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        tokio::time::sleep(INTERVAL - TICK * 3).await;
        settle().await;
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn check_now_refuses_within_a_minute() {
        let (s, hits) = counting(UpdatesConfig {
            check: false,
            ..Default::default()
        });
        let snap = s.check_now().await.unwrap();
        assert_eq!(snap.releases.len(), 1);
        assert_eq!(s.check_now().await, Err(CheckNowError::CheckedRecently));
        tokio::time::sleep(MIN_GAP).await;
        assert!(s.check_now().await.is_ok());
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn stores_results_and_loads_them_at_start() {
        let dir = TempDir::new().unwrap();
        let db = Arc::new(Database::new(dir.path().join("test.db")).await.unwrap());
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/releases")
            .match_query(Matcher::Any)
            .with_header("etag", "\"e1\"")
            .with_body(ONE_NEWER)
            .create_async()
            .await;
        let s = service(server.url(), UpdatesConfig::default(), Some(db.clone()));
        s.check().await;
        s.dismiss("0.4.0").await;
        let stored = db.updates().get().await.unwrap();
        assert_eq!(stored.etag.as_deref(), Some("\"e1\""));
        assert!(stored.last_success_at.is_some());

        // a new process: the same release, the etag, and the dismissal, with no request
        let again = service(
            "http://127.0.0.1:9".into(),
            UpdatesConfig::default(),
            Some(db),
        );
        again.load().await;
        let snap = again.snapshot();
        assert_eq!(snap.latest().unwrap().version, v("0.4.0"));
        assert!(snap.dismissed());
        assert_eq!(again.inner.etag.lock().unwrap().as_deref(), Some("\"e1\""));
    }

    #[tokio::test]
    async fn a_failure_keeps_the_known_releases() {
        let mut server = mockito::Server::new_async().await;
        let ok = server
            .mock("GET", "/releases")
            .match_query(Matcher::Any)
            .with_body(ONE_NEWER)
            .create_async()
            .await;
        let s = service(server.url(), UpdatesConfig::default(), None);
        s.check().await;
        ok.remove_async().await;
        server
            .mock("GET", "/releases")
            .match_query(Matcher::Any)
            .with_status(500)
            .create_async()
            .await;
        s.check().await;
        let snap = s.snapshot();
        assert_eq!(snap.error.as_deref(), Some("GitHub answered 500"));
        assert_eq!(snap.releases.len(), 1);
    }
}
