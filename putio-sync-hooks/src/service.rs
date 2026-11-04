//! The hooks service: one dispatcher reading the bus, one queue and worker per hook.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use putio_sync_core::events::{self, Event, EventType, HookTest, Payload};
use putio_sync_core::hooks::{HookConfig, HookKind};
use putio_sync_telemetry::hooks as metrics;
use serde::Serialize;
use tokio::sync::{broadcast, Notify};

use crate::{command, webhook, HookRun, RunResult};

/// Events a hook may have waiting; past this the oldest is dropped.
const QUEUE_LIMIT: usize = 1000;
/// Runs kept per hook.
const HISTORY: usize = 20;

/// A hook as the API and the doctor show it.
#[derive(Debug, Clone, Serialize)]
pub struct HookStatus {
    /// The hook's name
    pub name: String,
    /// Whether it runs
    pub enabled: bool,
    /// `command` or `url`
    pub kind: HookKind,
    /// The redacted URL or the program path
    pub target: String,
    /// Its event filter
    pub events: Vec<String>,
    /// Its timeout
    pub timeout_secs: u64,
    /// Events waiting
    pub queued: usize,
    /// The last runs, newest first
    pub runs: Vec<HookRun>,
}

/// Why a test could not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestError {
    /// No hook has that name
    Unknown,
    /// A test of that hook is still running
    Busy,
}

struct Slot {
    config: RwLock<HookConfig>,
    queue: Mutex<VecDeque<Arc<Event>>>,
    wake: Notify,
    runs: Mutex<VecDeque<HookRun>>,
    removed: AtomicBool,
}

impl Slot {
    fn new(config: HookConfig) -> Arc<Slot> {
        metrics::record_configured(&config.name);
        Arc::new(Slot {
            config: RwLock::new(config),
            queue: Mutex::new(VecDeque::new()),
            wake: Notify::new(),
            runs: Mutex::new(VecDeque::new()),
            removed: AtomicBool::new(false),
        })
    }

    fn config(&self) -> HookConfig {
        self.config.read().expect("hook config lock").clone()
    }

    fn push(&self, event: Arc<Event>) {
        let mut queue = self.queue.lock().expect("hook queue lock");
        if queue.len() >= QUEUE_LIMIT {
            queue.pop_front();
            let name = self.config().name;
            metrics::record_dropped(&name, 1);
            tracing::warn!(hook = %name, "hook queue full; dropped the oldest event");
        }
        queue.push_back(event);
        drop(queue);
        self.wake.notify_one();
    }

    fn record(&self, run: HookRun) {
        let mut runs = self.runs.lock().expect("hook runs lock");
        runs.push_front(run);
        runs.truncate(HISTORY);
    }
}

struct Inner {
    slots: Mutex<Vec<Arc<Slot>>>,
    client: reqwest::Client,
    retry_delays: Vec<Duration>,
    started: AtomicBool,
    testing: Mutex<HashSet<String>>,
}

/// Runs the configured hooks for events on the bus. Cheap to clone.
#[derive(Clone)]
pub struct HooksService {
    inner: Arc<Inner>,
}

impl HooksService {
    /// A service for `hooks` (already validated); nothing runs until [`start`](Self::start).
    pub fn new(hooks: Vec<HookConfig>) -> HooksService {
        Self::with_retry_delays(hooks, webhook::RETRY_DELAYS.to_vec())
    }

    /// As [`new`](Self::new), with other webhook retry waits (for tests).
    pub fn with_retry_delays(hooks: Vec<HookConfig>, retry_delays: Vec<Duration>) -> HooksService {
        HooksService {
            inner: Arc::new(Inner {
                slots: Mutex::new(hooks.into_iter().map(Slot::new).collect()),
                client: webhook::client(),
                retry_delays,
                started: AtomicBool::new(false),
                testing: Mutex::new(HashSet::new()),
            }),
        }
    }

    /// Start reading the bus and running hooks. Needs a Tokio runtime; a second call
    /// does nothing.
    pub fn start(&self) {
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let rx = events::subscribe();
        for slot in self.slots() {
            self.spawn_worker(slot);
        }
        tokio::spawn(dispatch(self.inner.clone(), rx));
        let count = self.slots().len();
        if count > 0 {
            tracing::info!(hooks = count, "event hooks started");
        }
    }

