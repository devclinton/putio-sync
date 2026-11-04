# Research: Lua Plugins

## R1 Which Lua, which binding

- **Decision**: `mlua` 0.11, features `lua54`, `vendored`, `serialize`.
- **Rationale**: a spike (2026-11-04, scratchpad) confirmed on macOS: `Lua::new_with`
  with `STRING | TABLE | MATH | UTF8 | COROUTINE` leaves `os`, `io`, `require` and
  `package` out; `set_memory_limit` turns a runaway allocation into a catchable "not
  enough memory" error; `set_hook(every_nth_instruction(1000))` returning an error stops
  `while true do end` within the deadline (300 ms target, 299.7 ms measured); `to_value`
  turns event JSON into tables. `vendored` builds Lua from C with `cc`; `ring` already needs
  a C compiler on every target in the workflows (Alpine `rust:alpine` has gcc, Snap and
  Debian runners have build-essential, Debian arm64 uses `aarch64-linux-gnu-gcc`, which `cc`
  picks for that target, Windows has MSVC), so no workflow changes.
- **Alternatives**: Luau (built-in sandboxing, but C++ build, `set_interrupt` instead of
  instruction hooks, a less familiar dialect); LuaJIT (no arm64 memory-limit parity,
  FFI to disable); `rlua` (merged into mlua).

## R2 Sandbox

- **Decision**: safe libraries only (R1). After creation: remove `dofile`, `loadfile`,
  `collectgarbage` and `string.dump`; replace `load` with a wrapper that always passes mode
  `"t"` (text only; the spike showed binary chunks are otherwise accepted by the parser);
  replace `print` with `putio.log.info`. No `debug`, so `debug.sethook` cannot undo the
  limit. Each plugin has its own `Lua`, so globals and the string metatable are not shared.
- **Note**: `pcall` can catch the time-limit error, but the hook fires again 1 000
  instructions later outside it, so a loop around `pcall` still ends; the worker also checks
  the deadline after the call returns and records a timeout either way.

## R3 Threads and the deadline

- **Decision**: one `std::thread` per enabled plugin owning its `Lua`. Jobs (event, tick,
  init, test) come from a `Mutex<VecDeque>` plus `Condvar` (1 000, drop oldest; tests go to
  the front). Each job sets `deadline = now + timeout`; the instruction hook errors past it;
  host HTTP calls run `handle.block_on(tokio::time::timeout(remaining, request))` with the
  `tokio::runtime::Handle` captured at `start`. A memory error, a timeout or a reload marks
  the state stale; the worker rebuilds it (new `Lua`, run the entry, call `init`) before the
  next job.
- **Rationale**: a looping script must never occupy a Tokio worker; `block_on` from a plain
  thread is allowed; one thread per plugin keeps plugins independent.
- **Alternatives**: mlua `async` + `send` (scripts still run on Tokio workers between
  awaits; an infinite loop without host calls would block one); `reqwest::blocking`
  (a second HTTP stack and runtime).

## R4 Manifest and needed hosts

- **Decision**: `plugin.toml` (see contracts/manifest.md). The hosts a plugin needs are
  `permissions.http.hosts` plus the host of each `permissions.http.host_from_config` field's
  current value (config or secret). A request is allowed when its host (lowercase, no port)
  is both needed and in `approved_hosts`. A Settings save sets `approved_hosts` to the
  needed hosts of every enabled plugin in the save (the form shows them before saving).
- **Rationale**: self-hosted ntfy and Discord webhook URLs come from config, so a static list
  is not enough; tying approval to what the user saw keeps "approved" meaningful.

## R5 Secrets

- **Decision**: `[[plugins]]` keeps `config` and `secrets` as separate maps; a field with
  `secret = true` must be in `secrets`. Core masks every `secrets` value as `********` in the
  settings view and the export; a save or import sending the mask keeps the stored value for
  the same plugin and key, and an import with no stored value is refused (as hooks). The
  worker scrubs every secret value of 4+ characters from plugin log lines, run errors and
  test output. HTTP errors are built as `"<host>: <kind>"` or `"<host>: HTTP <status>"`.
- **Rationale**: core can mask without the manifest; Discord webhook URLs are secrets in the
  URL itself.

## R6 Store

- **Decision**: `<config dir>/plugin-data/<name>.json`, a JSON object loaded on first use
  and written whole (temp file + rename) after each `set`/`delete`. Limits: key ≤ 128 bytes,
  value ≤ 64 KiB as JSON, file ≤ 1 MiB. With no config path (tests, `server` without a
  config file) the store is in memory.
- **Alternatives**: a SQLite table (a migration, and the plugins crate would depend on db for
  a few KiB of state).

## R7 Ticks and grouping

- **Decision**: one Tokio interval (60 s) queues `tick` for each running plugin whose module
  has `on_tick`, unless a tick is already queued. The bundled notifiers group per event type:
  the first event sends at once and opens a window (`group_minutes`, default 5); later events
  in the window are counted (first 5 names kept); a tick after the window, or the next event
  after it, sends one summary. Notifiers read time through `putio.time.now()`, which the
  tests can fix per call through a test hook on the service.

## R8 Event and test delivery

- **Decision**: the event table is `serde_json::to_value(&event)` converted with nulls as
  `nil`. `plugin.test` (new payload `{plugin, message}`) reaches a plugin only through
  `POST /plugins/test`; manifest `"*"` excludes it, and hooks' `*` now excludes it too.
- **Rationale**: test events must not reach every consumer.

## R9 Reload

- **Decision**: `reload()` rediscovers bundled and folder plugins; slots are matched by name
  (queue, history and store kept), new ones get a worker, gone ones stop. A slot's
  generation number goes up, so its worker rebuilds the state before the next job.
  `configure(settings)` swaps settings the same way. Both run on a settings save; only
  `reload()` reads disk.

## R10 The synchronous extension points

- **Decision**: not in 028 (spec clarification). The sync path (`filter`, `rules`,
  `path_utils`) is synchronous code in core used by CLI one-shots too; adding plugins there
  needs a blocking call into a worker with a millisecond limit, a defined fallback per rule,
  and UI to attach scripts to rules. The worker built here can take a synchronous job type
  later.
