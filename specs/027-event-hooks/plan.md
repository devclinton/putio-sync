# Implementation Plan: Event Hooks

**Branch**: `027-event-hooks` (worktree branch `claude/hungry-heyrovsky-9bcd85`) | **Date**: 2026-11-04 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/027-event-hooks/spec.md`

## Summary

`core` gains an `events` module: the versioned `Event` envelope (`schema_version`, `id`,
`type`, `occurred_at`, `source`, `data`), one typed payload struct per event type, the
JSON Schema (`events/schema.json`, embedded with `include_str!`), and a process-wide
event bus: a `tokio::sync::broadcast` sender in a `OnceLock`, with `events::emit(payload)`
(never blocks, never fails the caller) and `events::subscribe()`. Being process-wide is
what lets the `server --download` thread (its own runtime and `DownloadManager`) reach the
same hooks as the API process, and it is what 028/030/031 will subscribe to later.
`core` also gains `redact::url()` (scheme, host, port and a shortened path; no query, no
user info), used by every log line, error and API answer that names a hook URL.

Emitters, each in the crate that owns the concern:

- `download`: `download_file` emits `download.finished` on success and `download.failed`
  on failure (not on skip or stop); `sync_downloads` emits `sync.completed` with a
  `trigger` taken from an `AtomicBool` that `request_sync` sets.
- `download`: a `TransferEvents` tracker diffs each fresh Put.io transfer list in
  `transfers_snapshot` against the last one (persisted in `daemon_flags` as
  `transfer_events_seen`, so a restart does not re-send) and emits `transfer.added`,
  `transfer.finished` and `transfer.failed`, looking up the origin in
  `transfer_origins` and the arr table.
- `storage`: `StorageService::refresh` compares free space with `warn_free_percent` and
  emits `storage.low` on the crossing from not-low to low (in-memory edge flag).

A new crate, `putio-sync-hooks`, owns running hooks: `HooksService` subscribes to the bus
with one dispatcher task that copies each event into the bounded queue (1 000, drop
oldest) of every enabled hook whose filter matches; one worker task per hook runs one
event at a time. A command runs with `tokio::process::Command` (absolute program, argument
list, `env_clear` plus a fixed environment, event JSON on stdin, stdout/stderr kept to
4 KiB each, `kill_on_drop`, timeout). A webhook POSTs with `reqwest` (headers, User-Agent,
per-attempt timeout, same-host redirects up to 3, retries after 5 s and 30 s on
connect/timeout/429/5xx). Each run is logged, counted, and kept in a 20-entry ring per
hook. `configure(hooks)` swaps the set live, keeping queues and history by name.
`test(name)` runs a `hook.test` event against one hook right away and returns the run.

The CLI's `daemon` and `server` commands build one `HooksService`, start it, and hand it
to the `AppState` and the `Doctor`. In `server` mode a small task polls the transfer list
(only while a hook listens to a `transfer.*` type) and refreshes storage every 15 minutes,
since the daemon's loops are not running there.

`config` gains `hooks: Vec<HookConfig>` (`[[hooks]]`), validated in `Config::validate`.
The API gains `GET /hooks`, `POST /hooks/test` and `GET /events/schema`, and a `hooks`
settings group (`GET`/`PUT /settings/hooks`, `/preview`) whose views mask header and env
values (`********`; a save sending the mask keeps the stored value). The assistant gains
the hook risks, the doctor a `Hooks` check, telemetry four families. The UI gets a Hooks
group in Settings with a hook editor, the last run per hook and "Send test event".

## Technical Context

**Language/Version**: Rust stable (edition 2021); browser ES modules for the UI

**Primary Dependencies**: `uuid` (v4) added to `core` for event ids; it is already in
the workspace lock through `api` and `storage`. The new `putio-sync-hooks` crate uses only
workspace dependencies (`tokio`, `reqwest`, `serde_json`, `tracing`, `chrono`).

**Storage**: config TOML (`[[hooks]]`); one new key in the existing `daemon_flags` table
(`transfer_events_seen`), no migration. Run history is in memory only.

**Testing**: core unit tests for the envelope, payload serialisation, `redact::url`, hook
validation and the schema (a small in-test validator checks every sample event against
`schema.json`); hooks crate tests with real child processes (`/bin/sh` scripts in a temp
dir: stdin, env, exit codes, timeout kill, output cut) and a mockito server (headers,
retries, 4xx no-retry, redirects to another host refused, no secret in errors); download
tests for the transfer diff and the emitted events; storage test for the low edge; API
tests (`/hooks`, `/hooks/test`, settings masking and keep-on-mask, refusal, setup-mode
503, secrets sweep); telemetry contract; doctor check unit test; UI node test for the hook
editor helpers.

**Target Platform**: Linux NAS daemon (Docker), desktop CLI. Command hooks rely on Unix
process semantics; on Windows they run without the executable-bit check.

**Project Type**: Cargo workspace: daemon + HTTP API + web UI

**Performance Goals**: `emit` is one broadcast send (no allocation beyond the `Arc`); the
dispatcher only clones `Arc<Event>` into queues.

**Constraints**: UI bundle cap 384 KB (about 58 KB free); setup mode answers 503 for every
non-wizard route (the fallback already covers new routes).

**Scale/Scope**: up to 32 hooks and 32 headers per hook (validated), so the `hook` metric
label stays bounded.

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Crate Boundaries | ✅ | Event types and the bus are shared by download, storage, hooks, api and doctor, so they live in `core`. Running hooks is its own concern, in a new `putio-sync-hooks` crate (depends on core and telemetry only). Emitters stay in the crates that own the event. |
| II. Local-First Verification | ✅ | `make ci` covers it all; the new crate joins the workspace and the Dockerfile's crate list. |
| III. Tests Without Put.io | ✅ | Transfer diff tests use fixed `Transfer` lists; webhook tests use mockito; command tests use local shell scripts. |
| IV. Download Integrity | ✅ | Events are emitted after the download is recorded; hooks cannot change or delay it. Nothing is deleted. |
| V. Observable by Default | ✅ | `putio_sync_events_total{type}`, `putio_sync_hook_runs_total{hook,event,result}`, `putio_sync_hook_run_duration_seconds{hook}`, `putio_sync_hook_events_dropped_total{hook}`; labels are closed sets or bounded by config. Each run is logged with structured fields. |
| VI. Secrets Stay Secret | ✅ | Header and env values masked in every view; URLs redacted everywhere; commands get a clean environment, so the Put.io token never reaches them; events hold no secrets; the secrets sweep test covers hooks. |
| VII. Simplicity | ✅ | One new crate, justified by the boundary rule (process and HTTP running is neither core nor api). One new dependency in core (`uuid`, already in the lock). No retries for commands, no templates, no persistence of runs. |

Post-design re-check: unchanged, all ✅.

## Project Structure

### Documentation (this feature)

```text
specs/027-event-hooks/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── events.md       # event envelope and payloads (schema.json is the source)
│   ├── config.md       # [[hooks]]
│   └── api.md          # /hooks, /hooks/test, /events/schema, /settings/hooks
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/src/events.rs              # new: Event, payloads, bus, emit/subscribe
putio-sync-core/src/events/schema.json     # new: JSON Schema, served and documented
putio-sync-core/src/redact.rs              # new: url()
putio-sync-core/src/hooks.rs               # new: HookConfig, validation, risks helpers
putio-sync-core/src/config.rs              # Config.hooks; validate
putio-sync-core/src/assistant.rs           # hook risks
putio-sync-telemetry/src/{lib,hooks}.rs, tests/contract.rs   # four families
putio-sync-hooks/                          # new crate: service, runner (command, webhook)
putio-sync-download/src/transfer_events.rs # new: transfer diff
putio-sync-download/src/manager.rs         # emit download/sync events, transfer tracker
putio-sync-db/src/pipeline.rs              # TRANSFER_EVENTS_SEEN flag key
putio-sync-db/src/arr.rs                   # arr item by transfer id (origin lookup)
putio-sync-storage/src/lib.rs              # storage.low edge
putio-sync-doctor/src/{report.rs,checks/mod.rs,checks/hooks.rs}   # Hooks check
putio-sync-api/src/lib.rs                  # AppState.hooks, with_hooks, routes
putio-sync-api/src/routes/system/hooks.rs  # new: /hooks, /hooks/test, /events/schema
putio-sync-api/src/routes/system/{settings,assistant}.rs          # hooks group
putio-sync-api/ui/hooks.js                 # new: DOM-free helpers (target label, masks)
putio-sync-api/ui/pages/settings.js        # Hooks group editor
putio-sync-cli/src/commands/{daemon,server}.rs                    # build and start hooks
EVENTS.md                                  # new: event reference with examples
examples/hooks/on-download.py              # new: example hook
README.md, config.example.toml, example-config.toml, CHANGELOG.md, Dockerfile, Cargo.toml
```

**Structure Decision**: existing workspace layout plus one crate (`putio-sync-hooks`).

## Complexity Tracking

| Addition | Why needed | Simpler alternative rejected because |
|----------|------------|--------------------------------------|
| New crate `putio-sync-hooks` | Spawning processes and calling URLs for hooks is a concern of its own, used by api, doctor and cli | In `core` it would pull process handling into the crate every other crate depends on; in `api` the doctor could not reach it without an upward dependency |
| Process-wide bus (global) | `server --download` runs a second runtime with its own manager; both must reach one set of hooks | Passing a sender through every constructor would touch every `DownloadManager::new` caller and still miss the legacy thread |
