//! The host API, version 1 (contracts/host-api.md): the global `putio` table a plugin
//! calls into. Described in JSON terms so 029's WebAssembly host can offer the same calls.
//!
//! Every log line and error a plugin produces has the plugin's secret values replaced by
//! `********`. HTTP errors name only the host and what failed, never the URL.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashSet};
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

fn log_table(lua: &Lua, host: &Host) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for level in ["debug", "info", "warn", "error"] {
        let name = host.name.clone();
        let secrets = host.secrets.clone();
        let logs = host.logs.clone();
        let f = lua.create_function(move |_, (message, fields): (LuaValue, Option<Table>)| {
            let mut text = message
                .to_string()
                .unwrap_or_else(|_| message.type_name().to_string());
            if let Some(fields) = fields {
                let mut pairs = BTreeMap::new();
                for pair in fields.pairs::<String, LuaValue>() {
                    let (k, v) = pair?;
                    let v = v.to_string().unwrap_or_else(|_| v.type_name().to_string());
                    pairs.insert(k, v);
                }
                for (k, v) in pairs {
                    text.push_str(&format!(" {}={}", k, v));
                }
            }
            let mut text = scrub(&text, &secrets);
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
            if logs.len() < MAX_LOG_LINES {
                logs.push(LogLine {
                    level: level.to_string(),
                    message: text,
                });
            }
            Ok(())
        })?;
        table.set(level, f)?;
    }
    Ok(table)
}

#[derive(Deserialize)]
struct RequestOptions {
    method: Option<String>,
    url: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    body: Option<String>,
    timeout: Option<f64>,
}

struct Response {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
    truncated: bool,
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
            return fail(lua, "time limit reached".to_string());
        }
        let host = url_host(&opts.url).unwrap_or_default();
        let limit = wanted.min(left);
        let result = http.handle.block_on(async {
            tokio::time::timeout(limit, fetch(&http.client, &http.allowed, opts)).await
        });
        let resp = match result {
            Err(_) => Err(format!("{}: timed out", host)),
            Ok(r) => r,
        };
        match resp {
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
            let name_ok = !name.is_empty()
                && name.len() <= 40
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if !name_ok {
                return Err(mlua::Error::runtime(
                    "metrics.count: name may only use a-z, 0-9 and _ (1 to 40)",
                ));
            }
            let n = n.unwrap_or(1.0);
            if !(n.is_finite() && n >= 0.0 && n.fract() == 0.0) {
                return Err(mlua::Error::runtime(
                    "metrics.count: n must be a whole number, 0 or more",
                ));
            }
            {
                let mut used = counters.lock().unwrap();
                if !used.contains(&name) {
                    if used.len() >= MAX_COUNTERS {
                        return Err(mlua::Error::runtime(format!(
                            "metrics.count: at most {} names per plugin",
                            MAX_COUNTERS
                        )));
                    }
                    used.insert(name.clone());
                }
            }
            putio_sync_telemetry::plugins::record_counter(&plugin, &name, n as u64);
            Ok(())
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
            let s = seconds.unwrap_or_else(|| clock());
            let millis = (s * 1000.0) as i64;
            let at = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis)
                .ok_or_else(|| mlua::Error::runtime("time out of range"))?;
            Ok(at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
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
        let host = Host {
            name: "t".to_string(),
            version: "1.2".to_string(),
            config: serde_json::json!({"topic": "nas", "token": "tk_SECRET"})
                .as_object()
                .unwrap()
                .clone(),
            secrets: Rc::new(vec!["tk_SECRET".to_string(), "abc".to_string()]),
            http: http.map(|hosts| Http {
                allowed: hosts.into_iter().map(String::from).collect(),
                client: reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap(),
                handle: rt.as_ref().unwrap().handle().clone(),
            }),
            store: store.then(|| Arc::new(Mutex::new(crate::store::Store::open(None, "t")))),
            download_dir: "/downloads".to_string(),
            counters: Counters::default(),
            deadline: deadline.clone(),
            logs: logs.clone(),
            clock: Arc::new(|| 1_793_784_600.5),
        };
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
}
