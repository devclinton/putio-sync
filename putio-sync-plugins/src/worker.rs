//! One plugin's worker (research R3): a thread that owns the plugin's Lua state, or its
//! WebAssembly instance (029), and runs the jobs queued for it, one at a time, each under
//! the plugin's time limit. Both runtimes are called the same way: JSON arguments in, JSON
//! out.
//!
//! A timeout or memory error (or a WebAssembly trap) throws the state away; the next job
//! builds a new one (load the script or module, call `init`). A state that cannot be built
//! stays broken until the next reload or settings change.

use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use mlua::{Function, Lua, Table, Value as LuaValue};
use putio_sync_core::events::Event;
use putio_sync_core::plugins::PluginSettings;
use putio_sync_core::sync_hooks::Hook;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::host::{self, Clock, Counters, Files, Host, Http, LogLine, Logs};
use crate::manifest::Manifest;
use crate::sandbox::{self, Deadline, TIME_LIMIT};
use crate::store::{SharedStore, Store};
use crate::wasm::{self, Failure};
use crate::workbench::{Tap, MAX_WORKBENCH_LOG_LINES};

/// Jobs kept per plugin; the oldest event is dropped past this
pub const MAX_QUEUE: usize = 1000;
/// Runs kept per plugin
pub const MAX_RUNS: usize = 20;
/// A plugin thread's stack: room for the WebAssembly interpreter's translator and Lua's
/// C calls, with a margin for debug builds
const STACK_BYTES: usize = 8 * 1024 * 1024;

/// How a run ended
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunResult {
    /// The function returned
    Success,
    /// It raised an error, or the state could not be built
    Failure,
    /// It ran past the time limit
    Timeout,
}

impl RunResult {
    /// The metric and API label
    pub fn as_str(&self) -> &'static str {
        match self {
            RunResult::Success => "success",
            RunResult::Failure => "failure",
            RunResult::Timeout => "timeout",
        }
    }
}

/// One call into a plugin
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginRun {
    /// An event type, `tick` or `init`
    pub event_type: String,
    /// The event's id; empty for `tick` and `init`
    pub event_id: String,
    /// RFC 3339
    pub started_at: String,
    /// Wall clock, host calls included
    pub duration_ms: u64,
    /// How it ended
    pub result: RunResult,
    /// Why it failed, secrets replaced
    pub error: Option<String>,
    /// What it logged (test runs only)
    pub logs: Vec<LogLine>,
}

/// Recent runs by plugin name, kept across reloads
pub type History = Arc<Mutex<HashMap<String, VecDeque<PluginRun>>>>;

/// Each plugin's store, opened once and shared by all its workers
pub type Stores = Arc<Mutex<HashMap<String, SharedStore>>>;

/// Each plugin's counter names, shared by all its workers
pub type CounterNames = Arc<Mutex<HashMap<String, Counters>>>;

/// The download folder plugins see as `putio.download_dir`
pub type DownloadDir = Arc<Mutex<Option<PathBuf>>>;

/// What a worker builds its state from; replaced on reload and settings changes
#[derive(Debug, Clone)]
pub struct Spec {
    /// The checked manifest
    pub manifest: Manifest,
    /// The entry script (Lua)
    pub script: String,
    /// The module (WebAssembly, 029)
    pub wasm: Option<Arc<[u8]>>,
    /// The plugin's settings
    pub settings: PluginSettings,
    /// Hosts both needed and approved
    pub allowed: HashSet<String>,
    /// `r:`/`w:` folders both needed and approved (029)
    pub paths: Vec<String>,
}

/// What every worker shares
#[derive(Clone)]
pub struct Env {
    /// Where stores live; in memory when `None`
    pub data_dir: Option<PathBuf>,
    /// HTTP client, redirects off
    pub client: reqwest::Client,
    /// The runtime HTTP calls run on
    pub handle: tokio::runtime::Handle,
    /// `putio.time.now()`
    pub clock: Clock,
    /// Recent runs
    pub history: History,
    /// Open stores
    pub stores: Stores,
    /// Counter names used
    pub counters: CounterNames,
    /// `putio.download_dir`
    pub download_dir: DownloadDir,
    /// A workbench run's recorder (055); such a worker is not counted or kept in the runs
    pub tap: Option<Tap>,
}

