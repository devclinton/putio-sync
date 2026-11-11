//! The plugin workbench's Run panel (feature 055): call one plugin function on a worker of
//! its own and show everything it did. A dry run records HTTP requests and file writes
//! instead of doing them and works on a scratch copy of the store; a real run sends to the
//! approved hosts and uses the real store, like a live event. Lua and WebAssembly plugins
//! run the same way, through [`Job::Call`].
//!
//! Workbench runs are not counted in the metrics or kept in the plugin's run list; the
//! last [`MAX_RUNS_KEPT`] are kept here, with what was called, to be run again.

use std::collections::{BTreeMap, VecDeque};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use putio_sync_core::events::{samples, Event, EventType};
use putio_sync_core::plugins::MASK;
use putio_sync_core::sync_hooks::{self, Hook};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::sync::oneshot;

use crate::host::{self, RequestOptions, Response, Summary, MAX_BODY_BYTES};
use crate::source::{self, Problem};
use crate::store::{SharedStore, Store};
use crate::worker::{now_text, Job, PluginRun, RunResult, Slot};
use crate::PluginsService;

/// Log lines kept per workbench run
pub const MAX_WORKBENCH_LOG_LINES: usize = 200;
/// Workbench runs kept to be run again
pub const MAX_RUNS_KEPT: usize = 20;
/// Requests recorded per run; later ones still run, unrecorded
pub const MAX_RECORDED: usize = 100;
/// Largest event data, action input or sample file taken, as JSON
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;

/// The answer a dry run's requests get
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    /// HTTP status
    #[serde(default = "ok")]
    pub status: u16,
    /// Body text
    #[serde(default)]
    pub body: String,
}

fn ok() -> u16 {
    200
}

impl Default for Reply {
    fn default() -> Reply {
        Reply {
            status: ok(),
            body: String::new(),
        }
    }
}

/// One request a run made or would have made, secrets masked
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRecord {
    /// Upper case
    pub method: String,
    /// The full URL
    pub url: String,
    /// Request headers; credentials masked whole
    pub headers: BTreeMap<String, String>,
    /// Request body text, cut at 1 MiB
    pub body: String,
    /// Whether it went out (a real run)
    pub sent: bool,
    /// The answer's status: the reply's in a dry run, the server's in a real one
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// How long a sent request took
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Why it failed or was refused
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A file a dry run would have written
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileWrite {
    /// As the plugin gave it
    pub path: String,
    /// Text length
    pub bytes: usize,
}

#[derive(Debug, Default)]
struct Recorded {
    requests: Vec<HttpRecord>,
    writes: Vec<FileWrite>,
    memory_bytes: Option<u64>,
}

/// What a workbench worker records into; shared by the worker thread and the caller
#[derive(Debug, Clone)]
pub struct Tap {
    /// Requests and writes recorded, not done
    pub dry: bool,
    reply: Reply,
    secrets: Arc<Vec<String>>,
    recorded: Arc<Mutex<Recorded>>,
    /// The store the run uses instead of the plugin's own (a dry run's scratch copy)
    pub(crate) store: Option<SharedStore>,
}

impl Tap {
    /// A recorder for one run; `secrets` are masked in what it keeps
    pub fn new(dry: bool, reply: Reply, secrets: Vec<String>) -> Tap {
        Tap {
            dry,
            reply,
            secrets: Arc::new(secrets),
            recorded: Arc::default(),
            store: None,
        }
    }

    /// The dry run's answer to a request
    pub(crate) fn reply(&self) -> Response {
        Response {
            status: self.reply.status,
            headers: BTreeMap::new(),
            body: self.reply.body.clone().into_bytes(),
            truncated: false,
        }
    }

    pub(crate) fn record_request(
        &self,
        opts: &RequestOptions,
        sent: bool,
        status: Result<u16, &str>,
        took: Duration,
    ) {
        self.record_summary(Summary::of(opts), sent, status, took);
    }

    pub(crate) fn record_summary(
        &self,
        summary: Summary,
        sent: bool,
        status: Result<u16, &str>,
        took: Duration,
    ) {
        let scrub = |text: &str| host::scrub(text, &self.secrets);
        let headers = summary
            .headers
            .iter()
            .map(|(k, v)| {
                let v = if is_credential(k) {
                    MASK.to_string()
                } else {
                    scrub(v)
                };
                (k.clone(), v)
            })
            .collect();
        let record = HttpRecord {
            method: summary.method,
            url: scrub(&summary.url),
            headers,
            body: cut(&scrub(&summary.body), MAX_BODY_BYTES),
            sent,
            status: status.ok(),
            duration_ms: sent.then(|| took.as_millis().min(u64::MAX as u128) as u64),
            error: status.err().map(scrub),
        };
        let mut recorded = self.recorded.lock().unwrap();
        if recorded.requests.len() < MAX_RECORDED {
            recorded.requests.push(record);
        }
    }

    pub(crate) fn record_write(&self, path: &str, bytes: usize) {
        let mut recorded = self.recorded.lock().unwrap();
        if recorded.writes.len() < MAX_RECORDED {
            recorded.writes.push(FileWrite {
                path: host::scrub(path, &self.secrets),
                bytes,
            });
        }
    }

    pub(crate) fn set_memory(&self, bytes: u64) {
        self.recorded.lock().unwrap().memory_bytes = Some(bytes);
    }
}

/// Headers whose whole value is a credential
fn is_credential(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
    ) || ["token", "key", "secret", "password"]
        .iter()
        .any(|w| name.contains(w))
}

