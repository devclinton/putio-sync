//! The plugins service: finds plugins, keeps one worker per running plugin, feeds them
//! events from the bus and a tick every 60 s, and answers the API (status, test, reload).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use putio_sync_core::config::Config;
use putio_sync_core::events::{self, Event, EventType, Payload, PluginTest};
use putio_sync_core::plugins::{dir_for, PluginSettings};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::oneshot;

use crate::discover::{discover, Found};
use crate::host::{system_clock, Clock};
use crate::manifest::{ConfigField, Permissions};
use crate::worker::{Env, History, Job, PluginRun, Slot, Spec};

/// How often `on_tick` runs
pub const TICK: Duration = Duration::from_secs(60);
/// The message in a test event
pub const TEST_MESSAGE: &str = "Test event from putio-sync";

/// Where a plugin stands
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    /// Found, not enabled
    Disabled,
    /// Running
    Enabled,
    /// The manifest, the script, a value or `init` failed
    Broken,
    /// Enabled, but a host it needs is not approved
    NeedsApproval,
    /// Enabled, but a required value is unset
    MissingConfig,
}

/// One plugin, as `GET /plugins` shows it. Holds no secret value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginStatus {
    /// The plugin's name
    pub name: String,
    /// Its version, when the manifest parsed
    pub version: String,
    /// Its description
    pub description: String,
    /// `bundled` or `folder`
    pub source: String,
    /// The folder, for folder plugins
    pub path: Option<String>,
    /// Where it stands
    pub state: PluginState,
    /// Why it is broken, needs approval or misses config
    pub error: Option<String>,
    /// The events it handles
    pub events: Vec<String>,
    /// What it may do
    pub permissions: Permissions,
    /// Its settings form
    pub config_fields: Vec<ConfigField>,
    /// The hosts it would call with the current settings
    pub hosts_needed: Vec<String>,
    /// The hosts approved in its settings
    pub approved_hosts: Vec<String>,
    /// Whether it has `on_tick` (known once running)
    pub has_tick: bool,
    /// Jobs waiting
    pub queued: usize,
    /// The last runs, oldest first
    pub runs: Vec<PluginRun>,
}

/// Why a test could not run
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestError {
    /// No plugin of that name
    NotFound,
    /// A test of it is still running
    Running,
    /// It cannot load; the error says why
    Broken(String),
    /// The worker went away
    Unavailable,
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestError::NotFound => write!(f, "no such plugin"),
            TestError::Running => write!(f, "a test of this plugin is running"),
            TestError::Broken(e) => write!(f, "{}", e),
            TestError::Unavailable => write!(f, "the plugin stopped during the test"),
        }
    }
}

/// What the service starts from
#[derive(Clone, Default)]
pub struct PluginsOptions {
    /// The plugins folder
    pub dir: Option<PathBuf>,
    /// Where stores are kept (`<config dir>/plugin-data`); in memory when `None`
    pub data_dir: Option<PathBuf>,
    /// `[[plugins]]`
    pub settings: Vec<PluginSettings>,
    /// `putio.time.now()`; the system clock when `None`
    pub clock: Option<Clock>,
}

struct State {
    dir: Option<PathBuf>,
    found: Vec<Found>,
    settings: Vec<PluginSettings>,
    slots: HashMap<String, Arc<Slot>>,
    env: Option<Env>,
}

struct Inner {
    state: Mutex<State>,
    data_dir: Option<PathBuf>,
    clock: Clock,
    history: History,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Ok(state) = self.state.get_mut() {
            for slot in state.slots.values() {
                slot.stop();
            }
        }
    }
}

/// The plugins service; cheap to share behind an `Arc`
pub struct PluginsService {
    inner: Arc<Inner>,
}

/// The state a plugin would be in, with the reason
fn state_of(found: &Found, settings: Option<&PluginSettings>) -> (PluginState, Option<String>) {
    let Some(manifest) = found.loadable() else {
        return (PluginState::Broken, found.error.clone());
    };
    let Some(settings) = settings.filter(|s| s.enabled) else {
        return (PluginState::Disabled, None);
    };
    if let Err(e) = manifest.check_values(settings) {
        return (PluginState::Broken, Some(e));
    }
    let missing = manifest.missing(settings);
    if !missing.is_empty() {
        return (
            PluginState::MissingConfig,
            Some(format!("required: {}", missing.join(", "))),
        );
    }
    let unapproved: Vec<String> = manifest
        .hosts_needed(settings)
        .into_iter()
        .filter(|h| !settings.approved_hosts.contains(h))
        .collect();
    if !unapproved.is_empty() {
        return (
            PluginState::NeedsApproval,
            Some(format!("not approved: {}", unapproved.join(", "))),
        );
    }
    (PluginState::Enabled, None)
}

fn spec_of(found: &Found, settings: Option<&PluginSettings>) -> Option<Spec> {
    let manifest = found.loadable()?.clone();
    let settings = settings.cloned().unwrap_or_else(|| PluginSettings {
        name: manifest.name.clone(),
        ..PluginSettings::default()
    });
    let approved: HashSet<&String> = settings.approved_hosts.iter().collect();
    let allowed = manifest
        .hosts_needed(&settings)
        .into_iter()
        .filter(|h| approved.contains(h))
        .collect();
    Some(Spec {
        manifest,
        script: found.script.clone(),
        settings,
        allowed,
    })
}

