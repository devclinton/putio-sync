# Contract: WebAssembly plugin ABI, version 1

A WebAssembly plugin is a core module (WebAssembly 1.0 plus the usual post-MVP features:
bulk memory, sign extension, multi-value, mutable globals, reference types). Values cross the
boundary as UTF-8 JSON text in the module's linear memory. There is one host import for the
whole host API, so a binding for a new language is a few lines.

## Module exports

| Export | Type | Required | Meaning |
|---|---|---|---|
| `memory` | memory | yes | The linear memory the host reads and writes |
| `putio_alloc` | `(size: i32) -> i32` | yes | Room for `size` bytes; the host writes arguments and host-call replies there |
| `putio_free` | `(ptr: i32, size: i32)` | no | Called by the host on a result it has read |
| `_initialize` | `()` | no | WASI reactor start (Go `-buildmode=c-shared`, TinyGo); run once per instance |
| `_start` | `()` | no | Refused: a plugin is a library, not a command |
| `init`, `on_event`, `on_tick`, `filter`, `rename`, `folder`, `on_action`, `health` | `(ptr: i32, len: i32) -> i64` | as for Lua | The plugin's functions |

A plugin function receives a JSON **array** of its arguments, the same values a Lua function
gets: `on_event([event])`, `on_tick([])`, `filter([file])`, `on_action([id, input])`,
`health([])`, `init([])`. The memory at `ptr` was given by `putio_alloc` and is the
plugin's to free.

It returns `0` for no value (Lua `nil`), or `(result_ptr << 32) | result_len` pointing at
JSON text: what the Lua function would return. A function that is not exported is "not
defined", exactly like a missing Lua function. The manifest's checks are the same: events
need `on_event`; each hook needs its function.

## Host import: module `putio`

| Import | Type | Meaning |
|---|---|---|
| `call` | `(ptr: i32, len: i32) -> i32` | One host API call; returns the reply's length |
| `reply` | `(ptr: i32)` | Copy the last reply to `ptr` (room for the length `call` gave) |
| `fail` | `(ptr: i32, len: i32)` | Fail the current call with this message (Lua `error(msg)`) |

`call` takes `{"fn": "<name>", "args": [...]}`. The host keeps the reply and returns its
length; the plugin allocates that much and calls `reply` to copy it out. No plugin code runs
inside a host call (the host never calls `putio_alloc` re-entrantly), so the time limit's
fuel slices cover everything. The reply is:

- `{"ok": <value>}` on success (`null` when the call returns nothing)
- `{"error": "<text>"}` on failure, secrets replaced

| `fn` | `args` | `ok` | Lua equivalent |
|---|---|---|---|
| `log.debug` / `log.info` / `log.warn` / `log.error` | `[message, fields?]` | `null` | `putio.log.*` |
| `config` | `[]` | object | `putio.config` |
| `plugin` | `[]` | `{name, version}` | `putio.plugin` |
| `version` / `api_version` / `download_dir` | `[]` | string / 1 / string | same |
| `http.request` | `[{method?, url, headers?, body?, timeout?}]` | `{status, headers, body, truncated}` | `putio.http.request` |
| `store.get` / `store.set` / `store.delete` / `store.keys` | `[key]` / `[key, value]` / `[key]` / `[]` | value / true / true / list | `putio.store.*` |
| `time.now` / `time.iso` | `[]` / `[seconds?]` | number / string | `putio.time.*` |
| `json.encode` / `json.decode` | `[value]` / `[text]` | string / value | `putio.json.*` |
| `path.map` / `path.parent` / `path.within` | `[path, rules]` / `[path]` / `[path, folder]` | string / string / bool | `putio.path.*` |
| `metrics.count` | `[name, n?]` | `null` | `putio.metrics.count` |
| `fs.read` / `fs.write` / `fs.list` / `fs.exists` | `[path]` / `[path, text]` / `[dir]` / `[path]` | string / true / list of `{name, dir, size}` / bool | `putio.fs.*` (new, both runtimes) |

`http.*`, `store.*` and `fs.*` answer `{"error": ...}` when the manifest does not ask for
that permission. HTTP response bodies are text (invalid UTF-8 replaced). An unknown `fn`
answers `{"error": "unknown host call \"x\""}`.

## WASI (`wasi_snapshot_preview1`)

Offered so ordinary Go, TinyGo and Rust `wasm32-wasip1` builds load. No real access:

| Call | Behaviour |
|---|---|
| `fd_write` to 1 or 2 | Each line becomes an `info` (stdout) or `warn` (stderr) log line |
| `clock_time_get`, `clock_res_get` | Real clocks |
| `random_get` | Random bytes |
| `args_*`, `environ_*` | Empty |
| `proc_exit` | Fails the call: `exited with code N` |
| `sched_yield` | No-op |
| `poll_oneoff` | Returns at once (sleeps do not sleep) |
| everything else | `ENOSYS` (52) |

Any other import module makes the plugin Broken: `imports "<module>.<name>", which the host
does not offer`.

## Limits

| Limit | Value |
|---|---|
| Module size | 16 MiB |
| Memory | the plugin's `memory_mb` (4–256, default 32), all memories and tables together |
| Time | `timeout_secs` per call, `hook_timeout_ms` for sync rules; measured with fuel slices checked against the deadline |
| Result | 1 MiB of JSON |

Going over the time limit ends the call with `time limit reached (N s)`; over memory with
`memory limit reached (N MiB)`. Either, or a trap, throws the instance away; the next job
instantiates the module again and calls `init`.