    /// Replace the hooks. A hook keeping its name keeps its queue and history.
    pub fn configure(&self, hooks: Vec<HookConfig>) {
        let mut added = Vec::new();
        {
            let mut slots = self.inner.slots.lock().expect("hook slots lock");
            let mut next = Vec::with_capacity(hooks.len());
            for config in hooks {
                match slots.iter().position(|s| s.config().name == config.name) {
                    Some(i) => {
                        let slot = slots.remove(i);
                        *slot.config.write().expect("hook config lock") = config;
                        next.push(slot);
                    }
                    None => {
                        let slot = Slot::new(config);
                        added.push(slot.clone());
                        next.push(slot);
                    }
                }
            }
            for gone in slots.drain(..) {
                gone.removed.store(true, Ordering::SeqCst);
                gone.wake.notify_one();
            }
            *slots = next;
        }
        if self.inner.started.load(Ordering::SeqCst) {
            for slot in added {
                self.spawn_worker(slot);
            }
        }
    }

    /// Every hook with its last runs.
    pub fn status(&self) -> Vec<HookStatus> {
        self.slots()
            .iter()
            .map(|slot| {
                let config = slot.config();
                HookStatus {
                    target: config.target_label(),
                    kind: config.kind(),
                    enabled: config.enabled,
                    events: config.events.clone(),
                    timeout_secs: config.timeout_secs,
                    name: config.name,
                    queued: slot.queue.lock().map(|q| q.len()).unwrap_or(0),
                    runs: slot
                        .runs
                        .lock()
                        .map(|r| r.iter().cloned().collect())
                        .unwrap_or_default(),
                }
            })
            .collect()
    }

    /// The hooks, as configured.
    pub fn configs(&self) -> Vec<HookConfig> {
        self.slots().iter().map(|s| s.config()).collect()
    }

    /// Whether any enabled hook runs for `event_type`.
    pub fn wants(&self, event_type: EventType) -> bool {
        self.slots().iter().any(|s| {
            let c = s.config();
            c.enabled && c.matches(event_type)
        })
    }

    /// Send one `hook.test` event to the named hook now, enabled or not, without
    /// retries, and return the run.
    pub async fn test(&self, name: &str) -> Result<HookRun, TestError> {
        let slot = self
            .slots()
            .into_iter()
            .find(|s| s.config().name == name)
            .ok_or(TestError::Unknown)?;
        let _guard = TestGuard::claim(&self.inner, name).ok_or(TestError::Busy)?;
        let event = Event::new(Payload::HookTest(HookTest {
            hook: name.to_string(),
            message: "Test event from putio-sync".to_string(),
        }));
        let run = execute(&self.inner, &slot.config(), &event, &[]).await;
        slot.record(run.clone());
        Ok(run)
    }

    fn slots(&self) -> Vec<Arc<Slot>> {
        self.inner.slots.lock().expect("hook slots lock").clone()
    }

    fn spawn_worker(&self, slot: Arc<Slot>) {
        let inner = self.inner.clone();
        tokio::spawn(async move {
            loop {
                if slot.removed.load(Ordering::SeqCst) {
                    break;
                }
                let next = slot.queue.lock().expect("hook queue lock").pop_front();
                let Some(event) = next else {
                    slot.wake.notified().await;
                    continue;
                };
                let config = slot.config();
                let run = execute(&inner, &config, &event, &inner.retry_delays).await;
                slot.record(run);
            }
        });
    }
}

/// Marks a hook as testing until dropped, even when the caller goes away mid-test.
struct TestGuard<'a> {
    inner: &'a Inner,
    name: String,
}

impl<'a> TestGuard<'a> {
    fn claim(inner: &'a Inner, name: &str) -> Option<TestGuard<'a>> {
        let mut testing = inner.testing.lock().expect("hook testing lock");
        testing.insert(name.to_string()).then(|| TestGuard {
            inner,
            name: name.to_string(),
        })
    }
}

impl Drop for TestGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut testing) = self.inner.testing.lock() {
            testing.remove(&self.name);
        }
    }
}

