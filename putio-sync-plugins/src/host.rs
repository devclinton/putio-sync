//! The host API, version 1 (contracts/host-api.md): the global `putio` table a plugin
//! calls into. WebAssembly plugins (029) reach the same calls through [`call_json`], in JSON
//! terms; both runtimes share the code behind each call.
//!
//! Every log line and error a plugin produces has the plugin's secret values replaced by
//! `********`. HTTP errors name only the host and what failed, never the URL.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mlua::{Lua, LuaSerdeExt, SerializeOptions, Table, Value as LuaValue};
use putio_sync_core::plugins::MASK;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::manifest::url_host;
use crate::sandbox::Deadline;
use crate::store::SharedStore;
use crate::workbench::Tap;

/// The host API version; additive changes keep it
pub const API_VERSION: i64 = 1;
/// Largest response body handed to a plugin
pub const MAX_BODY_BYTES: usize = 1024 * 1024;
/// Redirects followed, same host only
pub const MAX_REDIRECTS: usize = 3;
/// Most counter names per plugin (`putio.metrics.count`)
pub const MAX_COUNTERS: usize = 16;
/// Log lines kept per run
pub const MAX_LOG_LINES: usize = 20;
/// Longest log line kept
const MAX_LINE_CHARS: usize = 1000;
/// Request timeout when the plugin gives none (still capped by the call's deadline)
const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Largest file read or written through `putio.fs` (029)
pub const MAX_FILE_BYTES: usize = 1024 * 1024;
/// Most entries `putio.fs.list` gives back
pub const MAX_LIST_ENTRIES: usize = 1000;

/// Unix seconds with fractions; replaceable in tests
pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

/// The real clock
pub fn system_clock() -> Clock {
    Arc::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0)
    })
}

/// One line a plugin logged
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    /// `debug`, `info`, `warn` or `error`
    pub level: String,
    /// The message and its fields, secrets replaced
    pub message: String,
}

/// Lines logged during the current run
pub type Logs = Rc<RefCell<Vec<LogLine>>>;

thread_local! {
    /// Lines kept per run on this thread; a workbench worker keeps more (055)
    pub(crate) static LOG_LIMIT: std::cell::Cell<usize> = const { std::cell::Cell::new(MAX_LOG_LINES) };
}

/// The counter names a plugin has used, shared by its workers
pub type Counters = Arc<Mutex<BTreeSet<String>>>;

/// HTTP access: the hosts both needed and approved
pub struct Http {
    /// Lowercase host names the plugin may call
    pub allowed: HashSet<String>,
    /// Shared client, redirects off (they are followed here, same host only)
    pub client: reqwest::Client,
    /// The runtime the requests run on
    pub handle: tokio::runtime::Handle,
    /// A workbench run's recorder (055): requests are recorded, and in a dry run not sent
    pub tap: Option<Tap>,
}

/// File access (029): the approved folders, `{download_dir}` already put in
#[derive(Debug, Clone, Default)]
pub struct Files {
    /// Folders the plugin may read
    pub read: Vec<PathBuf>,
    /// Folders the plugin may read and write
    pub write: Vec<PathBuf>,
    /// A workbench run's recorder (055): a dry run records writes instead of doing them
    pub tap: Option<Tap>,
}

impl Files {
    /// The approved `r:`/`w:` folders that the manifest still asks for, with `download_dir`
    /// for `{download_dir}`; a folder that needs an unknown download folder is left out
    pub fn new(approved: &[String], needed: &[String], download_dir: &str) -> Files {
        let mut files = Files::default();
        for path in approved.iter().filter(|p| needed.contains(p)) {
            let (list, folder) = match path.split_once(':') {
                Some(("r", f)) => (&mut files.read, f),
                Some(("w", f)) => (&mut files.write, f),
                _ => continue,
            };
            let folder = match folder.strip_prefix(crate::manifest::DOWNLOAD_DIR) {
                Some(_) if download_dir.is_empty() => continue,
                Some(rest) => format!("{}{}", download_dir.trim_end_matches(['/', '\\']), rest),
                None => folder.to_string(),
            };
            list.push(PathBuf::from(folder));
        }
        files
    }

    /// `path` made real (links followed, `..` gone) if it lies in one of the folders that
    /// allow `write`. A path that does not exist yet is checked through its parent.
    fn resolve(&self, path: &str, write: bool) -> Result<PathBuf, String> {
        let wanted = Path::new(path);
        if !wanted.is_absolute() {
            return Err(format!("{}: not an absolute path", path));
        }
        let real = match std::fs::canonicalize(wanted) {
            Ok(real) => real,
            Err(_) => {
                let (Some(parent), Some(name)) = (wanted.parent(), wanted.file_name()) else {
                    return Err(format!("{}: not found", path));
                };
                if name == ".." {
                    return Err(format!("{}: not allowed", path));
                }
                std::fs::canonicalize(parent)
                    .map_err(|_| format!("{}: not found", path))?
                    .join(name)
            }
        };
        let roots = if write {
            self.write.iter().collect::<Vec<_>>()
        } else {
            self.read.iter().chain(&self.write).collect()
        };
        let inside = roots
            .iter()
            .any(|root| std::fs::canonicalize(root).is_ok_and(|root| real.starts_with(&root)));
        if !inside {
            let what = if write { "write" } else { "read" };
            return Err(format!(
                "{}: not in a folder this plugin may {}",
                path, what
            ));
        }
        Ok(real)
    }

