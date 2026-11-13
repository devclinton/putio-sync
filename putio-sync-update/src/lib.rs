//! Update checker and self-update (feature 055).
//!
//! [`UpdateService`] asks GitHub Releases for a newer version a minute after start and
//! then every 24 hours, keeps the answer in the database, and reports it to the API.
//! [`UpdateService::apply`] installs it: a verified swap and restart for the GitHub archive
//! install, the new installer for the `.pkg` and `-setup.exe` installs.

pub mod channel;
pub mod fetch;
pub mod github;
pub mod installer;
pub mod swap;
pub mod trial;
pub mod verify;
pub mod version;

use channel::{Channel, UpdateMode};
use chrono::{DateTime, Utc};
use fetch::{Plan, Progress, Target};
use github::{FetchError, Fetched, ReleaseInfo};
use putio_sync_core::config::UpdatesConfig;
use putio_sync_db::Database;
use serde::Serialize;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Why "Update now" didn't start
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// This channel updates through its own tool (FR-010)
    NotSupported(UpdateMode),
    /// Another attempt is running
    InProgress,
    /// Nothing newer is known
    NoUpdate,
    /// Transfers are running and the user didn't agree to interrupt them (FR-019)
    Busy(usize),
    /// The binary's folder can't be written (US3 scenario 6)
    NotWritable(PathBuf),
}

/// What an attempt downloads
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptKind {
    Patch,
    Full,
    Installer,
}

impl AttemptKind {
    fn label(self) -> &'static str {
        match self {
            Self::Patch => "patch",
            Self::Full => "full",
            Self::Installer => "installer",
        }
    }
}

impl From<fetch::Kind> for AttemptKind {
    fn from(kind: fetch::Kind) -> Self {
        match kind {
            fetch::Kind::Patch => Self::Patch,
            fetch::Kind::Full => Self::Full,
        }
    }
}

/// Where an attempt is (data-model.md "Attempt states")
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptState {
    WaitingForIdle,
    Downloading,
    Verifying,
    Swapping,
    Restarting,
    /// The installer is open on the desktop
    Opened,
    /// The installer is downloaded; the user opens it
    Downloaded,
    /// Nothing was replaced
    Failed,
}

impl AttemptState {
    /// Still going: a second "Update now" gets `InProgress`
    pub fn is_running(self) -> bool {
        matches!(
            self,
            Self::WaitingForIdle
                | Self::Downloading
                | Self::Verifying
                | Self::Swapping
                | Self::Restarting
        )
    }
}

/// One "Update now"
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attempt {
    pub to: Version,
    pub kind: AttemptKind,
    pub state: AttemptState,
    pub error: Option<String>,
    /// The downloaded installer
    pub path: Option<PathBuf>,
    /// Bytes downloaded so far
    pub downloaded_bytes: u64,
}

/// How many transfers and pipeline steps are running
pub type BusyFuture = Pin<Box<dyn Future<Output = usize> + Send>>;
type BusySource = Arc<dyn Fn() -> BusyFuture + Send + Sync>;
type Restart = Arc<dyn Fn() + Send + Sync>;

/// Where this install lives, for the swap and the trial
#[derive(Debug, Clone)]
struct Install {
    exe: PathBuf,
    config_dir: PathBuf,
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
    /// The last "Update now", while it runs and after it ends
    pub attempt: Option<Attempt>,
}

impl Snapshot {
    /// The newest release, if there is one
    pub fn latest(&self) -> Option<&ReleaseInfo> {
        self.releases.first()
    }