impl Env {
    /// `name`'s store, opened on first use
    pub fn store_for(&self, name: &str) -> SharedStore {
        self.stores
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(Store::open(self.data_dir.as_deref(), name))))
            .clone()
    }

    /// `name`'s counter names
    pub fn counters_for(&self, name: &str) -> Counters {
        self.counters
            .lock()
            .unwrap()
            .entry(name.to_string())
            .or_default()
            .clone()
    }

    fn download_dir_text(&self) -> String {
        self.download_dir
            .lock()
            .unwrap()
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    }
}

/// What a [`Job::Call`] sends back
#[derive(Debug)]
pub struct CallReply {
    /// The run, logs included
    pub run: PluginRun,
    /// What the function returned, as JSON; `None` when it failed or is not defined
    pub value: Option<serde_json::Value>,
    /// Whether the module has the function
    pub defined: bool,
}

/// A queued call
pub enum Job {
    /// An event from the bus
    Event(Arc<Event>),
    /// The plugin's tick
    Tick,
    /// A test event; the run goes back on the channel
    Test(Arc<Event>, oneshot::Sender<PluginRun>),
    /// Call a module function by name (031: `on_action`, `health`); the run and what it
    /// returned go back on the channel. `kind` is the run's event type.
    Call {
        /// The function in the module
        function: String,
        /// `action` or `health`
        kind: String,
        /// Its arguments, as JSON
        args: Vec<serde_json::Value>,
        /// The time limit when not the plugin's `timeout_secs` (028: sync hooks)
        limit: Option<Duration>,
        /// Where the answer goes
        reply: oneshot::Sender<CallReply>,
    },
    /// Build the state and do nothing else, so a sync hook's first call does not wait
    /// for the plugin to load (a Go module takes seconds to compile)
    Load,
}

struct Queue {
    jobs: VecDeque<Job>,
    stop: bool,
    generation: u64,
    spec: Arc<Spec>,
}

/// A running plugin, shared by the service and its thread
pub struct Slot {
    /// The plugin's name
    pub name: String,
    queue: Mutex<Queue>,
    ready: Condvar,
    has_tick: AtomicBool,
    testing: AtomicBool,
    load_error: Mutex<Option<String>>,
    /// The generation whose state is built; `u64::MAX` before the first
    built: AtomicU64,
    tick_every: Mutex<Duration>,
    last_tick: Mutex<Instant>,
}

impl Slot {
    /// A slot for `spec`; [`Slot::spawn`] starts its thread
    pub fn new(spec: Spec) -> Arc<Slot> {
        let every = Duration::from_secs(spec.manifest.tick_every());
        Arc::new(Slot {
            name: spec.manifest.name.clone(),
            queue: Mutex::new(Queue {
                jobs: VecDeque::new(),
                stop: false,
                generation: 0,
                spec: Arc::new(spec),
            }),
            ready: Condvar::new(),
            has_tick: AtomicBool::new(false),
            testing: AtomicBool::new(false),
            load_error: Mutex::new(None),
            built: AtomicU64::new(u64::MAX),
            tick_every: Mutex::new(every),
            last_tick: Mutex::new(Instant::now()),
        })
    }

    /// Start the thread
    pub fn spawn(self: &Arc<Self>, env: Env) -> std::io::Result<()> {
        let slot = self.clone();
        std::thread::Builder::new()
            .name(format!("plugin-{}", self.name))
            .stack_size(STACK_BYTES)
            .spawn(move || Worker::new(slot, env).run())
            .map(|_| ())
    }

    /// The current spec
    pub fn spec(&self) -> Arc<Spec> {
        self.queue.lock().unwrap().spec.clone()
    }

    /// Swap the spec; the worker rebuilds before its next job
    pub fn update(&self, spec: Spec) {
        *self.tick_every.lock().unwrap() = Duration::from_secs(spec.manifest.tick_every());
        let mut q = self.queue.lock().unwrap();
        q.spec = Arc::new(spec);
        q.generation += 1;
        *self.load_error.lock().unwrap() = None;
    }