    /// `putio.fs.read(path)`: the file's text, invalid UTF-8 replaced
    pub fn read(&self, path: &str) -> Result<String, String> {
        let real = self.resolve(path, false)?;
        let meta = std::fs::metadata(&real).map_err(|_| format!("{}: not found", path))?;
        if meta.is_dir() {
            return Err(format!("{}: is a folder", path));
        }
        if meta.len() > MAX_FILE_BYTES as u64 {
            return Err(format!(
                "{}: larger than {} KiB",
                path,
                MAX_FILE_BYTES / 1024
            ));
        }
        let bytes = std::fs::read(&real).map_err(|e| format!("{}: {}", path, e.kind()))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// `putio.fs.write(path, text)`: the file replaced, its folder made if missing
    pub fn write(&self, path: &str, text: &str) -> Result<(), String> {
        if text.len() > MAX_FILE_BYTES {
            return Err(format!(
                "{}: larger than {} KiB",
                path,
                MAX_FILE_BYTES / 1024
            ));
        }
        let real = self.resolve(path, true)?;
        if real.is_dir() {
            return Err(format!("{}: is a folder", path));
        }
        if let Some(tap) = self.tap.as_ref().filter(|t| t.dry) {
            tap.record_write(path, text.len());
            return Ok(());
        }
        std::fs::write(&real, text).map_err(|e| format!("{}: {}", path, e.kind()))
    }

    /// `putio.fs.list(folder)`: `{name, dir, size}` per entry, sorted by name
    pub fn list(&self, path: &str) -> Result<Value, String> {
        let real = self.resolve(path, false)?;
        let entries = std::fs::read_dir(&real).map_err(|_| format!("{}: not a folder", path))?;
        let mut out: Vec<(String, bool, u64)> = entries
            .flatten()
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                Some((
                    e.file_name().to_string_lossy().into_owned(),
                    meta.is_dir(),
                    meta.len(),
                ))
            })
            .collect();
        out.sort();
        out.truncate(MAX_LIST_ENTRIES);
        Ok(Value::Array(
            out.into_iter()
                .map(|(name, dir, size)| {
                    serde_json::json!({"name": name, "dir": dir, "size": if dir { 0 } else { size }})
                })
                .collect(),
        ))
    }

    /// `putio.fs.exists(path)`
    pub fn exists(&self, path: &str) -> Result<bool, String> {
        let real = self.resolve(path, false)?;
        Ok(real.exists())
    }
}

/// What one plugin's `putio` table is built from
pub struct Host {
    /// The plugin's name
    pub name: String,
    /// The plugin's version
    pub version: String,
    /// `putio.config`: defaults, values and secrets
    pub config: serde_json::Map<String, Value>,
    /// Secret values to scrub from logs and errors
    pub secrets: Rc<Vec<String>>,
    /// Absent without the `http` permission
    pub http: Option<Http>,
    /// Absent without the `store` permission
    pub store: Option<SharedStore>,
    /// Absent without `[permissions.files]` (029)
    pub files: Option<Rc<Files>>,
    /// `putio.download_dir`; empty when unknown
    pub download_dir: String,
    /// Names used with `putio.metrics.count`
    pub counters: Counters,
    /// The current call's deadline
    pub deadline: Deadline,
    /// Where log lines of the current run go
    pub logs: Logs,
    /// `putio.time.now()`
    pub clock: Clock,
}

/// `text` with every secret of 4 or more characters replaced by [`MASK`]
pub fn scrub(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_string();
    for secret in secrets {
        if secret.chars().count() >= 4 {
            out = out.replace(secret.as_str(), MASK);
        }
    }
    out
}

/// Conversion to Lua with JSON nulls as `nil`
pub fn to_lua(lua: &Lua, value: &Value) -> mlua::Result<LuaValue> {
    let opts = SerializeOptions::new()
        .serialize_none_to_null(false)
        .serialize_unit_to_null(false);
    lua.to_value_with(value, opts)
}

/// Conversion from Lua; an empty table becomes `{}`
pub fn from_lua(lua: &Lua, value: LuaValue) -> Result<Value, String> {
    lua.from_value::<Value>(value)
        .map_err(|e| crate::sandbox::error_text(&e))
}

fn fail(lua: &Lua, error: String) -> mlua::Result<(LuaValue, LuaValue)> {
    Ok((LuaValue::Nil, LuaValue::String(lua.create_string(error)?)))
}

/// Install `putio` (and `print`) in `lua`
pub fn install(lua: &Lua, host: Host) -> mlua::Result<()> {
    let putio = lua.create_table()?;
    putio.set("log", log_table(lua, &host)?)?;
    putio.set("config", to_lua(lua, &Value::Object(host.config.clone()))?)?;
    if let Some(http) = host.http {
        putio.set(
            "http",
            http_table(lua, http, host.deadline.clone(), host.secrets.clone())?,
        )?;
    }
    if let Some(store) = host.store {
        putio.set("store", store_table(lua, store)?)?;
    }
    if let Some(files) = host.files {
        putio.set("fs", fs_table(lua, files, host.secrets.clone())?)?;
    }
    putio.set("time", time_table(lua, host.clock.clone())?)?;
    putio.set("json", json_table(lua)?)?;
    putio.set("path", path_table(lua)?)?;
    putio.set(
        "metrics",
        metrics_table(lua, host.name.clone(), host.counters.clone())?,
    )?;
    putio.set("download_dir", host.download_dir.clone())?;
    let plugin = lua.create_table()?;
    plugin.set("name", host.name.clone())?;
    plugin.set("version", host.version.clone())?;
    putio.set("plugin", plugin)?;
    putio.set("version", env!("CARGO_PKG_VERSION"))?;
    putio.set("api_version", API_VERSION)?;

    let info: mlua::Function = putio.get::<Table>("log")?.get("info")?;
    let print = lua.create_function(move |_, args: mlua::Variadic<LuaValue>| {
        let parts: Vec<String> = args
            .iter()
            .map(|v| v.to_string().unwrap_or_else(|_| v.type_name().to_string()))
            .collect();
        info.call::<()>(parts.join("\t"))
    })?;
    lua.globals().set("print", print)?;
    lua.globals().set("putio", putio)?;
    Ok(())
}