impl PluginsService {
    /// A service with the plugins found in `options.dir`; nothing runs until [`start`]
    ///
    /// [`start`]: PluginsService::start
    pub fn new(options: PluginsOptions) -> PluginsService {
        let found = discover(options.dir.as_deref());
        PluginsService {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    dir: options.dir,
                    found,
                    settings: options.settings,
                    slots: HashMap::new(),
                    env: None,
                }),
                data_dir: options.data_dir,
                clock: options.clock.unwrap_or_else(system_clock),
                history: History::default(),
            }),
        }
    }

    fn env(&self, handle: tokio::runtime::Handle) -> Env {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(format!("putio-sync/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_default();
        Env {
            data_dir: self.inner.data_dir.clone(),
            client,
            handle,
            clock: self.inner.clock.clone(),
            history: self.inner.history.clone(),
        }
    }

    /// Start the enabled plugins, the event dispatcher and the ticker. Call from inside
    /// a Tokio runtime, once.
    pub fn start(&self) {
        let handle = tokio::runtime::Handle::current();
        {
            let mut state = self.inner.state.lock().unwrap();
            if state.env.is_some() {
                return;
            }
            state.env = Some(self.env(handle.clone()));
            apply(&mut state, false);
        }
        let weak = Arc::downgrade(&self.inner);
        let mut events = events::subscribe();
        handle.spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => {
                        let Some(inner) = weak.upgrade() else { break };
                        dispatch(&inner, event);
                    }
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(
                            missed = n,
                            "Plugins fell behind the event bus; events skipped"
                        )
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });
        let weak: Weak<Inner> = Arc::downgrade(&self.inner);
        handle.spawn(async move {
            let mut every = tokio::time::interval(TICK);
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            every.tick().await;
            loop {
                every.tick().await;
                let Some(inner) = weak.upgrade() else { break };
                let state = inner.state.lock().unwrap();
                for slot in state.slots.values().filter(|s| s.has_tick()) {
                    slot.push(Job::Tick);
                }
            }
        });
    }

    /// Use new settings now; running plugins rebuild their state
    pub fn configure(&self, settings: Vec<PluginSettings>) {
        let mut state = self.inner.state.lock().unwrap();
        state.settings = settings;
        apply(&mut state, true);
    }

    /// Find the plugins again; returns how many were found and how many are broken
    pub fn reload(&self) -> (usize, usize) {
        let mut state = self.inner.state.lock().unwrap();
        state.found = discover(state.dir.as_deref());
        apply(&mut state, true);
        let broken = state
            .found
            .iter()
            .filter(|f| f.loadable().is_none())
            .count();
        (state.found.len(), broken)
    }

    /// Use another plugins folder and reload
    pub fn set_dir(&self, dir: Option<PathBuf>) -> (usize, usize) {
        self.inner.state.lock().unwrap().dir = dir;
        self.reload()
    }

    /// The plugins folder
    pub fn dir(&self) -> Option<PathBuf> {
        self.inner.state.lock().unwrap().dir.clone()
    }

    /// Every plugin found, sorted by name
    pub fn status(&self) -> Vec<PluginStatus> {
        let state = self.inner.state.lock().unwrap();
        let history = self.inner.history.lock().unwrap();
        state
            .found
            .iter()
            .map(|found| {
                let settings = state.settings.iter().find(|s| s.name == found.name);
                let slot = state.slots.get(&found.name);
                let (mut plugin_state, mut error) = state_of(found, settings);
                if let Some(e) = slot.and_then(|s| s.load_error()) {
                    plugin_state = PluginState::Broken;
                    error = Some(e);
                }
                let manifest = found.manifest.as_ref();
                PluginStatus {
                    name: found.name.clone(),
                    version: manifest.map(|m| m.version.clone()).unwrap_or_default(),
                    description: manifest.map(|m| m.description.clone()).unwrap_or_default(),
                    source: found.source.as_str().to_string(),
                    path: found.source.path().map(|p| p.display().to_string()),
                    state: plugin_state,
                    error,
                    events: manifest.map(|m| m.events.clone()).unwrap_or_default(),
                    permissions: manifest.map(|m| m.permissions.clone()).unwrap_or_default(),
                    config_fields: manifest.map(|m| m.config.clone()).unwrap_or_default(),
                    hosts_needed: match (manifest, settings) {
                        (Some(m), Some(s)) => m.hosts_needed(s),
                        (Some(m), None) => m.hosts_needed(&PluginSettings::default()),
                        _ => Vec::new(),
                    },
                    approved_hosts: settings
                        .map(|s| s.approved_hosts.clone())
                        .unwrap_or_default(),
                    has_tick: slot.is_some_and(|s| s.has_tick()),
                    queued: slot.map(|s| s.queued()).unwrap_or(0),
                    runs: history
                        .get(&found.name)
                        .map(|r| r.iter().cloned().collect())
                        .unwrap_or_default(),
                }
            })
            .collect()
    }

    /// Whether a running plugin handles `event_type`
    pub fn wants(&self, event_type: EventType) -> bool {
        let state = self.inner.state.lock().unwrap();
        state
            .slots
            .values()
            .any(|s| s.spec().manifest.wants(event_type))
    }

    /// Send `name` a `plugin.test` event and return the run. A plugin that is not
    /// running is tested on a worker of its own.
    pub async fn test(&self, name: &str) -> Result<PluginRun, TestError> {
        let event = Arc::new(Event::new(Payload::PluginTest(PluginTest {
            plugin: name.to_string(),
            message: TEST_MESSAGE.to_string(),
        })));
        let (reply, answer) = oneshot::channel();
        {
            let state = self.inner.state.lock().unwrap();
            let found = state
                .found
                .iter()
                .find(|f| f.name == name)
                .ok_or(TestError::NotFound)?;
            if found.loadable().is_none() {
                return Err(TestError::Broken(
                    found
                        .error
                        .clone()
                        .unwrap_or_else(|| "cannot load".to_string()),
                ));
            }
            let settings = state.settings.iter().find(|s| s.name == name);
            if let Some(slot) = state.slots.get(name) {
                if !slot.begin_test() {
                    return Err(TestError::Running);
                }
                slot.push(Job::Test(event, reply));
            } else {
                let spec = spec_of(found, settings).ok_or(TestError::Unavailable)?;
                let env = match &state.env {
                    Some(env) => env.clone(),
                    None => self.env(tokio::runtime::Handle::current()),
                };
                let slot = Slot::new(spec);
                slot.begin_test();
                slot.push(Job::Test(event, reply));
                slot.finish();
                slot.spawn(env).map_err(|_| TestError::Unavailable)?;
            }
        }
        answer.await.map_err(|_| TestError::Unavailable)
    }

    /// Check settings before a save: values against each manifest, and the approved
    /// hosts of each enabled plugin set to the hosts it needs. Plugins not found are
    /// kept as they are.
    pub fn check(&self, settings: &[PluginSettings]) -> Result<Vec<PluginSettings>, String> {
        let state = self.inner.state.lock().unwrap();
        let mut out = Vec::with_capacity(settings.len());
        for s in settings {
            let mut s = s.clone();
            if let Some(m) = state
                .found
                .iter()
                .find(|f| f.name == s.name)
                .and_then(|f| f.loadable())
            {
                m.check_values(&s)?;
                if s.enabled {
                    s.approved_hosts = m.hosts_needed(&s);
                }
            }
            out.push(s);
        }
        Ok(out)
    }

    /// `<config dir>/plugin-data` for a config file at `config_path`
    pub fn data_dir_for(config_path: Option<&Path>) -> Option<PathBuf> {
        config_path
            .and_then(|p| p.parent())
            .map(|d| d.join("plugin-data"))
    }

    /// A service for `config` loaded from `config_path`: its `plugins_dir` (default
    /// `<config dir>/plugins`), stores in `<config dir>/plugin-data`, its `[[plugins]]`
    pub fn from_config(config: &Config, config_path: Option<&Path>) -> PluginsService {
        PluginsService::new(PluginsOptions {
            dir: dir_for(config.plugins_dir.as_deref(), config_path),
            data_dir: PluginsService::data_dir_for(config_path),
            settings: config.plugins.clone(),
            clock: None,
        })
    }
}