    /// Queue a job: tests go first, a second tick is not queued, and past
    /// [`MAX_QUEUE`] the oldest event or tick is dropped
    pub fn push(&self, job: Job) {
        let mut q = self.queue.lock().unwrap();
        if q.stop {
            return;
        }
        match job {
            Job::Test(..) => q.jobs.push_front(job),
            Job::Tick if q.jobs.iter().any(|j| matches!(j, Job::Tick)) => return,
            _ => {
                if q.jobs.len() >= MAX_QUEUE {
                    if let Some(i) = q.jobs.iter().position(|j| !matches!(j, Job::Test(..))) {
                        q.jobs.remove(i);
                        putio_sync_telemetry::plugins::record_dropped(&self.name, 1);
                    }
                }
                q.jobs.push_back(job);
            }
        }
        drop(q);
        self.ready.notify_one();
    }

    /// Jobs waiting
    pub fn queued(&self) -> usize {
        self.queue.lock().unwrap().jobs.len()
    }

    /// Whether the module has `on_tick` (known once the state is built)
    pub fn has_tick(&self) -> bool {
        self.has_tick.load(Ordering::Relaxed)
    }

    /// Whether the plugin's tick interval has passed since its last tick at `now`; when
    /// it has, that becomes the last tick. Half a second early counts, so a 5 s tick on
    /// a 5 s ticker is not skipped by jitter.
    pub fn tick_due(&self, now: Instant) -> bool {
        let every = *self.tick_every.lock().unwrap();
        let mut last = self.last_tick.lock().unwrap();
        let slack = Duration::from_millis(500);
        if now.saturating_duration_since(*last) + slack >= every {
            *last = now;
            true
        } else {
            false
        }
    }

    /// Whether the state for the current spec is built
    pub fn loaded(&self) -> bool {
        self.built.load(Ordering::Acquire) == self.queue.lock().unwrap().generation
    }

    /// Why the state could not be built, until the next update
    pub fn load_error(&self) -> Option<String> {
        self.load_error.lock().unwrap().clone()
    }

    /// Mark a test as running; false when one already is
    pub fn begin_test(&self) -> bool {
        !self.testing.swap(true, Ordering::AcqRel)
    }

    /// Stop the thread, dropping what is queued
    pub fn stop(&self) {
        let mut q = self.queue.lock().unwrap();
        q.stop = true;
        q.jobs.clear();
        drop(q);
        self.ready.notify_one();
    }

    /// Stop the thread once the queue is empty
    pub fn finish(&self) {
        self.queue.lock().unwrap().stop = true;
        self.ready.notify_one();
    }

    fn next(&self) -> Option<(Job, u64, Arc<Spec>)> {
        let mut q = self.queue.lock().unwrap();
        loop {
            if let Some(job) = q.jobs.pop_front() {
                return Some((job, q.generation, q.spec.clone()));
            }
            if q.stop {
                return None;
            }
            q = self.ready.wait(q).unwrap();
        }
    }
}

enum Runtime {
    Lua { lua: Lua, module: Table },
    Wasm(Box<wasm::Instance>),
}

struct Loaded {
    runtime: Runtime,
    logs: Logs,
    secrets: Rc<Vec<String>>,
}

impl Loaded {
    /// Memory the state uses now: Lua's allocations, or the module's linear memory
    fn memory_bytes(&self) -> u64 {
        match &self.runtime {
            Runtime::Lua { lua, .. } => lua.used_memory() as u64,
            Runtime::Wasm(instance) => instance.memory_bytes(),
        }
    }

    /// Whether the plugin defines the function `name`
    fn defines(&self, name: &str) -> bool {
        match &self.runtime {
            Runtime::Lua { module, .. } => {
                matches!(module.get::<LuaValue>(name), Ok(LuaValue::Function(_)))
            }
            Runtime::Wasm(instance) => instance.has(name),
        }
    }
}

struct Outcome {
    result: RunResult,
    error: Option<String>,
    rebuild: bool,
}

struct Worker {
    slot: Arc<Slot>,
    env: Env,
    deadline: Deadline,
    store: Option<SharedStore>,
    state: Option<(u64, Loaded)>,
    broken: Option<(u64, String)>,
}