/// Log `message` and its sorted `fields` at `level`, secrets replaced, and keep the line
/// with the run's logs
pub fn log(
    name: &str,
    secrets: &[String],
    logs: &Logs,
    level: &str,
    message: String,
    fields: BTreeMap<String, String>,
) {
    let mut text = message;
    for (k, v) in fields {
        text.push_str(&format!(" {}={}", k, v));
    }
    let mut text = scrub(&text, secrets);
    if text.chars().count() > MAX_LINE_CHARS {
        text = text.chars().take(MAX_LINE_CHARS).collect::<String>() + "…";
    }
    match level {
        "debug" => tracing::debug!(plugin = %name, "{}", text),
        "info" => tracing::info!(plugin = %name, "{}", text),
        "warn" => tracing::warn!(plugin = %name, "{}", text),
        _ => tracing::error!(plugin = %name, "{}", text),
    }
    let mut logs = logs.borrow_mut();
    if logs.len() < LOG_LIMIT.with(|l| l.get()) {
        logs.push(LogLine {
            level: level.to_string(),
            message: text,
        });
    }
}

/// The log levels, as `putio.log` names them
pub const LEVELS: [&str; 4] = ["debug", "info", "warn", "error"];

fn log_table(lua: &Lua, host: &Host) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for level in LEVELS {
        let name = host.name.clone();
        let secrets = host.secrets.clone();
        let logs = host.logs.clone();
        let f = lua.create_function(move |_, (message, fields): (LuaValue, Option<Table>)| {
            let text = message
                .to_string()
                .unwrap_or_else(|_| message.type_name().to_string());
            let mut pairs = BTreeMap::new();
            if let Some(fields) = fields {
                for pair in fields.pairs::<String, LuaValue>() {
                    let (k, v) = pair?;
                    let v = v.to_string().unwrap_or_else(|_| v.type_name().to_string());
                    pairs.insert(k, v);
                }
            }
            log(&name, &secrets, &logs, level, text, pairs);
            Ok(())
        })?;
        table.set(level, f)?;
    }
    Ok(table)
}

#[derive(Deserialize)]
pub(crate) struct RequestOptions {
    pub(crate) method: Option<String>,
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Option<String>,
    timeout: Option<f64>,
}

pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Vec<u8>,
    pub(crate) truncated: bool,
}

fn http_error(host: &str, e: &reqwest::Error) -> String {
    let what = if e.is_timeout() {
        "timed out"
    } else if e.is_connect() {
        "connection failed"
    } else if e.is_builder() {
        "invalid request"
    } else {
        "request failed"
    };
    format!("{}: {}", host, what)
}

async fn fetch(
    client: &reqwest::Client,
    allowed: &HashSet<String>,
    opts: RequestOptions,
) -> Result<Response, String> {
    let mut url = reqwest::Url::parse(&opts.url).map_err(|_| "invalid URL".to_string())?;
    let mut method = match opts.method.as_deref() {
        None => reqwest::Method::GET,
        Some(m) => reqwest::Method::from_bytes(m.to_ascii_uppercase().as_bytes())
            .map_err(|_| format!("invalid method \"{}\"", m))?,
    };
    let mut body = opts.body;
    let check = |url: &reqwest::Url| -> Result<String, String> {
        let host = url_host(url.as_str()).ok_or("only http and https URLs")?;
        if !allowed.contains(&host) {
            return Err(format!("host not approved: {}", host));
        }
        Ok(host)
    };
    let first = check(&url)?;
    for hop in 0..=MAX_REDIRECTS {
        let host = check(&url)?;
        let mut request = client.request(method.clone(), url.clone());
        for (k, v) in &opts.headers {
            request = request.header(k.as_str(), v.as_str());
        }
        if let Some(b) = &body {
            request = request.body(b.clone());
        }
        let mut resp = request.send().await.map_err(|e| http_error(&host, &e))?;
        let status = resp.status();
        if status.is_redirection() {
            if let Some(location) = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|l| l.to_str().ok())
            {
                if hop == MAX_REDIRECTS {
                    return Err(format!("{}: too many redirects", host));
                }
                let next = url
                    .join(location)
                    .map_err(|_| format!("{}: bad redirect", host))?;
                if url_host(next.as_str()).as_deref() != Some(first.as_str()) {
                    return Err(format!("{}: redirect to another host refused", host));
                }
                let to_get = status == reqwest::StatusCode::SEE_OTHER
                    || (matches!(status.as_u16(), 301 | 302) && method == reqwest::Method::POST);
                if to_get {
                    method = reqwest::Method::GET;
                    body = None;
                }
                url = next;
                continue;
            }
        }
        let headers = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string())))
            .collect();
        let mut bytes = Vec::new();
        let mut truncated = false;
        while let Some(chunk) = resp.chunk().await.map_err(|e| http_error(&host, &e))? {
            let room = MAX_BODY_BYTES - bytes.len();
            if chunk.len() > room {
                bytes.extend_from_slice(&chunk[..room]);
                truncated = true;
                break;
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(Response {
            status: status.as_u16(),
            headers,
            body: bytes,
            truncated,
        });
    }
    Err(format!("{}: too many redirects", first))
}

