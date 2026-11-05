//! One plugin's worker (research R3): a thread that owns the plugin's Lua state and runs
//! the jobs queued for it, one at a time, each under the plugin's time limit.
//!
//! A timeout or memory error throws the state away; the next job builds a new one (load
//! the script, call `init`). A state that cannot be built stays broken until the next
//! reload or settings change.

use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use mlua::{Function, Lua, Table, Value as LuaValue};
use putio_sync_core::events::Event;
use putio_sync_core::plugins::PluginSettings;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::host::{self, Clock, Counters, Host, Http, LogLine, Logs};
use crate::manifest::Manifest;
use crate::sandbox::{self, Deadline, TIME_LIMIT};
use crate::store::{SharedStore, Store};

/// Jobs kept per plugin; the oldest event is dropped past this
pub const MAX_QUEUE: usize = 1000;
/// Runs kept per plugin
pub const MAX_RUNS: usize = 20;

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
    /// The entry script
    pub script: String,
    /// The plugin's settings
    pub settings: PluginSettings,
    /// Hosts both needed and approved
    pub allowed: HashSet<String>,
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
        /// Where the answer goes
        reply: oneshot::Sender<CallReply>,
    },
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
            tick_every: Mutex::new(every),
            last_tick: Mutex::new(Instant::now()),
        })
    }

    /// Start the thread
    pub fn spawn(self: &Arc<Self>, env: Env) -> std::io::Result<()> {
        let slot = self.clone();
        std::thread::Builder::new()
            .name(format!("plugin-{}", self.name))
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

struct Loaded {
    lua: Lua,
    module: Table,
    logs: Logs,
    secrets: Rc<Vec<String>>,
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
            putio_sync_telemetry::plugins::record_run(
                &self.slot.name,
                &run.event_type,
                run.result.as_str(),
                Duration::ZERO,
            );
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
        let lua = sandbox::new_state(spec.settings.memory_mb, self.deadline.clone())
            .map_err(|e| sandbox::error_text(&e))?;
        let secrets: Rc<Vec<String>> = Rc::new(spec.settings.secrets.values().cloned().collect());
        let logs: Logs = Rc::default();
        if m.permissions.store && self.store.is_none() {
            self.store = Some(self.env.store_for(&m.name));
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
            }),
            store: if m.permissions.store {
                self.store.clone()
            } else {
                None
            },
            download_dir: self.env.download_dir_text(),
            counters: self.env.counters_for(&m.name),
            deadline: self.deadline.clone(),
            logs: logs.clone(),
            clock: self.env.clock.clone(),
        };
        host::install(&lua, host).map_err(|e| sandbox::error_text(&e))?;

        let timeout = spec.settings.timeout();
        let started = Instant::now();
        self.deadline.set(Some(started + timeout));
        let loaded = lua
            .load(spec.script.as_str())
            .set_name(format!("@{}", m.entry))
            .eval::<LuaValue>();
        self.deadline.set(None);
        let module = match loaded {
            Ok(LuaValue::Table(t)) => t,
            Ok(_) => return Err(format!("{}: must return a table", m.entry)),
            Err(e) => return Err(host::scrub(&describe(&e, started, timeout), &secrets)),
        };
        if !matches!(
            module.get::<LuaValue>("on_event"),
            Ok(LuaValue::Function(_))
        ) {
            return Err(format!(
                "{}: the returned table has no on_event function",
                m.entry
            ));
        }
        let has_tick = matches!(module.get::<LuaValue>("on_tick"), Ok(LuaValue::Function(_)));
        self.slot.has_tick.store(has_tick, Ordering::Relaxed);
        let loaded = Loaded {
            lua,
            module,
            logs,
            secrets,
        };
        if let Ok(LuaValue::Function(init)) = loaded.module.get::<LuaValue>("init") {
            let started_at = now_text();
            let (out, _) = self.call(&loaded, &init, Vec::new(), timeout);
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
        Ok(())
    }

    /// Call `f` under the time limit; the outcome and the first value it returned
    fn call(
        &self,
        loaded: &Loaded,
        f: &Function,
        args: Vec<LuaValue>,
        timeout: Duration,
    ) -> (Outcome, LuaValue) {
        loaded.logs.borrow_mut().clear();
        let started = Instant::now();
        self.deadline.set(Some(started + timeout));
        let result = f.call::<LuaValue>(mlua::MultiValue::from_vec(args));
        self.deadline.set(None);
        let late = started.elapsed() >= timeout;
        let outcome = match result {
            Ok(_) if late => Outcome {
                result: RunResult::Timeout,
                error: Some(limit_text(timeout)),
                rebuild: true,
            },
            Ok(value) => {
                return (
                    Outcome {
                        result: RunResult::Success,
                        error: None,
                        rebuild: false,
                    },
                    value,
                )
            }
            Err(e) => {
                let timed_out = late || sandbox::error_text(&e).contains(TIME_LIMIT);
                let memory = sandbox::is_memory_error(&e);
                Outcome {
                    result: if timed_out {
                        RunResult::Timeout
                    } else {
                        RunResult::Failure
                    },
                    error: Some(host::scrub(
                        &describe(&e, started, timeout),
                        &loaded.secrets,
                    )),
                    rebuild: timed_out || memory,
                }
            }
        };
        (outcome, LuaValue::Nil)
    }

    fn execute(&mut self, job: Job, spec: &Spec) {
        let Some((_, loaded)) = &self.state else {
            return;
        };
        let timeout = spec.settings.timeout();
        let (event_type, event_id, function, arg, reply) = match job {
            Job::Call {
                function,
                kind,
                args,
                reply,
            } => {
                self.execute_call(function, kind, args, reply, timeout);
                return;
            }
            Job::Tick => {
                let Ok(LuaValue::Function(f)) = loaded.module.get::<LuaValue>("on_tick") else {
                    return;
                };
                ("tick".to_string(), String::new(), f, LuaValue::Nil, None)
            }
            Job::Event(event) => {
                let (f, arg) = match prepare(loaded, &event) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(plugin = %self.slot.name, error = %e, "Plugin event not delivered");
                        return;
                    }
                };
                (
                    event.event_type().as_str().to_string(),
                    event.id.clone(),
                    f,
                    arg,
                    None,
                )
            }
            Job::Test(event, reply) => match prepare(loaded, &event) {
                Ok((f, arg)) => (
                    event.event_type().as_str().to_string(),
                    event.id.clone(),
                    f,
                    arg,
                    Some(reply),
                ),
                Err(e) => {
                    self.refuse(Job::Test(event, reply), e);
                    return;
                }
            },
        };
        let started_at = now_text();
        let started = Instant::now();
        let args = if arg.is_nil() { Vec::new() } else { vec![arg] };
        let (out, _) = self.call(loaded, &function, args, timeout);
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
        if out.rebuild {
            self.state = None;
        }
        if let Some(reply) = reply {
            self.slot.testing.store(false, Ordering::Release);
            let _ = reply.send(run);
        }
    }

    /// Run a [`Job::Call`]: counted and logged like any run, kept out of the history
    fn execute_call(
        &mut self,
        function: String,
        kind: String,
        args: Vec<serde_json::Value>,
        reply: oneshot::Sender<CallReply>,
        timeout: Duration,
    ) {
        let Some((_, loaded)) = &self.state else {
            return;
        };
        let started_at = now_text();
        let Ok(LuaValue::Function(f)) = loaded.module.get::<LuaValue>(function.as_str()) else {
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
            return;
        };
        let mut lua_args = Vec::with_capacity(args.len());
        for a in &args {
            match host::to_lua(&loaded.lua, a) {
                Ok(v) => lua_args.push(v),
                Err(e) => {
                    let error = sandbox::error_text(&e);
                    self.refuse(
                        Job::Call {
                            function,
                            kind,
                            args,
                            reply,
                        },
                        error,
                    );
                    return;
                }
            }
        }
        let started = Instant::now();
        let (mut out, value) = self.call(loaded, &f, lua_args, timeout);
        let took = started.elapsed();
        let value = match (out.result, value) {
            (RunResult::Success, LuaValue::Nil) => Some(serde_json::Value::Null),
            (RunResult::Success, v) => match host::from_lua(&loaded.lua, v) {
                Ok(v) => Some(v),
                Err(e) => {
                    out.result = RunResult::Failure;
                    out.error = Some(format!(
                        "{} returned a value that is not JSON: {}",
                        function,
                        host::scrub(&e, &loaded.secrets)
                    ));
                    None
                }
            },
            _ => None,
        };
        let run = PluginRun {
            event_type: kind,
            event_id: String::new(),
            started_at,
            duration_ms: millis(took),
            result: out.result,
            error: out.error,
            logs: loaded.logs.borrow().clone(),
        };
        self.finished(&run, took, true);
        if out.rebuild {
            self.state = None;
        }
        let _ = reply.send(CallReply {
            run,
            value,
            defined: true,
        });
    }

    /// Log, count and keep a run; `quiet` runs are counted only
    fn finished(&self, run: &PluginRun, took: Duration, quiet: bool) {
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

fn prepare(loaded: &Loaded, event: &Event) -> Result<(Function, LuaValue), String> {
    let f: Function = loaded
        .module
        .get("on_event")
        .map_err(|e| sandbox::error_text(&e))?;
    let value = serde_json::to_value(event).map_err(|e| e.to_string())?;
    let arg = host::to_lua(&loaded.lua, &value).map_err(|e| sandbox::error_text(&e))?;
    Ok((f, arg))
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

fn now_text() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn millis(d: Duration) -> u64 {
    d.as_millis().min(u64::MAX as u128) as u64
}