/// `text` cut to at most `max` bytes on a character boundary, with a note
fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… (cut at {} KiB)", &text[..end], max / 1024)
}

/// What to call
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Call {
    /// `on_event` with an event of this type; `data` replaces the sample's data
    Event {
        /// An event type
        event_type: String,
        /// The event's `data`; the sample's when null
        #[serde(default)]
        data: Value,
    },
    /// `on_tick`
    Tick,
    /// `health`
    Health,
    /// `on_action(action, input)`
    Action {
        /// The action id
        action: String,
        /// Its input
        #[serde(default)]
        input: Value,
    },
    /// A sync hook with a file
    Hook {
        /// `filter`, `rename` or `folder`
        hook: String,
        /// A sync hook file: id, name, size, extension, folder, decision, reason
        file: Value,
    },
}

/// One Run panel run
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRequest {
    /// The plugin
    pub name: String,
    /// What to call
    pub call: Call,
    /// Send requests and use the real store
    #[serde(default)]
    pub real: bool,
    /// The dry run's answer to requests
    #[serde(default)]
    pub reply: Reply,
    /// Unsaved `plugin.toml` text (Lua plugins); sent with `script`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<String>,
    /// Unsaved entry script text
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
}

/// What a run did
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// Increasing, for running it again
    pub id: u64,
    /// What was asked
    pub request: RunRequest,
    /// It ran editor text, not the saved files
    pub unsaved: bool,
    /// Result, error, duration and log lines
    pub run: PluginRun,
    /// What the function returned; `null` for nothing
    pub value: Option<Value>,
    /// For a sync hook: what the sync makes of `value`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// Whether the plugin has the function
    pub defined: bool,
    /// Memory in use at the end
    pub memory_bytes: Option<u64>,
    /// Requests, in order
    pub requests: Vec<HttpRecord>,
    /// Files a dry run would have written
    pub writes: Vec<FileWrite>,
    /// The store before the run; `None` without the store permission
    pub store_before: Option<Value>,
    /// The store after the run (the scratch copy in a dry run)
    pub store_after: Option<Value>,
    /// What kept the plugin from loading
    pub problems: Vec<Problem>,
}

/// Why a run was not made
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkbenchError {
    /// No such plugin, or no such kept run
    NotFound,
    /// A run, action or health check of the plugin is running
    Busy,
    /// The request does not fit the plugin
    Invalid(String),
    /// The worker did not start or answer
    Unavailable,
}

impl std::fmt::Display for WorkbenchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkbenchError::NotFound => write!(f, "not found"),
            WorkbenchError::Busy => write!(f, "a call to this plugin is still running"),
            WorkbenchError::Invalid(e) => write!(f, "{}", e),
            WorkbenchError::Unavailable => write!(f, "the plugin worker did not answer"),
        }
    }
}