async fn dispatch(inner: Arc<Inner>, mut rx: broadcast::Receiver<Arc<Event>>) {
    loop {
        match rx.recv().await {
            Ok(event) => {
                let slots = inner.slots.lock().expect("hook slots lock").clone();
                for slot in slots {
                    let config = slot.config();
                    if config.enabled && config.matches(event.event_type()) {
                        slot.push(event.clone());
                    }
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(missed = n, "event hooks fell behind the event bus");
                for slot in inner.slots.lock().expect("hook slots lock").iter() {
                    metrics::record_dropped(&slot.config().name, n);
                }
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Shortest value [`scrub`] masks, so short words in a header or env value are left alone.
const MIN_SECRET: usize = 6;

/// The values of `hook` that may be secrets: header and env values (and each word of
/// them, for `Bearer <token>`), and the URL's password, path segments past the first and
/// query values.
fn secrets(hook: &HookConfig) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in hook.headers.values().chain(hook.env.values()) {
        out.push(value.clone());
        out.extend(value.split_whitespace().map(str::to_string));
    }
    if let Some(url) = hook.url.as_deref().and_then(|u| reqwest::Url::parse(u).ok()) {
        out.extend(url.password().map(str::to_string));
        out.extend(url.path_segments().into_iter().flatten().skip(1).map(str::to_string));
        out.extend(url.query_pairs().map(|(_, value)| value.into_owned()));
    }
    out.retain(|value| value.len() >= MIN_SECRET);
    // Longest first, so a whole value is masked before a word inside it
    out.sort_by_key(|value| std::cmp::Reverse(value.len()));
    out.dedup();
    out
}

/// Mask the hook's own secrets in what it printed or answered, which can echo them
/// (a webhook quoting its URL, a script printing its env).
fn scrub(hook: &HookConfig, run: &mut HookRun) {
    let secrets = secrets(hook);
    if secrets.is_empty() {
        return;
    }
    let mask = |text: &mut String| {
        for secret in &secrets {
            if text.contains(secret.as_str()) {
                *text = text.replace(secret.as_str(), "***");
            }
        }
    };
    mask(&mut run.stdout);
    mask(&mut run.stderr);
    if let Some(error) = run.error.as_mut() {
        mask(error);
    }
}

/// Run one hook for one event, then log and count the run.
async fn execute(inner: &Inner, hook: &HookConfig, event: &Event, delays: &[Duration]) -> HookRun {
    let mut run = match hook.kind() {
        HookKind::Command => command::run(hook, event).await,
        HookKind::Url => webhook::run(&inner.client, hook, event, delays).await,
    };
    scrub(hook, &mut run);
    metrics::record_run(
        &hook.name,
        &run.event_type,
        run.result.as_str(),
        Duration::from_millis(run.duration_ms),
    );
    let target = hook.target_label();
    match run.result {
        RunResult::Success => tracing::info!(
            hook = %hook.name,
            target = %target,
            event_type = %run.event_type,
            event_id = %run.event_id,
            result = run.result.as_str(),
            duration_ms = run.duration_ms,
            exit_code = ?run.exit_code,
            http_status = ?run.http_status,
            stdout = %run.stdout,
            stderr = %run.stderr,
            "hook ran"
        ),
        _ => tracing::warn!(
            hook = %hook.name,
            target = %target,
            event_type = %run.event_type,
            event_id = %run.event_id,
            result = run.result.as_str(),
            duration_ms = run.duration_ms,
            attempts = run.attempts,
            exit_code = ?run.exit_code,
            http_status = ?run.http_status,
            error = run.error.as_deref().unwrap_or(""),
            stdout = %run.stdout,
            stderr = %run.stderr,
            "hook failed"
        ),
    }
    run
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HookRun, RunResult};

    #[test]
    fn scrub_masks_the_hooks_own_secrets() {
        let hook = HookConfig {
            name: "h".to_string(),
            url: Some("https://u:pass1234@h.lan/api/webhook/abcdef123?token=tok987654&x=1".into()),
            headers: [("Authorization".to_string(), "Bearer hdr123456".to_string())].into(),
            env: [("SHORT".to_string(), "abc".to_string())].into(),
            ..Default::default()
        };
        let mut run = HookRun {
            event_type: "hook.test".to_string(),
            event_id: String::new(),
            started_at: chrono::Utc::now(),
            duration_ms: 0,
            result: RunResult::Failure,
            attempts: 1,
            exit_code: None,
            http_status: Some(400),
            error: Some("bad tok987654".to_string()),
            stdout: "got Bearer hdr123456 on /api/webhook/abcdef123 as pass1234".to_string(),
            stderr: "abc stays".to_string(),
        };
        scrub(&hook, &mut run);
        assert_eq!(run.stdout, "got *** on /api/***/*** as ***");
        assert_eq!(run.error.as_deref(), Some("bad ***"));
        assert_eq!(run.stderr, "abc stays");
    }
}