impl Worker {
    fn new(slot: Arc<Slot>, env: Env) -> Worker {
        Worker {
            slot,
            env,
            deadline: Rc::new(Cell::new(None)),
            store: None,
            state: None,
            broken: None,
        }
    }

    fn run(mut self) {
        if self.env.tap.is_some() {
            host::LOG_LIMIT.with(|l| l.set(MAX_WORKBENCH_LOG_LINES));
        }
        while let Some((job, generation, spec)) = self.slot.next() {
            if self.state.as_ref().is_some_and(|(g, _)| *g != generation) {
                self.state = None;
            }
            if self.state.is_none() {
                if let Some((g, error)) = &self.broken {
                    if *g == generation {
                        let error = error.clone();
                        self.refuse(job, error);
                        continue;
                    }
                }
                if let Err(error) = self.build(generation, &spec) {
                    *self.slot.load_error.lock().unwrap() = Some(error.clone());
                    self.broken = Some((generation, error.clone()));
                    self.refuse(job, error);
                    continue;
                }
            }
            self.execute(job, &spec);
        }
    }

    /// A job for a state that cannot be built: a test gets the error, the rest is dropped
    fn refuse(&self, job: Job, error: String) {
        if let Job::Call { kind, reply, .. } = job {
            let run = PluginRun {
                event_type: kind,
                event_id: String::new(),
                started_at: now_text(),
                duration_ms: 0,
                result: RunResult::Failure,
                error: Some(error),
                logs: Vec::new(),
            };
            if self.env.tap.is_none() {
                putio_sync_telemetry::plugins::record_run(
                    &self.slot.name,
                    &run.event_type,
                    run.result.as_str(),
                    Duration::ZERO,
                );
            }
            let _ = reply.send(CallReply {
                run,
                value: None,
                defined: true,
            });
            return;
        }
        if let Job::Test(event, reply) = job {
            let run = PluginRun {
                event_type: event.event_type().as_str().to_string(),
                event_id: event.id.clone(),
                started_at: now_text(),
                duration_ms: 0,
                result: RunResult::Failure,
                error: Some(error),
                logs: Vec::new(),
            };
            self.record(&run, true);
            self.slot.testing.store(false, Ordering::Release);
            let _ = reply.send(run);
        }
    }

    fn build(&mut self, generation: u64, spec: &Spec) -> Result<(), String> {
        let m = &spec.manifest;
        let secrets: Rc<Vec<String>> = Rc::new(spec.settings.secrets.values().cloned().collect());
        let logs: Logs = Rc::default();
        if m.permissions.store && self.store.is_none() {
            let scratch = self.env.tap.as_ref().and_then(|t| t.store.clone());
            self.store = Some(scratch.unwrap_or_else(|| self.env.store_for(&m.name)));
        }
        let host = Host {
            name: m.name.clone(),
            version: m.version.clone(),
            config: m.config_values(&spec.settings),
            secrets: secrets.clone(),
            http: m.permissions.http.any().then(|| Http {
                allowed: spec.allowed.clone(),
                client: self.env.client.clone(),
                handle: self.env.handle.clone(),
                tap: self.env.tap.clone(),
            }),
            store: if m.permissions.store {
                self.store.clone()
            } else {
                None
            },
            files: (!m.permissions.files.is_empty()).then(|| {
                let mut files = Files::new(&spec.paths, &spec.paths, &self.env.download_dir_text());
                files.tap = self.env.tap.clone();
                Rc::new(files)
            }),
            download_dir: self.env.download_dir_text(),
            counters: self.env.counters_for(&m.name),
            deadline: self.deadline.clone(),
            logs: logs.clone(),
            clock: self.env.clock.clone(),
        };
        let timeout = spec.settings.timeout();
        let started = Instant::now();
        self.deadline.set(Some(started + timeout));
        let runtime = self.load(spec, host, &secrets, started, timeout);
        self.deadline.set(None);
        let mut loaded = Loaded {
            runtime: runtime?,
            logs,
            secrets,
        };
        let (what, table) = match loaded.runtime {
            Runtime::Lua { .. } => ("the returned table", "function"),
            Runtime::Wasm(_) => ("the module", "export"),
        };
        if !m.events.is_empty() && !loaded.defines("on_event") {
            return Err(format!("{}: {} has no on_event {}", m.entry, what, table));
        }
        // A plugin that only answers sync hooks needs no on_event (028)
        for hook in &m.hooks {
            if !loaded.defines(hook) {
                return Err(format!("{}: {} has no {} {}", m.entry, what, hook, table));
            }
        }
        self.slot
            .has_tick
            .store(loaded.defines("on_tick"), Ordering::Relaxed);
        if loaded.defines("init") {
            let started_at = now_text();
            let (out, _) = call(
                &self.deadline,
                &mut loaded,
                "init",
                Vec::new(),
                timeout,
                false,
            );
            let run = PluginRun {
                event_type: "init".to_string(),
                event_id: String::new(),
                started_at,
                duration_ms: millis(started.elapsed()),
                result: out.result,
                error: out.error.clone(),
                logs: Vec::new(),
            };
            self.finished(&run, started.elapsed(), false);
            if out.result != RunResult::Success {
                return Err(format!(
                    "init: {}",
                    out.error.unwrap_or_else(|| out.result.as_str().to_string())
                ));
            }
        }
        self.state = Some((generation, loaded));
        self.slot.built.store(generation, Ordering::Release);
        Ok(())
    }