/// The last runs, newest last, and the next id
pub type Runs = Mutex<(u64, VecDeque<Run>)>;

/// Each event type with its sample `data`
pub fn event_samples() -> Vec<(String, Value)> {
    samples()
        .into_iter()
        .map(|payload| {
            let event = Event::new(payload);
            let data = serde_json::to_value(&event)
                .ok()
                .and_then(|v| v.get("data").cloned())
                .unwrap_or(Value::Null);
            (event.event_type().as_str().to_string(), data)
        })
        .collect()
}

/// A sample file for sync hooks
pub fn sample_file() -> Value {
    json!({
        "id": 1,
        "name": "Some.Show.S01E02.1080p.WEB.x264.mkv",
        "size": 1_500_000_000u64,
        "extension": "mkv",
        "folder": "TV",
        "decision": "download",
        "reason": "",
    })
}

/// The envelope `on_event` gets: a sample of `event_type` with `data` put in
fn envelope(event_type: &str, data: &Value) -> Result<Value, WorkbenchError> {
    let wanted = EventType::from_str(event_type)
        .map_err(|_| WorkbenchError::Invalid(format!("unknown event type {:?}", event_type)))?;
    let payload = samples()
        .into_iter()
        .find(|p| Event::new(p.clone()).event_type() == wanted)
        .ok_or_else(|| WorkbenchError::Invalid(format!("no sample for {}", event_type)))?;
    let mut value = serde_json::to_value(Event::new(payload))
        .map_err(|e| WorkbenchError::Invalid(e.to_string()))?;
    if !data.is_null() {
        value["data"] = data.clone();
    }
    Ok(value)
}

fn too_big(what: &str, value: &Value) -> Result<(), WorkbenchError> {
    let len = serde_json::to_vec(value).map(|v| v.len()).unwrap_or(0);
    if len > MAX_INPUT_BYTES {
        return Err(WorkbenchError::Invalid(format!(
            "{} larger than {} KiB",
            what,
            MAX_INPUT_BYTES / 1024
        )));
    }
    Ok(())
}

/// The function, the run's kind and the arguments for `call`
fn target(call: &Call) -> Result<(String, String, Vec<Value>), WorkbenchError> {
    Ok(match call {
        Call::Event { event_type, data } => {
            too_big("event data", data)?;
            let event = envelope(event_type, data)?;
            ("on_event".into(), event_type.clone(), vec![event])
        }
        Call::Tick => ("on_tick".into(), "tick".into(), Vec::new()),
        Call::Health => ("health".into(), "health".into(), Vec::new()),
        Call::Action { action, input } => {
            too_big("action input", input)?;
            let input = if input.is_null() {
                Value::Object(Map::new())
            } else {
                input.clone()
            };
            let args = vec![Value::from(action.as_str()), input];
            ("on_action".into(), "action".into(), args)
        }
        Call::Hook { hook, file } => {
            too_big("file", file)?;
            let hook = Hook::from_str(hook).map_err(WorkbenchError::Invalid)?;
            let name = hook.as_str().to_string();
            (name.clone(), name, vec![file.clone()])
        }
    })
}

/// What the sync makes of a hook's answer
fn hook_answer(hook: &str, value: &Value) -> Option<String> {
    let parsed = match Hook::from_str(hook).ok()? {
        Hook::Filter => {
            sync_hooks::parse_filter(value).map(|a| a.map(|a| format!("{:?}", a).to_lowercase()))
        }
        Hook::Rename => sync_hooks::parse_name(value),
        Hook::Folder => sync_hooks::parse_folder(value),
    };
    Some(match parsed {
        Ok(Some(answer)) => answer,
        Ok(None) => "no answer: the built-in rule applies".to_string(),
        Err(e) => format!("ignored: {}", e),
    })
}

/// A store's contents, secrets masked
fn snapshot(store: &SharedStore, secrets: &[String]) -> Value {
    let store = store.lock().unwrap();
    let mut out = Map::new();
    for key in store.keys() {
        if let Some(v) = store.get(&key) {
            out.insert(host::scrub(&key, secrets), masked(v, secrets));
        }
    }
    Value::Object(out)
}

