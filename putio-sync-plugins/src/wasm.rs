//! WebAssembly plugins (029, research R1–R3): a module run by the wasmi interpreter, behind
//! the same manifest, jobs, hooks and limits as a Lua script.
//!
//! The ABI is contracts/wasm-abi.md: JSON text in linear memory, one host import
//! (`putio.call`) for the whole host API, and a small WASI preview1 shim with no real access
//! so ordinary Go, TinyGo and Rust builds load. Time is measured with fuel slices: the
//! module runs a slice, the deadline is checked, and it runs on or stops.

use std::collections::hash_map::RandomState;
use std::collections::BTreeMap;
use std::hash::{BuildHasher, Hasher};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use wasmi::{
    Caller, CompilationMode, Config, Engine, Error, ExternType, Func, FuncType, Linker, Memory,
    Module, ResumableCall, Store, Val, ValType,
};
use wasmi_core::{LimiterError, ResourceLimiter};

use crate::host::{self, Host};
use crate::sandbox::TIME_LIMIT;

/// The first four bytes of every module
pub const MAGIC: &[u8] = b"\0asm";
/// The custom section that carries a single-file plugin's `plugin.toml`
pub const SECTION: &str = "putio-plugin";
/// Largest JSON a plugin function may return
pub const MAX_RESULT_BYTES: usize = 1024 * 1024;
/// Largest host call request (a 1 MiB `fs.write` and its JSON)
const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024 + 1024;
/// The error text when the memory limit is hit
pub const MEMORY_LIMIT: &str = "memory limit reached";
/// Fuel per slice; the deadline is checked between slices and on each host call
const FUEL_SLICE: u64 = 4_000_000;
const WASI: &str = "wasi_snapshot_preview1";
/// The WASI calls the shim answers; any other WASI import answers `ENOSYS`
const WASI_CALLS: [&str; 13] = [
    "fd_write",
    "clock_time_get",
    "clock_res_get",
    "random_get",
    "args_sizes_get",
    "args_get",
    "environ_sizes_get",
    "environ_get",
    "proc_exit",
    "sched_yield",
    "poll_oneoff",
    "fd_prestat_get",
    "fd_prestat_dir_name",
];
const ENOSYS: i32 = 52;
const EBADF: i32 = 8;
const EINVAL: i32 = 28;
const EFAULT: i32 = 21;

/// One engine for every plugin, with fuel on. Functions are compiled when the module loads:
/// lazy compilation charges fuel on first call, and running out there cannot be resumed.
fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        let mut config = Config::default();
        config.consume_fuel(true);
        config.compilation_mode(CompilationMode::Eager);
        Engine::new(&config)
    })
}

/// The text of `bytes`' [`SECTION`] custom section, if it has one
pub fn manifest_section(bytes: &[u8]) -> Result<Option<String>, String> {
    if bytes.len() < 8 || &bytes[..4] != MAGIC {
        return Err("not a WebAssembly module".to_string());
    }
    let mut pos = 8;
    while pos < bytes.len() {
        let id = bytes[pos];
        pos += 1;
        let size = leb(bytes, &mut pos).ok_or("truncated module")? as usize;
        let end = pos.checked_add(size).filter(|e| *e <= bytes.len());
        let end = end.ok_or("truncated module")?;
        if id == 0 {
            let mut at = pos;
            let len = leb(bytes, &mut at).ok_or("truncated module")? as usize;
            let name_end = at.checked_add(len).filter(|e| *e <= end);
            let name_end = name_end.ok_or("truncated module")?;
            if &bytes[at..name_end] == SECTION.as_bytes() {
                let text = std::str::from_utf8(&bytes[name_end..end])
                    .map_err(|_| format!("the {} section is not UTF-8", SECTION))?;
                return Ok(Some(text.to_string()));
            }
        }
        pos = end;
    }
    Ok(None)
}