fn dispatch(inner: &Inner, event: Arc<Event>) {
    let event_type = event.event_type();
    if event_type.is_test() {
        return;
    }
    let state = inner.state.lock().unwrap();
    for slot in state.slots.values() {
        if slot.spec().manifest.wants(event_type) {
            slot.push(Job::Event(event.clone()));
        }
    }
}

/// Start, update and stop workers to match what is found and the settings. `changed`
/// makes running workers rebuild.
fn apply(state: &mut State, changed: bool) {
    let Some(env) = state.env.clone() else {
        return;
    };
    let mut keep = HashSet::new();
    for found in &state.found {
        let settings = state.settings.iter().find(|s| s.name == found.name);
        if state_of(found, settings).0 != PluginState::Enabled {
            continue;
        }
        let Some(spec) = spec_of(found, settings) else {
            continue;
        };
        keep.insert(found.name.clone());
        putio_sync_telemetry::plugins::record_configured(&found.name);
        match state.slots.get(&found.name) {
            Some(slot) => {
                if changed {
                    slot.update(spec);
                }
            }
            None => {
                let slot = Slot::new(spec);
                match slot.spawn(env.clone()) {
                    Ok(()) => {
                        tracing::info!(plugin = %found.name, "Plugin started");
                        state.slots.insert(found.name.clone(), slot);
                    }
                    Err(e) => {
                        tracing::error!(plugin = %found.name, error = %e, "Plugin thread not started")
                    }
                }
            }
        }
    }
    state.slots.retain(|name, slot| {
        let kept = keep.contains(name);
        if !kept {
            slot.stop();
            tracing::info!(plugin = %name, "Plugin stopped");
        }
        kept
    });
}