fn masked(value: &Value, secrets: &[String]) -> Value {
    match value {
        Value::String(s) => Value::String(host::scrub(s, secrets)),
        Value::Array(a) => Value::Array(a.iter().map(|v| masked(v, secrets)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, v)| (host::scrub(k, secrets), masked(v, secrets)))
                .collect(),
        ),
        v => v.clone(),
    }
}

fn failed(error: String, kind: &str) -> PluginRun {
    PluginRun {
        event_type: kind.to_string(),
        event_id: String::new(),
        started_at: now_text(),
        duration_ms: 0,
        result: RunResult::Failure,
        error: Some(error),
        logs: Vec::new(),
    }
}

impl PluginsService {
    /// Run `request` on a worker of its own and keep it with the last runs
    pub async fn workbench_run(&self, request: RunRequest) -> Result<Run, WorkbenchError> {
        let (mut found, settings) = self
            .found_with_settings(&request.name)
            .ok_or(WorkbenchError::NotFound)?;
        let (function, kind, args) = target(&request.call)?;
        let unsaved = request.manifest.is_some() || request.script.is_some();
        let mut problems = Vec::new();
        if unsaved {
            let (Some(manifest), Some(script)) = (&request.manifest, &request.script) else {
                return Err(WorkbenchError::Invalid(
                    "send manifest and script together".to_string(),
                ));
            };
            if found.wasm.is_some() {
                return Err(WorkbenchError::Invalid(
                    "a WebAssembly plugin runs as installed".to_string(),
                ));
            }
            let (manifest, found_problems) = source::check(manifest, script, &found.source);
            problems = found_problems;
            found.manifest = manifest;
            found.script = script.clone();
            found.error = None;
            if let Some(m) = &found.manifest {
                if m.name != request.name {
                    problems.push(Problem::manifest(format!(
                        "name: must stay \"{}\" while running unsaved text",
                        request.name
                    )));
                    found.manifest = None;
                }
            }
        } else if let Some(error) = &found.error {
            problems.push(Problem::manifest(error.clone()));
        }
        let secrets: Vec<String> = settings.secrets.values().cloned().collect();
        let spec = found
            .manifest
            .is_some()
            .then(|| self.spec_for(&found, &settings));
        let Some(mut spec) = spec.flatten() else {
            let error = problems
                .first()
                .map(|p| p.message.clone())
                .unwrap_or_else(|| "cannot load".to_string());
            return Ok(self.keep(Run {
                id: 0,
                request,
                unsaved,
                run: failed(error, &kind),
                value: None,
                answer: None,
                defined: true,
                memory_bytes: None,
                requests: Vec::new(),
                writes: Vec::new(),
                store_before: None,
                store_after: None,
                problems,
            }));
        };
        if !request.real {
            spec.allowed = spec
                .manifest
                .hosts_needed(&spec.settings)
                .into_iter()
                .collect();
        }
        let _busy = self
            .begin(&request.name)
            .map_err(|_| WorkbenchError::Busy)?;
        let mut env = self.current_env();
        let mut tap = Tap::new(!request.real, request.reply.clone(), secrets.clone());
        let mut store_before = None;
        let mut used = None;
        if spec.manifest.permissions.store {
            let real = env.store_for(&request.name);
            let before = snapshot(&real, &[]);
            store_before = Some(masked(&before, &secrets));
            used = Some(if request.real {
                real
            } else {
                let scratch = Store::scratch(&before);
                let scratch: SharedStore = Arc::new(Mutex::new(scratch));
                tap.store = Some(scratch.clone());
                scratch
            });
        }
        env.tap = Some(tap.clone());
        let (reply, answer) = oneshot::channel();
        let slot = Slot::new(spec);
        slot.push(Job::Call {
            function,
            kind,
            args,
            limit: None,
            reply,
        });
        slot.finish();
        slot.spawn(env).map_err(|_| WorkbenchError::Unavailable)?;
        let reply = answer.await.map_err(|_| WorkbenchError::Unavailable)?;
        let recorded = std::mem::take(&mut *tap.recorded.lock().unwrap());
        let answer = match (&request.call, &reply.value) {
            (Call::Hook { hook, .. }, Some(v)) if reply.run.result == RunResult::Success => {
                hook_answer(hook, v)
            }
            _ => None,
        };
        Ok(self.keep(Run {
            id: 0,
            request,
            unsaved,
            run: reply.run,
            value: reply.value.map(|v| masked(&v, &secrets)),
            answer,
            defined: reply.defined,
            memory_bytes: recorded.memory_bytes,
            requests: recorded.requests,
            writes: recorded.writes,
            store_after: used.map(|s| snapshot(&s, &secrets)),
            store_before,
            problems,
        }))
    }

