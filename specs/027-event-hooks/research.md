# Research: Event Hooks

## R1 Where the bus lives and how it is reached

- **Decision**: `putio_sync_core::events`, a `broadcast::Sender<Arc<Event>>` (capacity
  4 096) in a `OnceLock`, with free functions `emit(Payload)` and `subscribe()`.
- **Rationale**: every emitter (download, storage) and consumer (hooks, later Lua and
  notifications) already depends on core. A global reaches the `server --download`
  thread, which builds its own client, database and `DownloadManager` on its own runtime;
  `broadcast` works across runtimes. `emit` with no subscriber is a no-op (`send` errors
  are ignored), so tests and CLI one-shots pay nothing.
- **Alternatives**: a sender passed through constructors (misses the legacy thread,
  touches every `DownloadManager::new` call); an `mpsc` per consumer (needs a registry,
  which is what `broadcast` is).

## R2 Lag and back-pressure

- **Decision**: the hooks dispatcher drains the broadcast receiver straight into per-hook
  `VecDeque`s (1 000 each, drop oldest). A `Lagged(n)` from the receiver is counted as `n`
  drops for every hook and logged.
- **Rationale**: the dispatcher does no I/O, so lag only happens under extreme bursts;
  per-hook queues keep a slow hook from affecting others (spec edge case).

## R3 Command execution

- **Decision**: `tokio::process::Command::new(program).args(args).env_clear()` plus
  `PATH` (the daemon's, else a standard default), `HOME`, `LANG`, `TZ` when set,
  `PUTIO_SYNC_EVENT_TYPE`, `PUTIO_SYNC_EVENT_ID`, `PUTIO_SYNC_HOOK_NAME` and the hook's
  `env`; stdin piped and written then closed (a `BrokenPipe` is ignored); stdout/stderr
  piped and read concurrently, keeping the first 4 KiB and counting the rest;
  `kill_on_drop(true)` and `tokio::time::timeout` around `wait`, so a timeout kills the
  child.
- **Rationale**: no shell avoids quoting bugs and injection from event fields; a clean
  environment keeps `PUTIO_SYNC_*` tokens and anything else in the daemon's environment
  out of hook code (Principle VI). Reading both pipes while waiting avoids a full-pipe
  deadlock.
- **Note**: a process that forks and leaves a grandchild holding the pipes: reads stop at
  the timeout too, since the read tasks run inside the same timeout.

## R4 Webhooks

- **Decision**: one `reqwest::Client` per service with `redirect::Policy::custom` (follow
  up to 3, only when the host and port match the original), `User-Agent:
  putio-sync/<version>`; each attempt has the hook's timeout; retries after 5 s and 30 s
  on connect errors, timeouts, 429 and 5xx. Errors are rebuilt from the status or the
  error kind and the redacted URL, never from `reqwest::Error`'s `Display`, which includes
  the full URL.
- **Rationale**: same-host redirects keep the `Authorization` header from going to
  another host; the `Display` of reqwest errors is the main leak path for query tokens.
- **Alternatives**: `Policy::none()` (breaks http→https redirects many services use).

## R5 Masking secrets in settings

- **Decision**: the `hooks` settings view replaces every header and env value with
  `********` and shows each URL cut down by `redact::url`. A save that sends `********`
  for a header or env key keeps the value the current config has for the same hook name
  and key, and a save that sends the cut-down form of the stored URL keeps that URL; any
  other value replaces it. The
  export masks the same way, and the import refuses a mask it cannot resolve.
- **Rationale**: the UI can edit hooks without ever seeing the secret; matches how the
  arr API keys are kept out of views.

## R6 Transfer transitions

- **Decision**: classify Put.io statuses as active (anything else), finished
  (`COMPLETED`, `SEEDING`) and failed (`ERROR`). For each fresh transfer list: a new id →
  `transfer.added` (then `finished` or `failed` too if it already is); active → finished
  or failed → the matching event; failed → finished (a retry) → `transfer.finished`.
  The id → class map of the latest list is stored in `daemon_flags`
  (`transfer_events_seen`) whenever it changes. With no stored map (first start after
  upgrading) the first list is taken as the baseline without events.
- **Origin**: `transfer_origins` (watch, manual) first, then the arr table by Put.io
  transfer id (`radarr`/`sonarr` from the arr item), else `putio`.
- **Where**: `DownloadManager::transfers_snapshot` when it fetches a fresh list; the
  daemon's pipeline loop calls it every 10 s with a 60 s max age. In `server` mode the CLI
  runs a 30 s ticker that calls it when a hook wants a `transfer.*` type.

## R7 Storage low

- **Decision**: `StorageService::refresh` (every 15 minutes in the daemon's storage loop,
  and in the server's ticker) checks `is_low(size, free, warn_free_percent)`; an
  `AtomicBool` turns true on the first low reading (emit) and false on the first reading
  above the threshold. Not persisted: a restart while low sends one more event.
- **Rationale**: simplest edge detection; a reminder after a restart is acceptable (the
  NAS restarts weekly, so at most one event a week while low).

## R8 Schema format and its test

- **Decision**: JSON Schema 2020-12 in `putio-sync-core/src/events/schema.json`, one
  `$defs` entry per type, the envelope with `oneOf` over `type` constants. A test builds a
  sample of every payload, serialises it and checks it against the schema with a small
  validator for the keywords used (`type`, `required`, `properties`, `enum`, `const`,
  `additionalProperties: true`); a second test checks `EventType::ALL` matches the schema
  and `EVENTS.md`'s headings.
- **Alternatives**: the `jsonschema` crate (large dependency tree for one test).

## R9 Hostname

- **Decision**: `source.host` from `HOSTNAME`, else `/etc/hostname`, else `COMPUTERNAME`,
  else `unknown`, read once.
- **Rationale**: avoids a new dependency for one string.

## R10 The unused `[notifications]` webhook

- **Decision**: left as is; the assistant warns when it is set ("has no effect; use a
  hook"). 030 replaces it.