/// `putio.http.request`: one request under the call's deadline
pub(crate) fn request(
    http: &Http,
    deadline: &Deadline,
    opts: RequestOptions,
) -> Result<Response, String> {
    let wanted = opts
        .timeout
        .filter(|t| t.is_finite() && *t > 0.0)
        .map(|t| Duration::from_secs_f64(t.min(3600.0)))
        .unwrap_or(DEFAULT_HTTP_TIMEOUT);
    let left = deadline
        .get()
        .map(|d| d.saturating_duration_since(Instant::now()))
        .unwrap_or(wanted);
    if left.is_zero() {
        return Err(crate::sandbox::TIME_LIMIT.to_string());
    }
    let host = url_host(&opts.url).unwrap_or_default();
    if let Some(tap) = &http.tap {
        if tap.dry {
            if !http.allowed.contains(&host) {
                let error = match url_host(&opts.url) {
                    None => "only http and https URLs".to_string(),
                    Some(_) => format!("host not approved: {}", host),
                };
                tap.record_request(&opts, false, Err(&error), Duration::ZERO);
                return Err(error);
            }
            let reply = tap.reply();
            tap.record_request(&opts, false, Ok(reply.status), Duration::ZERO);
            return Ok(reply);
        }
    }
    let limit = wanted.min(left);
    let started = Instant::now();
    let summary = http.tap.as_ref().map(|_| Summary::of(&opts));
    let result = http.handle.block_on(async {
        tokio::time::timeout(limit, fetch(&http.client, &http.allowed, opts)).await
    });
    let result = match result {
        Err(_) => Err(format!("{}: timed out", host)),
        Ok(r) => r,
    };
    if let (Some(tap), Some(summary)) = (&http.tap, summary) {
        let status = result.as_ref().map(|r| r.status).map_err(|e| e.as_str());
        tap.record_summary(summary, true, status, started.elapsed());
    }
    result
}

/// What a workbench run keeps of a request that has been handed to [`fetch`]
pub(crate) struct Summary {
    pub(crate) method: String,
    pub(crate) url: String,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: String,
}

impl Summary {
    pub(crate) fn of(opts: &RequestOptions) -> Summary {
        Summary {
            method: opts.method.as_deref().unwrap_or("GET").to_ascii_uppercase(),
            url: opts.url.clone(),
            headers: opts.headers.clone(),
            body: opts.body.clone().unwrap_or_default(),
        }
    }
}

fn http_table(
    lua: &Lua,
    http: Http,
    deadline: Deadline,
    secrets: Rc<Vec<String>>,
) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let request = lua.create_function(move |lua, opts: LuaValue| {
        let opts: RequestOptions = match lua.from_value(opts) {
            Ok(o) => o,
            Err(e) => {
                return fail(
                    lua,
                    format!("bad request options: {}", crate::sandbox::error_text(&e)),
                )
            }
        };
        match request(&http, &deadline, opts) {
            Err(e) => fail(lua, scrub(&e, &secrets)),
            Ok(r) => {
                let t = lua.create_table()?;
                t.set("status", r.status)?;
                let headers = lua.create_table()?;
                for (k, v) in r.headers {
                    headers.set(k, v)?;
                }
                t.set("headers", headers)?;
                t.set("body", lua.create_string(&r.body)?)?;
                t.set("truncated", r.truncated)?;
                Ok((LuaValue::Table(t), LuaValue::Nil))
            }
        }
    })?;
    table.set("request", request)?;
    Ok(table)
}

fn store_table(lua: &Lua, store: SharedStore) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let s = store.clone();
    table.set(
        "get",
        lua.create_function(move |lua, key: String| match s.lock().unwrap().get(&key) {
            Some(v) => to_lua(lua, v),
            None => Ok(LuaValue::Nil),
        })?,
    )?;
    let s = store.clone();
    table.set(
        "set",
        lua.create_function(move |lua, (key, value): (String, LuaValue)| {
            let value = match from_lua(lua, value) {
                Ok(v) => v,
                Err(e) => return fail(lua, format!("store value: {}", e)),
            };
            let set = s.lock().unwrap().set(&key, value);
            match set {
                Ok(()) => Ok((LuaValue::Boolean(true), LuaValue::Nil)),
                Err(e) => fail(lua, e),
            }
        })?,
    )?;
    let s = store.clone();
    table.set(
        "delete",
        lua.create_function(
            move |lua, key: String| match s.lock().unwrap().delete(&key) {
                Ok(()) => Ok((LuaValue::Boolean(true), LuaValue::Nil)),
                Err(e) => fail(lua, e),
            },
        )?,
    )?;
    table.set(
        "keys",
        lua.create_function(move |_, ()| Ok(store.lock().unwrap().keys()))?,
    )?;
    Ok(table)
}

/// `putio.path` (031): path rules for a media server that sees other paths
fn path_table(lua: &Lua) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "map",
        lua.create_function(|lua, (path, rules): (String, LuaValue)| {
            let rules = match rules {
                LuaValue::Nil => Vec::new(),
                v => {
                    let v = from_lua(lua, v).map_err(mlua::Error::runtime)?;
                    // An empty Lua table converts to an object
                    let v = match v {
                        Value::Object(o) if o.is_empty() => Value::Array(Vec::new()),
                        v => v,
                    };
                    crate::paths::rules(&v)
                        .map_err(|e| mlua::Error::runtime(format!("path rules: {}", e)))?
                }
            };
            Ok(crate::paths::map(&path, &rules))
        })?,
    )?;
    table.set(
        "parent",
        lua.create_function(|_, path: String| Ok(crate::paths::parent(&path)))?,
    )?;
    table.set(
        "within",
        lua.create_function(|_, (path, folder): (String, String)| {
            Ok(crate::paths::within(&path, &folder))
        })?,
    )?;
    Ok(table)
}

/// `putio.metrics` (031): `count(name, n)` adds to
/// `putio_sync_plugin_counter_total{plugin, name}`
fn metrics_table(lua: &Lua, plugin: String, counters: Counters) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "count",
        lua.create_function(move |_, (name, n): (String, Option<f64>)| {
            count(&plugin, &counters, &name, n).map_err(mlua::Error::runtime)
        })?,
    )?;
    Ok(table)
}