/// An unsigned LEB128 number at `pos`
fn leb(bytes: &[u8], pos: &mut usize) -> Option<u32> {
    let mut value: u32 = 0;
    for shift in (0..35).step_by(7) {
        let byte = *bytes.get(*pos)?;
        *pos += 1;
        value |= u32::from(byte & 0x7f).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// Why a call into a module ended badly
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// It ran past the deadline
    Timeout,
    /// It wanted more memory than the plugin's `memory_mb`
    Memory,
    /// It trapped (a Rust panic, an out-of-bounds access, `proc_exit`); the instance is
    /// thrown away
    Trap(String),
    /// It failed through `putio.fail`, or what it returned is not usable
    Error(String),
}

impl Failure {
    /// Whether the instance must be built again before the next job
    pub fn rebuild(&self) -> bool {
        !matches!(self, Failure::Error(_))
    }
}

struct Limiter {
    max: usize,
    hit: bool,
}

impl ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool, LimiterError> {
        if desired > self.max {
            self.hit = true;
            return Ok(false);
        }
        Ok(maximum.is_none_or(|m| desired <= m))
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool, LimiterError> {
        // A table entry is a few bytes; the same budget keeps a table from eating memory
        if desired.saturating_mul(16) > self.max {
            self.hit = true;
            return Ok(false);
        }
        Ok(maximum.is_none_or(|m| desired <= m))
    }

    fn instances(&self) -> usize {
        1
    }

    fn tables(&self) -> usize {
        4
    }

    fn memories(&self) -> usize {
        1
    }
}

/// What the host functions reach through [`Caller`]
struct Data {
    host: Host,
    limiter: Limiter,
    memory: Option<Memory>,
    alloc: Option<Func>,
    /// The message given to `putio.fail` in this call
    failed: Option<String>,
    /// The last `putio.call` reply, until `putio.reply` copies it out
    reply: Vec<u8>,
    /// Unfinished stdout and stderr lines
    lines: [Vec<u8>; 2],
}

/// A built module: its store, instance and functions
pub struct Instance {
    store: Store<Data>,
    instance: wasmi::Instance,
    memory_mb: u64,
}

impl Instance {
    /// Compile and instantiate `bytes` for `host`, then run `_initialize` if the module has
    /// it. The deadline is the one in `host`.
    pub fn new(bytes: &[u8], host: Host, memory_mb: u64) -> Result<Instance, Failure> {
        let module = Module::new(engine(), bytes)
            .map_err(|e| Failure::Error(format!("not a valid WebAssembly module: {}", e)))?;
        let mut linker = Linker::<Data>::new(engine());
        define(&mut linker).map_err(|e| Failure::Error(e.to_string()))?;
        for import in module.imports() {
            let (m, n) = (import.module(), import.name());
            let known = match m {
                "putio" => matches!(n, "call" | "reply" | "fail"),
                WASI => {
                    if let (false, ExternType::Func(ty)) = (WASI_CALLS.contains(&n), import.ty()) {
                        stub(&mut linker, n, ty.clone())
                            .map_err(|e| Failure::Error(e.to_string()))?;
                    }
                    matches!(import.ty(), ExternType::Func(_))
                }
                _ => false,
            };
            if !known {
                return Err(Failure::Error(format!(
                    "imports \"{}.{}\", which the host does not offer",
                    m, n
                )));
            }
        }
        if module.exports().any(|e| e.name() == "_start") {
            return Err(Failure::Error(
                "exports _start: build the plugin as a library (a WASI reactor), not a command"
                    .to_string(),
            ));
        }
        let data = Data {
            host,
            limiter: Limiter {
                max: memory_mb as usize * 1024 * 1024,
                hit: false,
            },
            memory: None,
            alloc: None,
            failed: None,
            reply: Vec::new(),
            lines: [Vec::new(), Vec::new()],
        };
        let mut store = Store::new(engine(), data);
        store.limiter(|d| &mut d.limiter);
        let mut built = Instance {
            instance: match linker.instantiate_and_start(&mut store, &module) {
                Ok(i) => i,
                Err(e) if store.data().limiter.hit => {
                    let _ = e;
                    return Err(Failure::Memory);
                }
                Err(e) => return Err(Failure::Error(e.to_string())),
            },
            store,
            memory_mb,
        };
        let memory = built.instance.get_memory(&built.store, "memory");
        let memory = memory.ok_or_else(|| Failure::Error("exports no memory".to_string()))?;
        let alloc = built.instance.get_func(&built.store, "putio_alloc");
        let alloc = alloc.ok_or_else(|| Failure::Error("exports no putio_alloc".to_string()))?;
        let want = FuncType::new([ValType::I32], [ValType::I32]);
        if alloc.ty(&built.store) != want {
            return Err(Failure::Error(
                "putio_alloc must take an i32 and return an i32".to_string(),
            ));
        }
        built.store.data_mut().memory = Some(memory);
        built.store.data_mut().alloc = Some(alloc);
        if let Some(init) = built.instance.get_func(&built.store, "_initialize") {
            built.run(&init, &[], &mut [])?;
        }
        Ok(built)
    }

    /// The size of the module's linear memory (055)
    pub fn memory_bytes(&self) -> u64 {
        let memory = self.store.data().memory;
        memory.map_or(0, |m| m.data(&self.store).len() as u64)
    }

    /// Whether the module exports a function `name`
    pub fn has(&self, name: &str) -> bool {
        self.instance.get_func(&self.store, name).is_some()
    }

    /// Call `name` with `args` as a JSON array: what it returned, `None` for nothing (Lua
    /// `nil`). The caller checks [`Instance::has`] first.
    pub fn call(&mut self, name: &str, args: &[Value]) -> Result<Option<Value>, Failure> {
        self.store.data_mut().failed = None;
        let func = self.instance.get_func(&self.store, name);
        let func = func.ok_or_else(|| Failure::Error(format!("{} is not exported", name)))?;
        let want = FuncType::new([ValType::I32, ValType::I32], [ValType::I64]);
        if func.ty(&self.store) != want {
            return Err(Failure::Error(format!(
                "{} must take (i32, i32) and return i64",
                name
            )));
        }
        let input = serde_json::to_vec(args).map_err(|e| Failure::Error(e.to_string()))?;
        let ptr = self.alloc(&input)?;
        let mut out = [Val::I64(0)];
        self.run(
            &func,
            &[Val::I32(ptr), Val::I32(input.len() as i32)],
            &mut out,
        )?;
        self.flush();
        let packed = out[0].i64().unwrap_or(0) as u64;
        if packed == 0 {
            return Ok(None);
        }
        let (at, len) = ((packed >> 32) as usize, (packed & 0xffff_ffff) as usize);
        if len > MAX_RESULT_BYTES {
            return Err(Failure::Error(format!(
                "{} returned more than {} KiB",
                name,
                MAX_RESULT_BYTES / 1024
            )));
        }
        let memory = self.store.data().memory.expect("set when built");
        let mut text = vec![0; len];
        memory
            .read(&self.store, at, &mut text)
            .map_err(|_| Failure::Error(format!("{} returned a value outside its memory", name)))?;
        if let Some(free) = self.instance.get_func(&self.store, "putio_free") {
            self.run(&free, &[Val::I32(at as i32), Val::I32(len as i32)], &mut [])?;
        }
        serde_json::from_slice(&text)
            .map(Some)
            .map_err(|e| Failure::Error(format!("{} returned text that is not JSON: {}", name, e)))
    }

    /// The error text for `failure`, as the run records it
    pub fn describe(&self, failure: &Failure, timeout: std::time::Duration) -> String {
        match failure {
            Failure::Timeout => format!("{} ({} s)", TIME_LIMIT, timeout.as_secs()),
            Failure::Memory => format!("{} ({} MiB)", MEMORY_LIMIT, self.memory_mb),
            Failure::Trap(text) | Failure::Error(text) => text.clone(),
        }
    }

    /// `bytes` copied into memory from `putio_alloc`
    fn alloc(&mut self, bytes: &[u8]) -> Result<i32, Failure> {
        let alloc = self.store.data().alloc.expect("set when built");
        let mut out = [Val::I32(0)];
        self.run(&alloc, &[Val::I32(bytes.len() as i32)], &mut out)?;
        let ptr = out[0].i32().unwrap_or(0);
        let memory = self.store.data().memory.expect("set when built");
        memory
            .write(&mut self.store, ptr as u32 as usize, bytes)
            .map_err(|_| Failure::Trap("putio_alloc returned memory out of bounds".to_string()))?;
        Ok(ptr)
    }

    /// Run `func` in fuel slices until it returns, fails or passes the deadline
    fn run(&mut self, func: &Func, inputs: &[Val], outputs: &mut [Val]) -> Result<(), Failure> {
        let _ = self.store.set_fuel(FUEL_SLICE);
        let mut call = func.call_resumable(&mut self.store, inputs, outputs);
        loop {
            match call {
                Ok(ResumableCall::Finished) => return Ok(()),
                Ok(ResumableCall::OutOfFuel(more)) => {
                    if past(&self.store.data().host) {
                        return Err(Failure::Timeout);
                    }
                    let _ = self.store.set_fuel(FUEL_SLICE);
                    call = more.resume(&mut self.store, outputs);
                }
                Ok(ResumableCall::HostTrap(trap)) => {
                    return Err(self.failure(trap.into_host_error()))
                }
                Err(e) => return Err(self.failure(e)),
            }
        }
    }

    fn failure(&mut self, e: Error) -> Failure {
        self.flush();
        let data = self.store.data_mut();
        if data.limiter.hit {
            return Failure::Memory;
        }
        if let Some(message) = data.failed.take() {
            return Failure::Error(message);
        }
        if let Some(code) = e.i32_exit_status() {
            return Failure::Trap(format!("exited with code {}", code));
        }
        let text = e.to_string();
        if text.contains(TIME_LIMIT) || past(&data.host) {
            return Failure::Timeout;
        }
        Failure::Trap(format!("trapped: {}", text))
    }

    /// Log what is left of the stdout and stderr lines
    fn flush(&mut self) {
        let data = self.store.data_mut();
        for fd in 0..2 {
            let rest = std::mem::take(&mut data.lines[fd]);
            if !rest.is_empty() {
                print_line(&data.host, fd, &rest);
            }
        }
    }
}

/// Whether the current call's deadline has passed
fn past(host: &Host) -> bool {
    host.deadline.get().is_some_and(|d| Instant::now() >= d)
}

fn print_line(host: &Host, fd: usize, line: &[u8]) {
    let level = if fd == 0 { "info" } else { "warn" };
    let text = String::from_utf8_lossy(line)
        .trim_end_matches('\r')
        .to_string();
    host::log(
        &host.name,
        &host.secrets,
        &host.logs,
        level,
        text,
        BTreeMap::new(),
    );
}

fn memory_of(caller: &Caller<'_, Data>) -> Result<Memory, Error> {
    caller
        .data()
        .memory
        .ok_or_else(|| Error::new("memory not ready"))
}

fn read(caller: &Caller<'_, Data>, ptr: i32, len: i32, max: usize) -> Result<Vec<u8>, Error> {
    let len = len as u32 as usize;
    if len > max {
        return Err(Error::new(format!(
            "host call larger than {} KiB",
            max / 1024
        )));
    }
    let mut buf = vec![0; len];
    memory_of(caller)?
        .read(caller, ptr as u32 as usize, &mut buf)
        .map_err(|_| Error::new("host call outside the module's memory"))?;
    Ok(buf)
}

fn write(caller: &mut Caller<'_, Data>, ptr: i32, bytes: &[u8]) -> bool {
    match memory_of(caller) {
        Ok(memory) => memory.write(caller, ptr as u32 as usize, bytes).is_ok(),
        Err(_) => false,
    }
}

/// `putio.call`: one host API call. The reply stays with the host and its length is
/// returned; `putio.reply` copies it into memory the plugin allocated. No plugin code runs
/// inside a host call, so fuel slices and the deadline cover all of it.
fn host_call(mut caller: Caller<'_, Data>, ptr: i32, len: i32) -> Result<i32, Error> {
    if past(&caller.data().host) {
        return Err(Error::new(TIME_LIMIT));
    }
    let request = read(&caller, ptr, len, MAX_REQUEST_BYTES)?;
    let reply = match serde_json::from_slice::<Value>(&request) {
        Err(e) => serde_json::json!({"error": format!("host call is not JSON: {}", e)}),
        Ok(request) => {
            let function = request
                .get("fn")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = match request.get("args") {
                Some(Value::Array(a)) => a.clone(),
                None | Some(Value::Null) => Vec::new(),
                Some(other) => vec![other.clone()],
            };
            match host::call_json(&caller.data().host, function, &args) {
                Ok(value) => serde_json::json!({"ok": value}),
                Err(e) => serde_json::json!({"error": e}),
            }
        }
    };
    if past(&caller.data().host) {
        return Err(Error::new(TIME_LIMIT));
    }
    let bytes = serde_json::to_vec(&reply).map_err(|e| Error::new(e.to_string()))?;
    let len = bytes.len() as i32;
    caller.data_mut().reply = bytes;
    Ok(len)
}

/// `putio.reply`: copy the last reply to `ptr`, which has room for the length `call` gave
fn host_reply(mut caller: Caller<'_, Data>, ptr: i32) -> Result<(), Error> {
    let bytes = std::mem::take(&mut caller.data_mut().reply);
    if !write(&mut caller, ptr, &bytes) {
        return Err(Error::new("putio.reply: memory out of bounds"));
    }
    Ok(())
}

/// `putio.fail`: end the call with the plugin's message
fn host_fail(mut caller: Caller<'_, Data>, ptr: i32, len: i32) -> Result<(), Error> {
    let text = read(&caller, ptr, len, 64 * 1024)?;
    let host = &caller.data().host;
    let message = host::scrub(&String::from_utf8_lossy(&text), &host.secrets);
    caller.data_mut().failed = Some(message.clone());
    Err(Error::new(message))
}

/// The host functions every module may import
fn define(linker: &mut Linker<Data>) -> Result<(), wasmi::errors::LinkerError> {
    linker.func_wrap("putio", "call", host_call)?;
    linker.func_wrap("putio", "reply", host_reply)?;
    linker.func_wrap("putio", "fail", host_fail)?;
    linker.func_wrap(
        WASI,
        "fd_write",
        |mut caller: Caller<'_, Data>, fd: i32, iovs: i32, count: i32, written: i32| -> i32 {
            if fd != 1 && fd != 2 {
                return EBADF;
            }
            let mut total = 0u32;
            for i in 0..count.max(0) {
                let Ok(iov) = read(&caller, iovs + i * 8, 8, 8) else {
                    return EFAULT;
                };
                let at = i32::from_le_bytes([iov[0], iov[1], iov[2], iov[3]]);
                let len = i32::from_le_bytes([iov[4], iov[5], iov[6], iov[7]]);
                let Ok(bytes) = read(&caller, at, len, 64 * 1024) else {
                    return EFAULT;
                };
                total += bytes.len() as u32;
                let index = fd as usize - 1;
                let data = caller.data_mut();
                for &b in &bytes {
                    if b == b'\n' {
                        let line = std::mem::take(&mut data.lines[index]);
                        print_line(&data.host, index, &line);
                    } else if data.lines[index].len() < 4096 {
                        data.lines[index].push(b);
                    }
                }
            }
            if write(&mut caller, written, &total.to_le_bytes()) {
                0
            } else {
                EFAULT
            }
        },
    )?;
    linker.func_wrap(
        WASI,
        "clock_time_get",
        |mut caller: Caller<'_, Data>, id: i32, _precision: i64, out: i32| -> i32 {
            let nanos = match id {
                0 => SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0),
                1..=3 => started().elapsed().as_nanos() as u64,
                _ => return EINVAL,
            };
            if write(&mut caller, out, &nanos.to_le_bytes()) {
                0
            } else {
                EFAULT
            }
        },
    )?;
    linker.func_wrap(
        WASI,
        "clock_res_get",
        |mut caller: Caller<'_, Data>, id: i32, out: i32| -> i32 {
            if !(0..=3).contains(&id) {
                return EINVAL;
            }
            if write(&mut caller, out, &1000u64.to_le_bytes()) {
                0
            } else {
                EFAULT
            }
        },
    )?;
    linker.func_wrap(
        WASI,
        "random_get",
        |mut caller: Caller<'_, Data>, at: i32, len: i32| -> i32 {
            let len = len.clamp(0, 64 * 1024) as usize;
            let mut bytes = Vec::with_capacity(len + 8);
            while bytes.len() < len {
                let mut h = RandomState::new().build_hasher();
                h.write_usize(bytes.len());
                bytes.extend_from_slice(&h.finish().to_le_bytes());
            }
            bytes.truncate(len);
            if write(&mut caller, at, &bytes) {
                0
            } else {
                EFAULT
            }
        },
    )?;
    for name in ["args_sizes_get", "environ_sizes_get"] {
        linker.func_wrap(
            WASI,
            name,
            |mut caller: Caller<'_, Data>, count: i32, size: i32| -> i32 {
                let ok = write(&mut caller, count, &0u32.to_le_bytes())
                    && write(&mut caller, size, &0u32.to_le_bytes());
                if ok {
                    0
                } else {
                    EFAULT
                }
            },
        )?;
    }
    for name in ["args_get", "environ_get"] {
        linker.func_wrap(WASI, name, |_: Caller<'_, Data>, _: i32, _: i32| -> i32 {
            0
        })?;
    }
    linker.func_wrap(
        WASI,
        "proc_exit",
        |_: Caller<'_, Data>, code: i32| -> Result<(), Error> { Err(Error::i32_exit(code)) },
    )?;
    linker.func_wrap(WASI, "sched_yield", |_: Caller<'_, Data>| -> i32 { 0 })?;
    linker.func_wrap(
        WASI,
        "poll_oneoff",
        |mut caller: Caller<'_, Data>, subs: i32, events: i32, count: i32, out: i32| -> i32 {
            // Every subscription fires at once: a sleep returns straight away
            let count = count.clamp(0, 64);
            for i in 0..count {
                let Ok(sub) = read(&caller, subs + i * 48, 48, 48) else {
                    return EFAULT;
                };
                let mut event = [0u8; 32];
                event[..8].copy_from_slice(&sub[..8]);
                event[10] = sub[8];
                if !write(&mut caller, events + i * 32, &event) {
                    return EFAULT;
                }
            }
            if write(&mut caller, out, &(count as u32).to_le_bytes()) {
                0
            } else {
                EFAULT
            }
        },
    )?;
    linker.func_wrap(
        WASI,
        "fd_prestat_get",
        |_: Caller<'_, Data>, _: i32, _: i32| -> i32 { EBADF },
    )?;
    linker.func_wrap(
        WASI,
        "fd_prestat_dir_name",
        |_: Caller<'_, Data>, _: i32, _: i32, _: i32| -> i32 { EBADF },
    )?;
    Ok(())
}