    /// Run a kept run's request again
    pub async fn workbench_rerun(&self, id: u64) -> Result<Run, WorkbenchError> {
        let request = {
            let runs = self.workbench_runs_lock().lock().unwrap();
            runs.1
                .iter()
                .find(|r| r.id == id)
                .map(|r| r.request.clone())
                .ok_or(WorkbenchError::NotFound)?
        };
        self.workbench_run(request).await
    }

    /// The kept runs, newest first
    pub fn workbench_runs(&self) -> Vec<Run> {
        let runs = self.workbench_runs_lock().lock().unwrap();
        runs.1.iter().rev().cloned().collect()
    }

    fn keep(&self, mut run: Run) -> Run {
        let mut runs = self.workbench_runs_lock().lock().unwrap();
        runs.0 += 1;
        run.id = runs.0;
        if runs.1.len() >= MAX_RUNS_KEPT {
            runs.1.pop_front();
        }
        runs.1.push_back(run.clone());
        run
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_are_masked_whole_and_secrets_everywhere() {
        let tap = Tap::new(true, Reply::default(), vec!["tk_SECRET".to_string()]);
        let mut headers = BTreeMap::new();
        headers.insert("Authorization".to_string(), "Bearer abc".to_string());
        headers.insert("X-Api-Key".to_string(), "abc".to_string());
        headers.insert("X-Note".to_string(), "has tk_SECRET".to_string());
        let summary = Summary {
            method: "POST".to_string(),
            url: "https://example.com/hook?key=tk_SECRET".to_string(),
            headers,
            body: "{\"token\":\"tk_SECRET\"}".to_string(),
        };
        tap.record_summary(summary, false, Ok(200), Duration::ZERO);
        let r = tap.recorded.lock().unwrap().requests[0].clone();
        assert_eq!(r.headers["Authorization"], MASK);
        assert_eq!(r.headers["X-Api-Key"], MASK);
        assert_eq!(r.headers["X-Note"], format!("has {}", MASK));
        assert!(!r.url.contains("tk_SECRET") && !r.body.contains("tk_SECRET"));
        assert_eq!((r.sent, r.status, r.duration_ms), (false, Some(200), None));
    }

    #[test]
    fn hook_answers_read_like_the_sync() {
        assert_eq!(hook_answer("filter", &json!("stream")).unwrap(), "stream");
        assert!(hook_answer("filter", &json!("maybe"))
            .unwrap()
            .starts_with("ignored: "));
        assert_eq!(
            hook_answer("filter", &Value::Null).unwrap(),
            "no answer: the built-in rule applies"
        );
        assert_eq!(hook_answer("rename", &json!("a.mkv")).unwrap(), "a.mkv");
    }

    #[test]
    fn every_event_type_has_a_sample_envelope() {
        let samples = event_samples();
        assert_eq!(samples.len(), EventType::ALL.len());
        for (kind, data) in samples {
            let e = envelope(&kind, &Value::Null).unwrap();
            assert_eq!(e["type"], kind.as_str());
            // Some samples carry the time they were made; compare their fields
            let fields = |v: &Value| v.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>());
            assert_eq!(fields(&e["data"]), fields(&data), "{}", kind);
        }
        let e = envelope("download.finished", &json!({"x": 1})).unwrap();
        assert_eq!(e["data"], json!({"x": 1}));
        assert!(envelope("nope", &Value::Null).is_err());
    }

    #[test]
    fn long_bodies_are_cut_on_a_char_boundary() {
        let text = "é".repeat(10);
        let out = cut(&text, 5);
        assert!(out.starts_with("éé…"), "{}", out);
    }
}