    /// The plugin's Lua state with its script run, or its WebAssembly instance
    fn load(
        &self,
        spec: &Spec,
        host: Host,
        secrets: &[String],
        started: Instant,
        timeout: Duration,
    ) -> Result<Runtime, String> {
        let m = &spec.manifest;
        if let Some(bytes) = &spec.wasm {
            let memory_mb = spec.settings.memory_mb;
            return match wasm::Instance::new(bytes, host, memory_mb) {
                Ok(instance) => Ok(Runtime::Wasm(Box::new(instance))),
                Err(failure) => {
                    let text = match failure {
                        Failure::Timeout => limit_text(timeout),
                        Failure::Memory => format!("{} ({} MiB)", wasm::MEMORY_LIMIT, memory_mb),
                        Failure::Trap(t) | Failure::Error(t) => t,
                    };
                    Err(host::scrub(&format!("{}: {}", m.entry, text), secrets))
                }
            };
        }
        let lua = sandbox::new_state(spec.settings.memory_mb, self.deadline.clone())
            .map_err(|e| sandbox::error_text(&e))?;
        host::install(&lua, host).map_err(|e| sandbox::error_text(&e))?;
        let loaded = lua
            .load(spec.script.as_str())
            .set_name(format!("@{}", m.entry))
            .eval::<LuaValue>();
        match loaded {
            Ok(LuaValue::Table(module)) => Ok(Runtime::Lua { lua, module }),
            Ok(_) => Err(format!("{}: must return a table", m.entry)),
            Err(e) => Err(host::scrub(&describe(&e, started, timeout), secrets)),
        }
    }

    fn execute(&mut self, job: Job, spec: &Spec) {
        let Some((generation, mut loaded)) = self.state.take() else {
            return;
        };
        let rebuild = self.execute_on(&mut loaded, job, spec);
        if !rebuild {
            self.state = Some((generation, loaded));
        } else if !spec.manifest.hooks.is_empty() {
            // Load again before the next sync hook is asked
            self.slot.built.store(u64::MAX, Ordering::Release);
            self.slot.push(Job::Load);
        }
    }