/// A WASI call the shim does not offer: `ENOSYS`, or a trap when it returns no errno
fn stub(
    linker: &mut Linker<Data>,
    name: &str,
    ty: FuncType,
) -> Result<(), wasmi::errors::LinkerError> {
    let errno = ty.results() == [ValType::I32];
    let what = format!("{}.{} is not offered", WASI, name);
    linker.func_new(WASI, name, ty, move |_, _, results| {
        if errno {
            results[0] = Val::I32(ENOSYS);
            Ok(())
        } else {
            Err(Error::new(what.clone()))
        }
    })?;
    Ok(())
}

/// When the process started, for the monotonic clock
fn started() -> Instant {
    static STARTED: OnceLock<Instant> = OnceLock::new();
    *STARTED.get_or_init(Instant::now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_section_is_found() {
        let with = wat::parse_str(r#"(module (@custom "putio-plugin" "name = \"x\""))"#).unwrap();
        assert_eq!(
            manifest_section(&with).unwrap().as_deref(),
            Some("name = \"x\"")
        );
        let without = wat::parse_str("(module (memory 1))").unwrap();
        assert_eq!(manifest_section(&without).unwrap(), None);
        assert!(manifest_section(b"PK\x03\x04 not wasm").is_err());
        // A section length past the end of the file
        let mut cut = with.clone();
        cut.truncate(cut.len() - 3);
        assert!(manifest_section(&cut).is_err());
    }
}
