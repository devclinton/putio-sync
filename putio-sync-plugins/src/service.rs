//! The plugins service: finds plugins, keeps one worker per running plugin, feeds them
//! events from the bus and their ticks, and answers the API (status, test, reload, and
//! since 031 settings actions and health checks).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use putio_sync_core::config::Config;
use putio_sync_core::events::{self, Event, EventType, Payload, PluginTest};
use putio_sync_core::plugins::{dir_for, PluginSettings};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::oneshot;

use crate::action::{self, ActionResult, HealthItem};
use crate::discover::{discover, Found};
use crate::hooks::HookSlot;
use crate::host::{system_clock, Clock};
use crate::manifest::{Action, ConfigField, Manifest, Permissions};
use crate::worker::{CallReply, Env, History, Job, PluginRun, RunResult, Slot, Spec, MAX_RUNS};

/// How often `on_tick` runs when the manifest sets no `tick_seconds`
pub const TICK: Duration = Duration::from_secs(60);
/// How often the ticker looks for plugins whose tick is due
pub const BASE_TICK: Duration = Duration::from_secs(5);
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
    /// Enabled, but a host or folder it needs is not approved
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
    /// The sync rules it provides: `filter`, `rename`, `folder` (feature 028)
    #[serde(default)]
    pub hooks: Vec<String>,
    /// What it may do
    pub permissions: Permissions,
    /// Its settings form
    pub config_fields: Vec<ConfigField>,
    /// Its settings buttons, hidden ones included (031)
    pub actions: Vec<Action>,
    /// The hosts it would call with the current settings
    pub hosts_needed: Vec<String>,
    /// The hosts approved in its settings
    pub approved_hosts: Vec<String>,
    /// `lua` or `wasm` (029)
    #[serde(default)]
    pub runtime: String,
    /// The `r:`/`w:` folders it would use with the current settings (029)
    #[serde(default)]
    pub paths_needed: Vec<String>,
    /// The folders approved in its settings
    #[serde(default)]
    pub approved_paths: Vec<String>,
    /// Whether it has `on_tick` (known once running)
    pub has_tick: bool,
    /// Jobs waiting
    pub queued: usize,
    /// The last runs, oldest first
    pub runs: Vec<PluginRun>,
    /// Who made it and where to read more, as its manifest says (059)
    #[serde(default)]
    pub links: Links,
}

/// A plugin's author, license and links; unset ones are left out
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Links {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issues: Option<String>,
}

impl Links {
    /// The links `manifest` sets
    pub fn of(manifest: &Manifest) -> Links {
        let set = |s: &String| (!s.is_empty()).then(|| s.clone());
        Links {
            author: set(&manifest.author),
            license: set(&manifest.license),
            homepage: set(&manifest.homepage),
            docs: set(&manifest.docs),
            repository: set(&manifest.repository),
            issues: set(&manifest.issues),
        }
    }
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

/// Why an action or health check could not give an answer
#[derive(Debug, Clone, PartialEq)]
pub enum CallError {
    /// No plugin of that name
    NotFound,
    /// The plugin has no such action
    NoAction,
    /// An action or health check of this plugin is running
    Busy,
    /// It cannot load, or a value is wrong; the error says why
    Broken(String),
    /// The function failed; the run says why
    Failed(Box<PluginRun>),
    /// It returned something the host refuses
    Invalid(String),
    /// The worker went away
    Unavailable,
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::NotFound => write!(f, "no such plugin"),
            CallError::NoAction => write!(f, "no such action"),
            CallError::Busy => write!(f, "an action or check of this plugin is running"),
            CallError::Broken(e) | CallError::Invalid(e) => write!(f, "{}", e),
            CallError::Failed(run) => write!(
                f,
                "{}",
                run.error
                    .clone()
                    .unwrap_or_else(|| run.result.as_str().to_string())
            ),
            CallError::Unavailable => write!(f, "the plugin stopped during the call"),
        }
    }
}

/// What an action gave back
#[derive(Debug, Clone)]
pub struct ActionAnswer {
    /// The checked result, secrets replaced
    pub result: ActionResult,
    /// The run, logs included
    pub run: PluginRun,
    /// The plugin's settings with the saved values, already checked; `None` when
    /// nothing was saved
    pub settings: Option<PluginSettings>,
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
    /// `putio.download_dir`
    pub download_dir: Option<PathBuf>,
}