/// `putio.metrics.count(name, n)`
pub fn count(plugin: &str, counters: &Counters, name: &str, n: Option<f64>) -> Result<(), String> {
    let name_ok = !name.is_empty()
        && name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !name_ok {
        return Err("metrics.count: name may only use a-z, 0-9 and _ (1 to 40)".to_string());
    }
    let n = n.unwrap_or(1.0);
    if !(n.is_finite() && n >= 0.0 && n.fract() == 0.0) {
        return Err("metrics.count: n must be a whole number, 0 or more".to_string());
    }
    {
        let mut used = counters.lock().unwrap();
        if !used.contains(name) {
            if used.len() >= MAX_COUNTERS {
                return Err(format!(
                    "metrics.count: at most {} names per plugin",
                    MAX_COUNTERS
                ));
            }
            used.insert(name.to_string());
        }
    }
    putio_sync_telemetry::plugins::record_counter(plugin, name, n as u64);
    Ok(())
}

/// `putio.time.iso(seconds)`
pub fn iso(seconds: f64) -> Result<String, String> {
    let millis = (seconds * 1000.0) as i64;
    let at = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis)
        .ok_or_else(|| "time out of range".to_string())?;
    Ok(at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// `putio.fs` (029): files in the approved folders
fn fs_table(lua: &Lua, files: Rc<Files>, secrets: Rc<Vec<String>>) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let (f, s) = (files.clone(), secrets.clone());
    table.set(
        "read",
        lua.create_function(move |lua, path: String| match f.read(&path) {
            Ok(text) => Ok((LuaValue::String(lua.create_string(text)?), LuaValue::Nil)),
            Err(e) => fail(lua, scrub(&e, &s)),
        })?,
    )?;
    let (f, s) = (files.clone(), secrets.clone());
    table.set(
        "write",
        lua.create_function(move |lua, (path, text): (String, mlua::String)| {
            match f.write(&path, &text.to_str()?) {
                Ok(()) => Ok((LuaValue::Boolean(true), LuaValue::Nil)),
                Err(e) => fail(lua, scrub(&e, &s)),
            }
        })?,
    )?;
    let (f, s) = (files.clone(), secrets.clone());
    table.set(
        "list",
        lua.create_function(move |lua, path: String| match f.list(&path) {
            Ok(v) => Ok((to_lua(lua, &v)?, LuaValue::Nil)),
            Err(e) => fail(lua, scrub(&e, &s)),
        })?,
    )?;
    table.set(
        "exists",
        lua.create_function(move |lua, path: String| match files.exists(&path) {
            Ok(b) => Ok((LuaValue::Boolean(b), LuaValue::Nil)),
            Err(e) => fail(lua, scrub(&e, &secrets)),
        })?,
    )?;
    Ok(table)
}

fn time_table(lua: &Lua, clock: Clock) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let now = clock.clone();
    table.set("now", lua.create_function(move |_, ()| Ok(now()))?)?;
    table.set(
        "iso",
        lua.create_function(move |_, seconds: Option<f64>| {
            iso(seconds.unwrap_or_else(|| clock())).map_err(mlua::Error::runtime)
        })?,
    )?;
    Ok(table)
}

fn json_table(lua: &Lua) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set(
        "encode",
        lua.create_function(|lua, value: LuaValue| match from_lua(lua, value) {
            Ok(v) => match serde_json::to_string(&v) {
                Ok(text) => Ok((LuaValue::String(lua.create_string(text)?), LuaValue::Nil)),
                Err(e) => fail(lua, e.to_string()),
            },
            Err(e) => fail(lua, e),
        })?,
    )?;
    table.set(
        "decode",
        lua.create_function(|lua, text: mlua::String| {
            match serde_json::from_slice::<Value>(&text.as_bytes()) {
                Ok(v) => Ok((to_lua(lua, &v)?, LuaValue::Nil)),
                Err(e) => fail(lua, format!("invalid JSON: {}", e)),
            }
        })?,
    )?;
    Ok(table)
}

/// A string argument of a JSON call
fn text_arg<'a>(args: &'a [Value], i: usize, what: &str) -> Result<&'a str, String> {
    args.get(i)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{}: argument {} must be a string", what, i + 1))
}

/// One host call in JSON terms (029, contracts/wasm-abi.md): `function` is the dotted name
/// Lua uses (`log.info`, `http.request`, `store.get`...) and `args` its arguments. A call the
/// plugin has no permission for, or does not exist, is an error like any other.
pub fn call_json(host: &Host, function: &str, args: &[Value]) -> Result<Value, String> {
    let result = dispatch(host, function, args);
    result.map_err(|e| scrub(&e, &host.secrets))
}