    /// Run `job` on `loaded`; whether the state must be built again
    fn execute_on(&mut self, loaded: &mut Loaded, job: Job, spec: &Spec) -> bool {
        let timeout = spec.settings.timeout();
        let (event_type, event_id, function, arg, reply) = match job {
            Job::Call {
                function,
                kind,
                args,
                limit,
                reply,
            } => {
                return self.execute_call(
                    loaded,
                    function,
                    kind,
                    args,
                    reply,
                    limit.unwrap_or(timeout),
                );
            }
            Job::Load => return false,
            Job::Tick => {
                if !loaded.defines("on_tick") {
                    return false;
                }
                ("tick".to_string(), String::new(), "on_tick", None, None)
            }
            Job::Event(event) => match serde_json::to_value(&*event) {
                Ok(arg) => (
                    event.event_type().as_str().to_string(),
                    event.id.clone(),
                    "on_event",
                    Some(arg),
                    None,
                ),
                Err(e) => {
                    tracing::warn!(plugin = %self.slot.name, error = %e, "Plugin event not delivered");
                    return false;
                }
            },
            Job::Test(event, reply) => match serde_json::to_value(&*event) {
                Ok(arg) => (
                    event.event_type().as_str().to_string(),
                    event.id.clone(),
                    "on_event",
                    Some(arg),
                    Some(reply),
                ),
                Err(e) => {
                    self.refuse(Job::Test(event, reply), e.to_string());
                    return false;
                }
            },
        };
        let started_at = now_text();
        let started = Instant::now();
        let args = arg.into_iter().collect();
        let (out, _) = call(&self.deadline, loaded, function, args, timeout, false);
        let took = started.elapsed();
        let test = reply.is_some();
        let run = PluginRun {
            event_type,
            event_id,
            started_at,
            duration_ms: millis(took),
            result: out.result,
            error: out.error,
            logs: if test {
                loaded.logs.borrow().clone()
            } else {
                Vec::new()
            },
        };
        let quiet = run.event_type == "tick" && run.result == RunResult::Success;
        self.finished(&run, took, quiet);
        if let Some(reply) = reply {
            self.slot.testing.store(false, Ordering::Release);
            let _ = reply.send(run);
        }
        out.rebuild
    }

    /// Run a [`Job::Call`]: counted and logged like any run, kept out of the history.
    /// Whether the state must be built again.
    fn execute_call(
        &mut self,
        loaded: &mut Loaded,
        function: String,
        kind: String,
        args: Vec<Value>,
        reply: oneshot::Sender<CallReply>,
        timeout: Duration,
    ) -> bool {
        let started_at = now_text();
        if !loaded.defines(&function) {
            let _ = reply.send(CallReply {
                run: PluginRun {
                    event_type: kind,
                    event_id: String::new(),
                    started_at,
                    duration_ms: 0,
                    result: RunResult::Success,
                    error: None,
                    logs: Vec::new(),
                },
                value: None,
                defined: false,
            });
            return false;
        }
        let started = Instant::now();
        let (out, value) = call(&self.deadline, loaded, &function, args, timeout, true);
        let took = started.elapsed();
        if let Some(tap) = &self.env.tap {
            tap.set_memory(loaded.memory_bytes());
        }
        let run = PluginRun {
            event_type: kind,
            event_id: String::new(),
            started_at,
            duration_ms: millis(took),
            result: out.result,
            error: out.error,
            logs: loaded.logs.borrow().clone(),
        };
        // A failed sync hook is kept with the runs, so Settings shows why it fell back
        let quiet = run.result == RunResult::Success || kind_is_settings(&run.event_type);
        self.finished(&run, took, quiet);
        let _ = reply.send(CallReply {
            run,
            value,
            defined: true,
        });
        out.rebuild
    }

    /// Log, count and keep a run; `quiet` runs are counted only
    fn finished(&self, run: &PluginRun, took: Duration, quiet: bool) {
        if self.env.tap.is_some() {
            return;
        }
        putio_sync_telemetry::plugins::record_run(
            &self.slot.name,
            &run.event_type,
            run.result.as_str(),
            took,
        );
        match &run.error {
            None => tracing::debug!(
                plugin = %self.slot.name,
                event = %run.event_type,
                duration_ms = run.duration_ms,
                "Plugin ran"
            ),
            Some(error) => tracing::warn!(
                plugin = %self.slot.name,
                event = %run.event_type,
                result = run.result.as_str(),
                error = %error,
                "Plugin run failed"
            ),
        }
        if !quiet {
            self.record(run, false);
        }
    }