struct State {
    dir: Option<PathBuf>,
    found: Vec<Found>,
    settings: Vec<PluginSettings>,
    slots: HashMap<String, Arc<Slot>>,
    /// The second worker of each running plugin with sync hooks
    hook_slots: HashMap<String, Arc<HookSlot>>,
    env: Option<Env>,
}

struct Inner {
    state: Mutex<State>,
    data_dir: Option<PathBuf>,
    clock: Clock,
    history: History,
    stores: crate::worker::Stores,
    counters: crate::worker::CounterNames,
    download_dir: crate::worker::DownloadDir,
    busy: Mutex<HashSet<String>>,
    /// The workbench's last runs (055)
    runs: crate::workbench::Runs,
}

/// Holds a plugin's place in the busy set until dropped
pub(crate) struct BusyGuard<'a> {
    inner: &'a Inner,
    name: String,
}

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.inner.busy.lock().unwrap().remove(&self.name);
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Ok(state) = self.state.get_mut() {
            for slot in state.slots.values() {
                slot.stop();
            }
            for hook in state.hook_slots.values() {
                hook.slot.stop();
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
        .chain(
            manifest
                .paths_needed(settings)
                .into_iter()
                .filter(|p| !settings.approved_paths.contains(p)),
        )
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
    let paths = manifest
        .paths_needed(&settings)
        .into_iter()
        .filter(|p| settings.approved_paths.contains(p))
        .collect();
    Some(Spec {
        manifest,
        script: found.script.clone(),
        wasm: found.wasm.clone(),
        settings,
        allowed,
        paths,
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
                    hook_slots: HashMap::new(),
                    env: None,
                }),
                data_dir: options.data_dir,
                clock: options.clock.unwrap_or_else(system_clock),
                history: History::default(),
                stores: Default::default(),
                counters: Default::default(),
                download_dir: Arc::new(Mutex::new(options.download_dir)),
                busy: Mutex::new(HashSet::new()),
                runs: Default::default(),
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
            stores: self.inner.stores.clone(),
            counters: self.inner.counters.clone(),
            download_dir: self.inner.download_dir.clone(),
            tap: None,
        }
    }

    /// The workers' shared environment, or a new one before [`PluginsService::start`]
    pub(crate) fn current_env(&self) -> Env {
        let state = self.inner.state.lock().unwrap();
        match &state.env {
            Some(env) => env.clone(),
            None => self.env(tokio::runtime::Handle::current()),
        }
    }

    /// `name` as found, broken or not, with its saved settings (disabled defaults when none)
    pub(crate) fn found_with_settings(&self, name: &str) -> Option<(Found, PluginSettings)> {
        let state = self.inner.state.lock().unwrap();
        let found = state.found.iter().find(|f| f.name == name)?.clone();
        let settings = state
            .settings
            .iter()
            .find(|s| s.name == name)
            .cloned()
            .unwrap_or_else(|| PluginSettings {
                name: name.to_string(),
                ..PluginSettings::default()
            });
        Some((found, settings))
    }

    /// The spec a worker for `found` would get
    pub(crate) fn spec_for(&self, found: &Found, settings: &PluginSettings) -> Option<Spec> {
        spec_of(found, Some(settings))
    }

    pub(crate) fn workbench_runs_lock(&self) -> &crate::workbench::Runs {
        &self.inner.runs
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
            apply(&mut state, Changed::Nothing);
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
            let mut every = tokio::time::interval(BASE_TICK);
            every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            every.tick().await;
            loop {
                every.tick().await;
                let Some(inner) = weak.upgrade() else { break };
                PluginsService { inner }.tick_due();
            }
        });
    }

    /// Queue `on_tick` for each running plugin that has one
    pub fn tick(&self) {
        let state = self.inner.state.lock().unwrap();
        for slot in state.slots.values().filter(|s| s.has_tick()) {
            slot.push(Job::Tick);
        }
    }

    /// Queue `on_tick` for each running plugin whose `tick_seconds` have passed (the
    /// ticker calls this every [`BASE_TICK`])
    pub fn tick_due(&self) {
        let now = Instant::now();
        let state = self.inner.state.lock().unwrap();
        for slot in state.slots.values().filter(|s| s.has_tick()) {
            if slot.tick_due(now) {
                slot.push(Job::Tick);
            }
        }
    }

    /// Set `putio.download_dir`; running plugins see it after their next rebuild
    pub fn set_download_dir(&self, dir: Option<PathBuf>) {
        *self.inner.download_dir.lock().unwrap() = dir;
    }

    /// Use new settings now; running plugins rebuild their state
    pub fn configure(&self, settings: Vec<PluginSettings>) {
        let mut state = self.inner.state.lock().unwrap();
        state.settings = settings;
        apply(&mut state, Changed::All);
    }

    /// Find the plugins again; returns how many were found and how many are broken
    pub fn reload(&self) -> (usize, usize) {
        let mut state = self.inner.state.lock().unwrap();
        state.found = discover(state.dir.as_deref());
        apply(&mut state, Changed::All);
        let broken = state
            .found
            .iter()
            .filter(|f| f.loadable().is_none())
            .count();
        (state.found.len(), broken)
    }

    /// Find the plugins again, rebuilding only `names`' workers; the others keep their
    /// state (055: a workbench save)
    pub fn reload_only(&self, names: &[String]) {
        let mut state = self.inner.state.lock().unwrap();
        state.found = discover(state.dir.as_deref());
        apply(&mut state, Changed::Only(names));
    }

    /// Install a plugin into the plugins folder and reload. A new plugin starts disabled.
    pub fn install(
        &self,
        package: crate::install::Package,
        replace: bool,
    ) -> Result<String, crate::install::InstallError> {
        let dir = self.dir().ok_or(crate::install::InstallError::NoFolder)?;
        let name = crate::install::install(&dir, package, replace)?;
        self.reload();
        Ok(name)
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
                let hook_error = state
                    .hook_slots
                    .get(&found.name)
                    .and_then(|h| h.slot.load_error());
                if let Some(e) = slot.and_then(|s| s.load_error()).or(hook_error) {
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
                    hooks: manifest.map(|m| m.hooks.clone()).unwrap_or_default(),
                    permissions: manifest.map(|m| m.permissions.clone()).unwrap_or_default(),
                    config_fields: manifest.map(|m| m.config.clone()).unwrap_or_default(),
                    actions: manifest.map(|m| m.actions.clone()).unwrap_or_default(),
                    hosts_needed: match (manifest, settings) {
                        (Some(m), Some(s)) => m.hosts_needed(s),
                        (Some(m), None) => m.hosts_needed(&PluginSettings::default()),
                        _ => Vec::new(),
                    },
                    approved_hosts: settings
                        .map(|s| s.approved_hosts.clone())
                        .unwrap_or_default(),
                    runtime: manifest.map(|m| m.runtime.clone()).unwrap_or_default(),
                    paths_needed: match (manifest, settings) {
                        (Some(m), Some(s)) => m.paths_needed(s),
                        (Some(m), None) => m.paths_needed(&PluginSettings::default()),
                        _ => Vec::new(),
                    },
                    approved_paths: settings
                        .map(|s| s.approved_paths.clone())
                        .unwrap_or_default(),
                    has_tick: slot.is_some_and(|s| s.has_tick()),
                    queued: slot.map(|s| s.queued()).unwrap_or(0),
                    runs: history
                        .get(&found.name)
                        .map(|r| r.iter().cloned().collect())
                        .unwrap_or_default(),
                    links: manifest.map(Links::of).unwrap_or_default(),
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
        let dir = self.dir();
        self.check_at(dir.as_deref(), settings)
    }

    /// [`check`](Self::check) against the plugins in `dir`, which a save may be about
    /// to switch to
    pub fn check_at(
        &self,
        dir: Option<&Path>,
        settings: &[PluginSettings],
    ) -> Result<Vec<PluginSettings>, String> {
        let state = self.inner.state.lock().unwrap();
        let other;
        let found = if state.dir.as_deref() == dir {
            &state.found
        } else {
            other = discover(dir);
            &other
        };
        let mut out = Vec::with_capacity(settings.len());
        for s in settings {
            let mut s = s.clone();
            if let Some(m) = found
                .iter()
                .find(|f| f.name == s.name)
                .and_then(|f| f.loadable())
            {
                m.check_values(&s)?;
                if s.enabled {
                    s.approved_hosts = m.hosts_needed(&s);
                    s.approved_paths = m.paths_needed(&s);
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
            download_dir: Some(config.download.target_directory.clone()),
        })
    }

    /// The running plugins' hook workers
    pub(crate) fn hook_workers(&self) -> Vec<Arc<HookSlot>> {
        let state = self.inner.state.lock().unwrap();
        state.hook_slots.values().cloned().collect()
    }

    /// Keep a run with the plugin's last runs, and count it
    pub(crate) fn keep_run(&self, name: &str, run: PluginRun) {
        putio_sync_telemetry::plugins::record_run(
            name,
            &run.event_type,
            run.result.as_str(),
            Duration::ZERO,
        );
        let mut history = self.inner.history.lock().unwrap();
        let runs = history.entry(name.to_string()).or_default();
        if runs.len() >= MAX_RUNS {
            runs.pop_front();
        }
        runs.push_back(run);
    }

    pub(crate) fn begin(&self, name: &str) -> Result<BusyGuard<'_>, CallError> {
        if !self.inner.busy.lock().unwrap().insert(name.to_string()) {
            return Err(CallError::Busy);
        }
        Ok(BusyGuard {
            inner: &self.inner,
            name: name.to_string(),
        })
    }

    /// Run `job` for `spec` on a worker of its own and wait for the answer
    async fn call_once(
        &self,
        spec: Spec,
        function: &str,
        kind: &str,
        args: Vec<Value>,
    ) -> Result<CallReply, CallError> {
        let (reply, answer) = oneshot::channel();
        let env = {
            let state = self.inner.state.lock().unwrap();
            match &state.env {
                Some(env) => env.clone(),
                None => self.env(tokio::runtime::Handle::current()),
            }
        };
        let slot = Slot::new(spec);
        slot.push(Job::Call {
            function: function.to_string(),
            kind: kind.to_string(),
            args,
            limit: None,
            reply,
        });
        slot.finish();
        slot.spawn(env).map_err(|_| CallError::Unavailable)?;
        answer.await.map_err(|_| CallError::Unavailable)
    }

    /// The loadable plugin `name` and its saved settings (disabled defaults when none)
    fn lookup(&self, name: &str) -> Result<(Found, PluginSettings), CallError> {
        let state = self.inner.state.lock().unwrap();
        let found = state
            .found
            .iter()
            .find(|f| f.name == name)
            .ok_or(CallError::NotFound)?;
        if found.loadable().is_none() {
            return Err(CallError::Broken(
                found
                    .error
                    .clone()
                    .unwrap_or_else(|| "cannot load".to_string()),
            ));
        }
        let settings = state
            .settings
            .iter()
            .find(|s| s.name == name)
            .cloned()
            .unwrap_or_else(|| PluginSettings {
                name: name.to_string(),
                ..PluginSettings::default()
            });
        Ok((found.clone(), settings))
    }

    /// Run settings action `id` of `name` (031) with `input`. `overrides` are unsaved
    /// form values that are not secret (`null` clears one) and count for this call only.
    /// The plugin may call every host its settings need, approved or not: the person is
    /// setting it up. One action or health check per plugin at a time.
    pub async fn action(
        &self,
        name: &str,
        id: &str,
        input: Value,
        overrides: serde_json::Map<String, Value>,
    ) -> Result<ActionAnswer, CallError> {
        let (found, saved) = self.lookup(name)?;
        let manifest = found.loadable().cloned().ok_or(CallError::NotFound)?;
        if !manifest.has_action(id) {
            return Err(CallError::NoAction);
        }
        let mut merged = saved.clone();
        for (key, value) in overrides {
            if manifest.config.iter().any(|f| f.key == key && f.secret) {
                continue;
            }
            if value.is_null() {
                merged.config.remove(&key);
            } else {
                merged.config.insert(key, value);
            }
        }
        manifest.check_values(&merged).map_err(CallError::Broken)?;
        let _busy = self.begin(name)?;
        let spec = Spec {
            allowed: manifest.hosts_needed(&merged).into_iter().collect(),
            paths: manifest.paths_needed(&merged),
            manifest: manifest.clone(),
            script: found.script.clone(),
            wasm: found.wasm.clone(),
            settings: merged.clone(),
        };
        let reply = self
            .call_once(spec, "on_action", "action", vec![Value::from(id), input])
            .await?;
        if !reply.defined {
            return Err(CallError::Invalid(format!(
                "{} declares actions but has no on_action function",
                name
            )));
        }
        let secrets: Vec<String> = merged.secrets.values().cloned().collect();
        let Some(value) = reply
            .value
            .filter(|_| reply.run.result == RunResult::Success)
        else {
            return Err(CallError::Failed(Box::new(reply.run)));
        };
        let result = action::parse(&manifest, value, &secrets).map_err(CallError::Invalid)?;
        let settings = match &result.save {
            None => None,
            Some(save) => {
                let mut next = saved;
                for (key, value) in &save.config {
                    if value.is_null() {
                        next.config.remove(key);
                    } else {
                        next.config.insert(key.clone(), value.clone());
                    }
                }
                for (key, value) in &save.secrets {
                    match value {
                        Some(v) => next.secrets.insert(key.clone(), v.clone()),
                        None => next.secrets.remove(key),
                    };
                }
                manifest
                    .check_values(&next)
                    .map_err(|e| CallError::Invalid(format!("save refused: {}", e)))?;
                Some(next)
            }
        };
        let mut run = reply.run;
        let mut all = secrets;
        if let Some(save) = &result.save {
            all.extend(save.secrets.values().flatten().cloned());
        }
        run.error = run.error.map(|e| crate::host::scrub(&e, &all));
        for line in &mut run.logs {
            line.message = crate::host::scrub(&line.message, &all);
        }
        Ok(ActionAnswer {
            result,
            run,
            settings,
        })
    }

    /// Ask `name`'s `health()` how it stands (031), with its saved settings and approved
    /// hosts. `Ok(None)` when the plugin has no `health` function.
    pub async fn health(&self, name: &str) -> Result<Option<Vec<HealthItem>>, CallError> {
        let (found, settings) = self.lookup(name)?;
        let spec = spec_of(&found, Some(&settings)).ok_or(CallError::NotFound)?;
        spec.manifest
            .check_values(&settings)
            .map_err(CallError::Broken)?;
        let secrets: Vec<String> = settings.secrets.values().cloned().collect();
        let _busy = self.begin(name)?;
        let reply = self.call_once(spec, "health", "health", Vec::new()).await?;
        if !reply.defined {
            return Ok(None);
        }
        match reply.value {
            Some(value) if reply.run.result == RunResult::Success => {
                Ok(Some(action::parse_health(value, &secrets)))
            }
            _ => Ok(Some(vec![HealthItem {
                status: action::HealthStatus::Fail,
                message: format!(
                    "health check failed: {}",
                    reply
                        .run
                        .error
                        .unwrap_or_else(|| reply.run.result.as_str().to_string())
                ),
            }])),
        }
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

/// Which running workers [`apply`] rebuilds
enum Changed<'a> {
    Nothing,
    All,
    Only(&'a [String]),
}

/// Start, update and stop workers to match what is found and the settings. `changed`
/// running workers rebuild.
fn apply(state: &mut State, changed: Changed) {
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
        let changed = match changed {
            Changed::Nothing => false,
            Changed::All => true,
            Changed::Only(names) => names.contains(&found.name),
        };
        putio_sync_telemetry::plugins::record_configured(&found.name);
        if spec.manifest.hooks.is_empty() {
            if let Some(hook) = state.hook_slots.remove(&found.name) {
                hook.slot.stop();
            }
        } else {
            match state.hook_slots.get(&found.name) {
                Some(hook) => {
                    if changed {
                        hook.slot.update(spec.clone());
                        let fresh = Arc::new(HookSlot::new(hook.slot.clone()));
                        state.hook_slots.insert(found.name.clone(), fresh);
                    }
                }
                None => {
                    let slot = Slot::new(spec.clone());
                    match slot.spawn(env.clone()) {
                        Ok(()) => {
                            state
                                .hook_slots
                                .insert(found.name.clone(), Arc::new(HookSlot::new(slot)));
                        }
                        Err(e) => tracing::error!(
                            plugin = %found.name,
                            error = %e,
                            "Plugin sync hook thread not started"
                        ),
                    }
                }
            }
        }
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
    state.hook_slots.retain(|name, hook| {
        let kept = keep.contains(name);
        if !kept {
            hook.slot.stop();
        }
        kept
    });
    state.slots.retain(|name, slot| {
        let kept = keep.contains(name);
        if !kept {
            slot.stop();
            tracing::info!(plugin = %name, "Plugin stopped");
        }
        kept
    });
}