    /// What a swap to the newest release downloads on this platform
    pub fn plan(&self) -> Option<Plan> {
        fetch::plan(self.latest()?, self.current, Target::current()?)
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
    started: AtomicBool,
    /// Downloads release assets
    client: reqwest::Client,
    /// The key `SHA256SUMS` must be signed with
    pubkey: Mutex<String>,
    install: Mutex<Option<Install>>,
    busy: Mutex<Option<BusySource>>,
    restart: Mutex<Option<Restart>>,
    restart_requested: AtomicBool,
    progress: Progress,
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
            attempt: None,
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
                client: github::client(),
                pubkey: Mutex::new(verify::pubkey()),
                install: Mutex::new(None),
                busy: Mutex::new(None),
                restart: Mutex::new(None),
                restart_requested: AtomicBool::new(false),
                progress: Progress::default(),
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

    /// The same checker trusting another signing key, for tests with the test key
    #[doc(hidden)]
    pub fn with_pubkey(self, pubkey: &str) -> Self {
        *self.inner.pubkey.lock().unwrap() = pubkey.to_string();
        self
    }

    /// Where the running binary and the config folder are, so "Update now" can swap it
    pub fn with_install(self, exe: PathBuf, config_dir: PathBuf) -> Self {
        *self.inner.install.lock().unwrap() = Some(Install { exe, config_dir });
        self
    }

    /// How "Update now" counts running transfers and pipeline steps (FR-019)
    pub fn with_busy_source(self, busy: impl Fn() -> BusyFuture + Send + Sync + 'static) -> Self {
        *self.inner.busy.lock().unwrap() = Some(Arc::new(busy));
        self
    }

    /// What stops the daemon's services before the restart (the Ctrl+C path)
    pub fn with_restart(self, restart: impl Fn() + Send + Sync + 'static) -> Self {
        *self.inner.restart.lock().unwrap() = Some(Arc::new(restart));
        self
    }

    /// An update is installed and the daemon stopped for it: exec the new binary
    pub fn restart_requested(&self) -> bool {
        self.inner.restart_requested.load(Ordering::SeqCst)
    }

    /// Running transfers and pipeline steps
    pub async fn busy(&self) -> usize {
        let source = self.inner.busy.lock().unwrap().clone();
        match source {
            Some(source) => source().await,
            None => 0,
        }
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
        // A rollback the last start did (R8): keep it, so it shows and is never installed
        // automatically again
        let config_dir = self.install().map(|i| i.config_dir);
        if let Some((version, reason)) = config_dir.as_deref().and_then(trial::take_rolled_back) {
            tracing::warn!(%version, %reason, "The update to {version} was rolled back");
            if let Err(e) = db.updates().record_rollback(&version, &reason).await {
                tracing::warn!(error = %e, "Could not store the update rollback");
            }
        }
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
        let mut snapshot = self.inner.state.lock().unwrap().clone();
        if let Some(attempt) = &mut snapshot.attempt {
            attempt.downloaded_bytes = self.inner.progress.bytes.load(Ordering::Relaxed);
            if attempt.state == AttemptState::Downloading
                && self.inner.progress.verifying.load(Ordering::Relaxed)
            {
                attempt.state = AttemptState::Verifying;
            }
        }
        snapshot
    }

    fn install(&self) -> Option<Install> {
        self.inner.install.lock().unwrap().clone()
    }

    fn set_attempt(&self, change: impl FnOnce(&mut Attempt)) {
        if let Some(attempt) = &mut self.inner.state.lock().unwrap().attempt {
            change(attempt);
        }
    }

    /// "Update now" (T037): install the newest release. The work runs in the background;
    /// poll [`Self::snapshot`]'s `attempt`. `interrupt` agrees to stop running transfers,
    /// which resume after the restart.
    pub async fn apply(&self, interrupt: bool) -> Result<Snapshot, ApplyError> {
        let snapshot = self.snapshot();
        let mode = snapshot.channel.mode();
        let install = match (mode, self.install()) {
            (UpdateMode::Instructions, _) | (_, None) => {
                return Err(ApplyError::NotSupported(mode))
            }
            (_, Some(install)) => install,
        };
        if snapshot
            .attempt
            .as_ref()
            .is_some_and(|a| a.state.is_running())
        {
            return Err(ApplyError::InProgress);
        }
        let latest = snapshot
            .latest()
            .filter(|r| r.version > snapshot.current)
            .cloned()
            .ok_or(ApplyError::NoUpdate)?;
        let kind = match mode {
            UpdateMode::Swap => {
                if !interrupt {
                    let busy = self.busy().await;
                    if busy > 0 {
                        return Err(ApplyError::Busy(busy));
                    }
                }
                swap::check_writable(&install.exe).map_err(|e| match e {
                    swap::SwapError::NotWritable(path) => ApplyError::NotWritable(path),
                    swap::SwapError::Io(_) => ApplyError::NotWritable(
                        install.exe.parent().unwrap_or(&install.exe).to_path_buf(),
                    ),
                })?;
                snapshot.plan().map_or(AttemptKind::Full, |p| p.kind.into())
            }
            _ => AttemptKind::Installer,
        };
        {
            let mut state = self.inner.state.lock().unwrap();
            if state.attempt.as_ref().is_some_and(|a| a.state.is_running()) {
                return Err(ApplyError::InProgress);
            }
            self.inner.progress.reset();
            state.attempt = Some(Attempt {
                to: latest.version,
                kind,
                state: AttemptState::Downloading,
                error: None,
                path: None,
                downloaded_bytes: 0,
            });
        }
        tracing::info!(from = %snapshot.current, to = %latest.version, kind = kind.label(), "Update started");
        let this = self.clone();
        tokio::spawn(async move {
            let from = this.snapshot().current;
            let result = match kind {
                AttemptKind::Installer => this.run_installer(&latest, &install).await,
                _ => this.run_swap(&latest, &install).await,
            };
            if let Err((stage, error)) = result {
                tracing::warn!(%from, to = %latest.version, kind = kind.label(), stage, %error, "Update failed");
                putio_sync_telemetry::update::record_attempt(kind.label(), "failed");
                this.set_attempt(|a| {
                    a.state = AttemptState::Failed;
                    a.error = Some(error);
                });
            }
        });
        Ok(self.snapshot())
    }

    /// Download, verify, swap, then stop the daemon for the restart. `Err` carries the
    /// stage and the reason; nothing is replaced on any of them.
    async fn run_swap(
        &self,
        release: &ReleaseInfo,
        install: &Install,
    ) -> Result<(), (&'static str, String)> {
        let from = self.snapshot().current;
        let target = Target::current().ok_or((
            "downloading",
            "no release build for this platform".to_string(),
        ))?;
        let running = tokio::fs::read(&install.exe).await.map_err(|e| {
            (
                "downloading",
                format!("could not read the running binary: {e}"),
            )
        })?;
        let dir = install.exe.parent().unwrap_or(std::path::Path::new("."));
        let pubkey = self.inner.pubkey.lock().unwrap().clone();
        let prepared = fetch::prepare(
            &self.inner.client,
            release,
            from,
            &running,
            target,
            dir,
            &pubkey,
            &self.inner.progress,
        )
        .await
        .map_err(|e| {
            let stage = match e {
                fetch::PrepareError::Verify(_) | fetch::PrepareError::Decode(_) => "verifying",
                _ => "downloading",
            };
            (stage, e.to_string())
        })?;
        let kind = AttemptKind::from(prepared.kind);
        tracing::info!(%from, to = %release.version, kind = kind.label(), "Update verified");

        self.set_attempt(|a| {
            a.kind = kind;
            a.state = AttemptState::Swapping;
        });
        trial::begin(
            &install.config_dir,
            from,
            release.version,
            kind.label(),
            &install.exe,
            self.inner.db.as_deref(),
        )
        .await
        .map_err(|e| ("swapping", e))?;
        if let Err(e) = swap::install(&prepared.bytes, &install.exe) {
            trial::abandon(&install.config_dir);
            return Err(("swapping", e.to_string()));
        }
        tracing::info!(%from, to = %release.version, kind = kind.label(), "Binary replaced; restarting");

        self.set_attempt(|a| a.state = AttemptState::Restarting);
        self.inner.restart_requested.store(true, Ordering::SeqCst);
        let restart = self.inner.restart.lock().unwrap().clone();
        if let Some(restart) = restart {
            restart();
        }
        Ok(())
    }

    /// Download and verify the installer, then open it or report where it is
    async fn run_installer(
        &self,
        release: &ReleaseInfo,
        install: &Install,
    ) -> Result<(), (&'static str, String)> {
        let channel = self.snapshot().channel;
        let name = installer::asset_name(&channel, release.version)
            .ok_or(("downloading", "no installer for this platform".to_string()))?;
        let pubkey = self.inner.pubkey.lock().unwrap().clone();
        let path = installer::download(
            &self.inner.client,
            release,
            &name,
            &install.config_dir.join("updates"),
            &pubkey,
            &self.inner.progress,
        )
        .await
        .map_err(|e| ("downloading", e.to_string()))?;
        let (state, path) = match installer::open_or_report(path) {
            installer::Outcome::Opened(path) => (AttemptState::Opened, path),
            installer::Outcome::Downloaded(path) => (AttemptState::Downloaded, path),
        };
        tracing::info!(to = %release.version, path = %path.display(), ?state, "Installer ready");
        putio_sync_telemetry::update::record_attempt("installer", "done");
        self.set_attempt(|a| {
            a.state = state;
            a.path = Some(path);
        });
        Ok(())
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

    /// A GitHub-archive install of 0.3.0 in a temp folder that knows `release`
    async fn installed(release: ReleaseInfo) -> (UpdateService, TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("putio-sync");
        std::fs::write(&exe, b"old binary").unwrap();
        let source = Source::Fake(Arc::new(move || {
            Ok(Fetched::Releases {
                etag: None,
                releases: vec![release.clone()],
            })
        }));
        let s = UpdateService::with_options(
            UpdatesConfig::default(),
            None,
            Channel::GithubArchive,
            v("0.3.0"),
            source,
            Arc::new(Utc::now),
        )
        .with_pubkey(verify::tests::TEST_PUBKEY)
        .with_install(exe, dir.path().to_path_buf());
        s.check().await;
        (s, dir)
    }

    fn bare(version: &str) -> ReleaseInfo {
        ReleaseInfo {
            version: v(version),
            published_at: None,
            html_url: String::new(),
            body: String::new(),
            assets: vec![],
        }
    }

    /// Wait for the attempt to leave the running states (or reach `Restarting`)
    async fn finished(s: &UpdateService) -> Attempt {
        for _ in 0..500 {
            let attempt = s.snapshot().attempt.unwrap();
            if !attempt.state.is_running() || attempt.state == AttemptState::Restarting {
                return attempt;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the attempt did not finish: {:?}", s.snapshot().attempt);
    }

    #[tokio::test]
    async fn apply_needs_a_swap_or_installer_channel() {
        let s = UpdateService::with_options(
            UpdatesConfig::default(),
            None,
            Channel::Homebrew,
            v("0.3.0"),
            Source::Fake(Arc::new(|| Err(FetchError::Network("off".into())))),
            Arc::new(Utc::now),
        )
        .with_install("/x/putio-sync".into(), "/x".into());
        assert_eq!(
            s.apply(false).await,
            Err(ApplyError::NotSupported(UpdateMode::Instructions))
        );
        // A swap channel without an install (the API tests' service) can't either
        let s = s.with_channel(Channel::GithubArchive);
        *s.inner.install.lock().unwrap() = None;
        assert_eq!(
            s.apply(false).await,
            Err(ApplyError::NotSupported(UpdateMode::Swap))
        );
    }

    #[tokio::test]
    async fn apply_needs_a_newer_release() {
        let (s, _dir) = installed(bare("0.3.0")).await;
        assert_eq!(s.apply(false).await, Err(ApplyError::NoUpdate));
    }

    #[tokio::test]
    async fn apply_waits_for_transfers_unless_told_to_interrupt() {
        let (s, _dir) = installed(bare("0.4.0")).await;
        let s = s.with_busy_source(|| Box::pin(async { 2 }));
        assert_eq!(s.apply(false).await, Err(ApplyError::Busy(2)));
        assert!(s.snapshot().attempt.is_none());
        // interrupting starts it (and this release has no assets, so it fails)
        assert!(s.apply(true).await.is_ok());
        let attempt = finished(&s).await;
        assert_eq!(attempt.state, AttemptState::Failed);
        assert!(attempt.error.unwrap().contains("SHA256SUMS"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn apply_refuses_a_read_only_folder() {
        use std::os::unix::fs::PermissionsExt;
        let (s, dir) = installed(bare("0.4.0")).await;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = s.apply(false).await;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            result,
            Err(ApplyError::NotWritable(dir.path().to_path_buf()))
        );
    }

    #[tokio::test]
    async fn swaps_the_binary_and_asks_for_a_restart() {
        use verify::tests::{line, sign};
        let Some(target) = Target::current() else {
            return; // no release build for this platform
        };
        let to = v("0.4.0");
        let new = b"new binary".to_vec();
        let mut full_xz = Vec::new();
        lzma_rs::xz_compress(&mut &new[..], &mut full_xz).unwrap();
        let listed = format!(
            "{}{}",
            line(&full_xz, &target.full(to)),
            line(&new, &target.bin(to))
        );
        let mut server = mockito::Server::new_async().await;
        let mut release = bare("0.4.0");
        for (name, body) in [
            (target.full(to), full_xz),
            (fetch::SUMS.to_string(), listed.clone().into_bytes()),
            (
                fetch::SIGNATURE.to_string(),
                sign(listed.as_bytes()).into_bytes(),
            ),
        ] {
            let path = format!("/d/{name}");
            server
                .mock("GET", path.as_str())
                .with_body(&body)
                .create_async()
                .await;
            release.assets.push(github::Asset {
                name,
                url: format!("{}{path}", server.url()),
                size: body.len() as u64,
            });
        }
        let (s, dir) = installed(release).await;
        let restarts = Arc::new(AtomicUsize::new(0));
        let r = restarts.clone();
        let s = s.with_restart(move || {
            r.fetch_add(1, Ordering::SeqCst);
        });

        let started = s.apply(false).await.unwrap();
        assert_eq!(started.attempt.as_ref().unwrap().kind, AttemptKind::Full);
        assert_eq!(s.apply(false).await, Err(ApplyError::InProgress));
        let attempt = finished(&s).await;
        assert_eq!(attempt.state, AttemptState::Restarting, "{attempt:?}");
        assert_eq!(attempt.kind, AttemptKind::Full);

        let exe = dir.path().join("putio-sync");
        assert_eq!(std::fs::read(&exe).unwrap(), new);
        assert_eq!(std::fs::read(swap::old_path(&exe)).unwrap(), b"old binary");
        assert!(matches!(
            trial::read(dir.path()),
            Some(trial::Marker::Pending { .. })
        ));
        assert!(s.restart_requested());
        assert_eq!(restarts.load(Ordering::SeqCst), 1);
        // still restarting: a second click waits
        assert_eq!(s.apply(true).await, Err(ApplyError::InProgress));
    }
}