fn dispatch(host: &Host, function: &str, args: &[Value]) -> Result<Value, String> {
    let no_permission = |what: &str| format!("{}: the plugin has no {} permission", function, what);
    if let Some(level) = function.strip_prefix("log.") {
        if !LEVELS.contains(&level) {
            return Err(format!("unknown host call \"{}\"", function));
        }
        let message = match args.first() {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(v) => v.to_string(),
        };
        let mut fields = BTreeMap::new();
        if let Some(Value::Object(o)) = args.get(1) {
            for (k, v) in o {
                let v = match v {
                    Value::String(s) => s.clone(),
                    v => v.to_string(),
                };
                fields.insert(k.clone(), v);
            }
        }
        log(
            &host.name,
            &host.secrets,
            &host.logs,
            level,
            message,
            fields,
        );
        return Ok(Value::Null);
    }
    match function {
        "config" => Ok(Value::Object(host.config.clone())),
        "plugin" => Ok(serde_json::json!({"name": host.name, "version": host.version})),
        "version" => Ok(Value::String(env!("CARGO_PKG_VERSION").to_string())),
        "api_version" => Ok(Value::from(API_VERSION)),
        "download_dir" => Ok(Value::String(host.download_dir.clone())),
        "http.request" => {
            let http = host.http.as_ref().ok_or_else(|| no_permission("http"))?;
            let opts: RequestOptions =
                serde_json::from_value(args.first().cloned().unwrap_or(Value::Null))
                    .map_err(|e| format!("bad request options: {}", e))?;
            let r = request(http, &host.deadline, opts)?;
            Ok(serde_json::json!({
                "status": r.status,
                "headers": r.headers,
                "body": String::from_utf8_lossy(&r.body),
                "truncated": r.truncated,
            }))
        }
        "store.get" | "store.set" | "store.delete" | "store.keys" => {
            let store = host.store.as_ref().ok_or_else(|| no_permission("store"))?;
            let mut store = store.lock().unwrap();
            match function {
                "store.get" => Ok(store
                    .get(text_arg(args, 0, function)?)
                    .cloned()
                    .unwrap_or(Value::Null)),
                "store.set" => {
                    let value = args.get(1).cloned().unwrap_or(Value::Null);
                    store.set(text_arg(args, 0, function)?, value)?;
                    Ok(Value::Bool(true))
                }
                "store.delete" => {
                    store.delete(text_arg(args, 0, function)?)?;
                    Ok(Value::Bool(true))
                }
                _ => Ok(Value::from(store.keys())),
            }
        }
        "fs.read" | "fs.write" | "fs.list" | "fs.exists" => {
            let files = host.files.as_ref().ok_or_else(|| no_permission("files"))?;
            let path = text_arg(args, 0, function)?;
            match function {
                "fs.read" => files.read(path).map(Value::String),
                "fs.write" => files
                    .write(path, text_arg(args, 1, function)?)
                    .map(|()| Value::Bool(true)),
                "fs.list" => files.list(path),
                _ => files.exists(path).map(Value::Bool),
            }
        }
        "time.now" => Ok(Value::from((host.clock)())),
        "time.iso" => {
            let seconds = args
                .first()
                .and_then(Value::as_f64)
                .unwrap_or_else(|| (host.clock)());
            iso(seconds).map(Value::String)
        }
        "json.encode" => serde_json::to_string(args.first().unwrap_or(&Value::Null))
            .map(Value::String)
            .map_err(|e| e.to_string()),
        "json.decode" => serde_json::from_str(text_arg(args, 0, function)?)
            .map_err(|e| format!("invalid JSON: {}", e)),
        "path.map" => {
            let rules = match args.get(1) {
                None | Some(Value::Null) => Vec::new(),
                Some(v) => crate::paths::rules(v).map_err(|e| format!("path rules: {}", e))?,
            };
            Ok(Value::String(crate::paths::map(
                text_arg(args, 0, function)?,
                &rules,
            )))
        }
        "path.parent" => Ok(Value::String(crate::paths::parent(text_arg(
            args, 0, function,
        )?))),
        "path.within" => Ok(Value::Bool(crate::paths::within(
            text_arg(args, 0, function)?,
            text_arg(args, 1, function)?,
        ))),
        "metrics.count" => {
            let n = args.get(1).and_then(Value::as_f64);
            count(&host.name, &host.counters, text_arg(args, 0, function)?, n)?;
            Ok(Value::Null)
        }
        _ => Err(format!("unknown host call \"{}\"", function)),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    struct Fixture {
        lua: Lua,
        logs: Logs,
        deadline: Deadline,
        _rt: Option<tokio::runtime::Runtime>,
    }

    fn host(
        http: Option<(Vec<&str>, tokio::runtime::Handle)>,
        store: bool,
        files: Option<Files>,
        deadline: &Deadline,
        logs: &Logs,
    ) -> Host {
        Host {
            name: "t".to_string(),
            version: "1.2".to_string(),
            config: serde_json::json!({"topic": "nas", "token": "tk_SECRET"})
                .as_object()
                .unwrap()
                .clone(),
            secrets: Rc::new(vec!["tk_SECRET".to_string(), "abc".to_string()]),
            http: http.map(|(hosts, handle)| Http {
                allowed: hosts.into_iter().map(String::from).collect(),
                client: reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap(),
                handle,
                tap: None,
            }),
            store: store.then(|| Arc::new(Mutex::new(crate::store::Store::open(None, "t")))),
            files: files.map(Rc::new),
            download_dir: "/downloads".to_string(),
            counters: Counters::default(),
            deadline: deadline.clone(),
            logs: logs.clone(),
            clock: Arc::new(|| 1_793_784_600.5),
        }
    }

    fn fixture(http: Option<Vec<&str>>, store: bool) -> Fixture {
        let deadline: Deadline = Rc::new(Cell::new(None));
        let lua = crate::sandbox::new_state(16, deadline.clone()).unwrap();
        let logs: Logs = Rc::default();
        let rt = http.as_ref().map(|_| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .unwrap()
        });
        let handle = rt.as_ref().map(|rt| rt.handle().clone());
        let host = host(http.zip(handle), store, None, &deadline, &logs);
        install(&lua, host).unwrap();
        Fixture {
            lua,
            logs,
            deadline,
            _rt: rt,
        }
    }

    #[test]
    fn plain_calls() {
        let f = fixture(None, false);
        let lua = &f.lua;
        let ok: bool = lua
            .load(
                r#"
                assert(putio.plugin.name == "t" and putio.plugin.version == "1.2")
                assert(putio.api_version == 1 and type(putio.version) == "string")
                assert(putio.config.topic == "nas")
                assert(putio.http == nil and putio.store == nil)
                assert(putio.time.now() == 1793784600.5)
                assert(putio.time.iso(0) == "1970-01-01T00:00:00Z")
                assert(putio.time.iso() == "2026-11-04T09:30:00Z")
                local v = putio.json.decode('{"a":[1,2],"b":null,"c":"x"}')
                assert(#v.a == 2 and v.b == nil and v.c == "x")
                assert(putio.json.encode({x = 1}) == '{"x":1}')
                local bad, err = putio.json.decode("{")
                assert(bad == nil and err:find("invalid JSON"))
                return true
                "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
    }

    #[test]
    fn path_and_metrics_calls() {
        let f = fixture(None, false);
        let ok: bool = f
            .lua
            .load(
                r#"
                assert(putio.download_dir == "/downloads")
                local rules = {{from = "/downloads", to = "/data"}, {from = "/downloads/TV", to = "D:\\TV"}}
                assert(putio.path.map("/downloads/Movies/x", rules) == "/data/Movies/x")
                assert(putio.path.map("/downloads/TV/S/e.mkv", rules) == "D:\\TV\\S\\e.mkv")
                assert(putio.path.map("/other", {}) == "/other")
                assert(putio.path.map("/other", nil) == "/other")
                assert(putio.path.parent("/a/b/c.mkv") == "/a/b")
                assert(putio.path.within("/a/b", "/a") and not putio.path.within("/ab", "/a"))
                local ok, err = pcall(putio.path.map, "/x", {{from = "x", to = "/y"}})
                assert(not ok and tostring(err):find("rule 1: from"))
                for i = 1, 16 do putio.metrics.count("c" .. i) end
                putio.metrics.count("c1", 3)
                local ok2, err2 = pcall(putio.metrics.count, "c17")
                assert(not ok2 and tostring(err2):find("at most 16"))
                local ok3 = pcall(putio.metrics.count, "Bad Name")
                local ok4 = pcall(putio.metrics.count, "c2", -1)
                return not ok3 and not ok4
                "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
    }

    #[test]
    fn logs_are_captured_and_scrubbed() {
        let f = fixture(None, false);
        f.lua
            .load(r#"putio.log.warn("token is " .. putio.config.token, {n = 2, why = "abc"}) print("a", 1)"#)
            .exec()
            .unwrap();
        let logs = f.logs.borrow();
        assert_eq!(logs[0].level, "warn");
        assert_eq!(logs[0].message, "token is ******** n=2 why=abc");
        assert_eq!(
            logs[1],
            LogLine {
                level: "info".into(),
                message: "a\t1".into()
            }
        );
        drop(logs);
        f.lua
            .load("for i = 1, 50 do putio.log.debug(string.rep('x', 2000)) end")
            .exec()
            .unwrap();
        assert_eq!(f.logs.borrow().len(), MAX_LOG_LINES);
        assert!(f.logs.borrow()[5].message.chars().count() <= MAX_LINE_CHARS + 1);
    }

    #[test]
    fn store_calls() {
        let f = fixture(None, true);
        let ok: bool = f
            .lua
            .load(
                r#"
                assert(putio.store.set("seen", {"a", "b"}))
                assert(putio.store.get("seen")[2] == "b")
                assert(putio.store.get("nope") == nil)
                local ok, err = putio.store.set(string.rep("k", 200), 1)
                assert(ok == nil and err:find("1 to 128"))
                local ok2, err2 = putio.store.set("f", function() end)
                assert(ok2 == nil and err2:find("store value"))
                putio.store.delete("seen")
                return #putio.store.keys() == 0
                "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
    }

    fn request(f: &Fixture, url: &str) -> (Option<u16>, Option<String>) {
        f.deadline
            .set(Some(Instant::now() + Duration::from_secs(5)));
        f.lua.globals().set("URL", url).unwrap();
        let (status, err): (Option<u16>, Option<String>) = f
            .lua
            .load(
                r#"
                local r, err = putio.http.request{method = "post", url = URL,
                  headers = {["X-Test"] = "1"}, body = putio.json.encode({a = 1})}
                if r then return r.status, r.body end
                return nil, err
                "#,
            )
            .eval()
            .unwrap();
        f.deadline.set(None);
        (status, err)
    }

    #[test]
    fn approved_hosts_only() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/hook?key=tk_SECRET")
            .match_header("x-test", "1")
            .match_body(r#"{"a":1}"#)
            .with_status(201)
            .with_body("ok")
            .create();
        let f = fixture(Some(vec!["127.0.0.1"]), false);
        let url = format!("{}/hook?key=tk_SECRET", server.url());
        assert_eq!(request(&f, &url), (Some(201), Some("ok".to_string())));
        mock.assert();

        let (_, err) = request(&f, "https://example.com/x");
        assert_eq!(err.as_deref(), Some("host not approved: example.com"));
        let (_, err) = request(&f, "file:///etc/passwd");
        assert_eq!(err.as_deref(), Some("only http and https URLs"));

        // Errors never show the path or query.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let (_, err) = request(&f, &format!("http://127.0.0.1:{}/hook?key=tk_SECRET", port));
        let err = err.unwrap();
        assert!(err.starts_with("127.0.0.1: "), "{}", err);
        assert!(!err.contains("hook") && !err.contains("SECRET"), "{}", err);
    }

    #[test]
    fn redirects_stay_on_the_host() {
        let mut server = mockito::Server::new();
        server
            .mock("POST", "/a")
            .with_status(307)
            .with_header("location", "/b")
            .create();
        server
            .mock("POST", "/b")
            .with_status(200)
            .with_body("b")
            .create();
        server
            .mock("POST", "/away")
            .with_status(302)
            .with_header("location", "https://example.com/")
            .create();
        let f = fixture(Some(vec!["127.0.0.1", "example.com"]), false);
        assert_eq!(
            request(&f, &format!("{}/a", server.url())),
            (Some(200), Some("b".to_string()))
        );
        let (_, err) = request(&f, &format!("{}/away", server.url()));
        assert_eq!(
            err.as_deref(),
            Some("127.0.0.1: redirect to another host refused")
        );
    }

    #[test]
    fn big_bodies_are_cut() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/big")
            .with_body("x".repeat(MAX_BODY_BYTES + 10))
            .create();
        let f = fixture(Some(vec!["127.0.0.1"]), false);
        f.deadline
            .set(Some(Instant::now() + Duration::from_secs(5)));
        f.lua
            .globals()
            .set("URL", format!("{}/big", server.url()))
            .unwrap();
        let (len, truncated): (usize, bool) = f
            .lua
            .load("local r = putio.http.request{url = URL} return #r.body, r.truncated")
            .eval()
            .unwrap();
        assert_eq!((len, truncated), (MAX_BODY_BYTES, true));
    }

    fn json_host(files: Option<Files>, store: bool) -> (Host, Logs) {
        let logs: Logs = Rc::default();
        let host = host(None, store, files, &Rc::new(Cell::new(None)), &logs);
        (host, logs)
    }

    #[test]
    fn json_calls_match_the_lua_api() {
        let (host, logs) = json_host(None, true);
        let call = |f: &str, args: Value| call_json(&host, f, args.as_array().unwrap());
        assert_eq!(
            call("config", serde_json::json!([])).unwrap()["topic"],
            "nas"
        );
        assert_eq!(
            call("plugin", serde_json::json!([])).unwrap()["version"],
            "1.2"
        );
        assert_eq!(call("api_version", serde_json::json!([])).unwrap(), 1);
        assert_eq!(
            call("download_dir", serde_json::json!([])).unwrap(),
            "/downloads"
        );
        assert_eq!(
            call("time.iso", serde_json::json!([0])).unwrap(),
            "1970-01-01T00:00:00Z"
        );
        assert_eq!(
            call("json.decode", serde_json::json!(["[1]"])).unwrap(),
            serde_json::json!([1])
        );
        assert_eq!(
            call("path.parent", serde_json::json!(["/a/b"])).unwrap(),
            "/a"
        );
        call("store.set", serde_json::json!(["k", {"n": 2}])).unwrap();
        assert_eq!(call("store.get", serde_json::json!(["k"])).unwrap()["n"], 2);
        assert_eq!(
            call("store.keys", serde_json::json!([])).unwrap(),
            serde_json::json!(["k"])
        );
        // Secrets are replaced in log lines and errors
        call("log.warn", serde_json::json!(["token tk_SECRET", {"x": 1}])).unwrap();
        let line = logs.borrow()[0].clone();
        assert_eq!(line.level, "warn");
        assert!(!line.message.contains("tk_SECRET"), "{}", line.message);
        let e = call("json.decode", serde_json::json!(["tk_SECRET{"])).unwrap_err();
        assert!(!e.contains("tk_SECRET"), "{e}");
        assert_eq!(
            call("nope", serde_json::json!([])).unwrap_err(),
            "unknown host call \"nope\""
        );
        assert_eq!(
            call("log.loud", serde_json::json!(["x"])).unwrap_err(),
            "unknown host call \"log.loud\""
        );
    }

    #[test]
    fn json_calls_need_their_permission() {
        let (host, _) = json_host(None, false);
        for (f, what) in [
            ("http.request", "http"),
            ("store.get", "store"),
            ("fs.read", "files"),
        ] {
            let e = call_json(&host, f, &[Value::from("/x")]).unwrap_err();
            assert!(e.contains(&format!("no {what} permission")), "{f}: {e}");
        }
    }

    #[test]
    fn files_stay_in_their_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let (inbox, out, secret) = (root.join("inbox"), root.join("out"), root.join("secret"));
        for d in [&inbox, &out, &secret] {
            std::fs::create_dir(d).unwrap();
        }
        std::fs::write(inbox.join("a.txt"), "hello").unwrap();
        std::fs::write(secret.join("key"), "s3cret").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(secret.join("key"), inbox.join("link")).unwrap();
        let entries = [
            format!("r:{}", inbox.display()),
            format!("w:{}", out.display()),
            format!("r:{}", secret.display()),
        ];
        // The secret folder is approved but no longer asked for
        let files = Files::new(&entries, &entries[..2], "");
        let p = |path: &Path| path.display().to_string();

        assert_eq!(files.read(&p(&inbox.join("a.txt"))).unwrap(), "hello");
        assert!(files.exists(&p(&inbox.join("a.txt"))).unwrap());
        let listed = files.list(&p(&inbox)).unwrap();
        assert_eq!(listed[0]["name"], "a.txt");
        files.write(&p(&out.join("b.txt")), "made").unwrap();
        assert_eq!(files.read(&p(&out.join("b.txt"))).unwrap(), "made");

        let refused = [
            files.read(&p(&secret.join("key"))),
            files.read(&format!("{}/../secret/key", inbox.display())),
            files.read("inbox/a.txt"),
            files
                .write(&p(&inbox.join("c.txt")), "x")
                .map(|_| String::new()),
            files
                .write(&format!("{}/../c.txt", out.display()), "x")
                .map(|_| String::new()),
        ];
        for r in refused {
            assert!(r.is_err(), "{r:?}");
        }
        #[cfg(unix)]
        assert!(files.read(&p(&inbox.join("link"))).is_err());
        assert!(!root.join("c.txt").exists());
    }

    #[test]
    fn download_dir_folders_need_a_download_dir() {
        let entries = vec!["w:{download_dir}/done".to_string(), "x:/what".to_string()];
        assert!(Files::new(&entries, &entries, "").write.is_empty());
        let files = Files::new(&entries, &entries, "/data/");
        assert_eq!(files.write, vec![PathBuf::from("/data/done")]);
        assert!(files.read.is_empty());
    }
}
