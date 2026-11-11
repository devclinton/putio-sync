//! The putio-sync plugin ABI for Rust (PLUGINS.md, "WebAssembly plugins"): JSON in and out
//! through linear memory, one host import for the whole host API.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

#[link(wasm_import_module = "putio")]
extern "C" {
    #[link_name = "call"]
    fn host_call(ptr: *const u8, len: usize) -> usize;
    #[link_name = "reply"]
    fn host_reply(ptr: *mut u8);
    #[link_name = "fail"]
    fn host_fail(ptr: *const u8, len: usize);
}

/// The manifest, embedded so the `.wasm` alone can be installed
#[link_section = "putio-plugin"]
#[used]
static MANIFEST: [u8; include_bytes!("../plugin.toml").len()] = *include_bytes!("../plugin.toml");

#[no_mangle]
pub extern "C" fn putio_alloc(size: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(size.max(1));
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

#[no_mangle]
pub unsafe extern "C" fn putio_free(ptr: *mut u8, size: usize) {
    drop(Vec::from_raw_parts(ptr, 0, size.max(1)));
}

/// Read and free a buffer the host filled
unsafe fn take(ptr: *mut u8, len: usize) -> Vec<u8> {
    Vec::from_raw_parts(ptr, len, len.max(1))
}

/// A function's JSON argument array
pub fn args(ptr: *mut u8, len: usize) -> Vec<Value> {
    let bytes = unsafe { take(ptr, len) };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Hand a value back to the host; `None` is "no answer"
pub fn result<T: Serialize>(value: Option<T>) -> u64 {
    let Some(value) = value else { return 0 };
    let text = serde_json::to_vec(&value).unwrap_or_default();
    let len = text.len();
    let ptr = Box::leak(text.into_boxed_slice()).as_mut_ptr();
    ((ptr as u64) << 32) | len as u64
}

/// Fail the current call with `message`
pub fn fail(message: &str) {
    unsafe { host_fail(message.as_ptr(), message.len()) }
}

/// One host API call
pub fn call<T: DeserializeOwned>(function: &str, args: Value) -> Result<T, String> {
    let request = serde_json::to_vec(&json!({"fn": function, "args": args})).unwrap();
    let len = unsafe { host_call(request.as_ptr(), request.len()) };
    let mut reply = vec![0u8; len];
    unsafe { host_reply(reply.as_mut_ptr()) };
    let reply: Value = serde_json::from_slice(&reply).map_err(|e| e.to_string())?;
    if let Some(error) = reply.get("error").and_then(Value::as_str) {
        return Err(error.to_string());
    }
    serde_json::from_value(reply.get("ok").cloned().unwrap_or(Value::Null)).map_err(|e| e.to_string())
}

/// `putio.log.info(message, fields)`
pub fn info(message: &str, fields: Value) {
    let _ = call::<Value>("log.info", json!([message, fields]));
}