    fn record(&self, run: &PluginRun, count: bool) {
        if self.env.tap.is_some() {
            return;
        }
        if count {
            putio_sync_telemetry::plugins::record_run(
                &self.slot.name,
                &run.event_type,
                run.result.as_str(),
                Duration::ZERO,
            );
        }
        let mut history = self.env.history.lock().unwrap();
        let runs = history.entry(self.slot.name.clone()).or_default();
        if runs.len() >= MAX_RUNS {
            runs.pop_front();
        }
        runs.push_back(run.clone());
    }
}

/// Whether a call's kind is a settings action or health check, not a sync hook
fn kind_is_settings(kind: &str) -> bool {
    kind.parse::<Hook>().is_err()
}

/// Call `function` with `args` under the time limit: the outcome and, when `want` is set,
/// what it returned as JSON (`null` for nothing)
fn call(
    deadline: &Deadline,
    loaded: &mut Loaded,
    function: &str,
    args: Vec<Value>,
    timeout: Duration,
    want: bool,
) -> (Outcome, Option<Value>) {
    loaded.logs.borrow_mut().clear();
    let started = Instant::now();
    deadline.set(Some(started + timeout));
    let result = match &mut loaded.runtime {
        Runtime::Lua { lua, module } => {
            call_lua(lua, module, function, args, started, timeout, want)
        }
        Runtime::Wasm(instance) => match instance.call(function, &args) {
            Ok(value) => Ok(want.then(|| value.unwrap_or(Value::Null))),
            Err(failure) => Err((
                failure == Failure::Timeout,
                failure.rebuild(),
                instance.describe(&failure, timeout),
            )),
        },
    };
    deadline.set(None);
    let late = started.elapsed() >= timeout;
    match result {
        Ok(_) if late => (
            Outcome {
                result: RunResult::Timeout,
                error: Some(limit_text(timeout)),
                rebuild: true,
            },
            None,
        ),
        Ok(value) => (
            Outcome {
                result: RunResult::Success,
                error: None,
                rebuild: false,
            },
            value,
        ),
        Err((timed_out, rebuild, error)) => {
            let timed_out = timed_out || late;
            (
                Outcome {
                    result: if timed_out {
                        RunResult::Timeout
                    } else {
                        RunResult::Failure
                    },
                    error: Some(host::scrub(
                        &if timed_out {
                            limit_text(timeout)
                        } else {
                            error
                        },
                        &loaded.secrets,
                    )),
                    rebuild: timed_out || rebuild,
                },
                None,
            )
        }
    }
}

/// A Lua call: what it returned, or (timed out, rebuild, error)
fn call_lua(
    lua: &Lua,
    module: &Table,
    function: &str,
    args: Vec<Value>,
    started: Instant,
    timeout: Duration,
    want: bool,
) -> Result<Option<Value>, (bool, bool, String)> {
    let f: Function = module
        .get(function)
        .map_err(|e| (false, false, sandbox::error_text(&e)))?;
    let mut lua_args = Vec::with_capacity(args.len());
    for a in &args {
        let v = host::to_lua(lua, a).map_err(|e| (false, false, sandbox::error_text(&e)))?;
        lua_args.push(v);
    }
    match f.call::<LuaValue>(mlua::MultiValue::from_vec(lua_args)) {
        Ok(_) if !want => Ok(None),
        Ok(LuaValue::Nil) => Ok(Some(Value::Null)),
        Ok(v) => host::from_lua(lua, v).map(Some).map_err(|e| {
            let text = format!("{} returned a value that is not JSON: {}", function, e);
            (false, false, text)
        }),
        Err(e) => {
            let timed_out = sandbox::error_text(&e).contains(TIME_LIMIT);
            let memory = sandbox::is_memory_error(&e);
            Err((timed_out, memory, describe(&e, started, timeout)))
        }
    }
}

fn describe(e: &mlua::Error, started: Instant, timeout: Duration) -> String {
    let text = sandbox::error_text(e);
    if text.contains(TIME_LIMIT) || started.elapsed() >= timeout {
        limit_text(timeout)
    } else {
        text
    }
}

fn limit_text(timeout: Duration) -> String {
    format!("{} ({} s)", TIME_LIMIT, timeout.as_secs())
}

pub(crate) fn now_text() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn millis(d: Duration) -> u64 {
    d.as_millis().min(u64::MAX as u128) as u64
}
