# Implementation Plan: Lua Plugins

**Branch**: `028-lua-plugins` (worktree branch `claude/inspiring-davinci-dc6601`) | **Date**: 2026-11-04 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/028-lua-plugins/spec.md`

## Summary

A new crate, `putio-sync-plugins`, is the second consumer of the 027 event bus. It finds
plugins (bundled ones compiled in with `include_str!`, and folders under the plugins
folder), parses each `plugin.toml` manifest, and runs each enabled plugin in its own
sandboxed Lua 5.4 state (`mlua`, vendored, so no system library) on its own OS thread. A
dispatcher task copies each bus event into the bounded queue (1 000, drop oldest) of every
enabled plugin that declared its type, like the hooks dispatcher. A 60 s ticker queues a
`tick` for plugins whose script defines `on_tick`. The worker thread runs one job at a time
with a wall-clock deadline enforced by an instruction-count hook (every 1 000
instructions) and by the timeout on each host HTTP call, and a memory limit set on the
state; a memory error rebuilds the state before the next job.

The host API is a `putio` table: `log`, `config`, `http.request` (approved hosts only,
through `Handle::block_on` on the shared `reqwest` client), `store` (a JSON file per
plugin), `time`, `json`, `plugin`, `version`. It is specified in JSON terms in
`contracts/host-api.md` and `PLUGINS.md`, so 029 can expose the same calls to WebAssembly.

`core` gains `plugins.rs`: `PluginSettings` (`[[plugins]]`: name, enabled, approved hosts,
timeout, memory, `config` and `secrets` maps) with structural validation and masking of
`secrets` (like hook headers), plus `Config.plugins_dir` and `Config.plugins`. Secret
fields are stored in `secrets` so core can mask them without knowing manifests; checking
values against field types needs the manifest and is done by the plugins crate. `events`
gains `plugin.test`; hooks' `*` no longer matches it.

The API gains `GET /plugins`, `POST /plugins/test`, `POST /plugins/reload` and a
`plugins` settings group (`GET`/`PUT /settings/plugins`, `/preview`). A save validates the
values against the manifests, records the needed hosts of each enabled plugin as approved,
writes the config and reconfigures the running service. The doctor gains a `Plugins`
check, telemetry three families, the UI a Plugins group. Four notifiers (ntfy, Discord,
Telegram, Pushover) ship bundled, disabled, built only on the public API.

The synchronous extension points (filters, rename and folder rules) are split off, as
recorded in the spec.

## Technical Context

**Language/Version**: Rust stable (edition 2021); Lua 5.4 for plugins; browser ES modules
for the UI

**Primary Dependencies**: `mlua` 0.11 with `lua54`, `vendored`, `serialize` (new, see
Complexity Tracking). `vendored` builds Lua's C sources with the `cc` crate, which already
runs for `ring` on every target, so no new build packages. Everything else is in the
workspace: `tokio`, `reqwest`, `serde`, `serde_json`, `toml`, `tracing`, `chrono`.

**Storage**: config TOML (`plugins_dir`, `[[plugins]]`); one JSON store file per plugin in
`<config dir>/plugin-data/`; run history in memory.

**Testing**: plugins crate unit tests (manifest validation, sandbox globals, time and
memory limits, `load` text-only, host API calls, store limits and persistence, host
approval, secret scrubbing, queue drop, reload keeping state) with plugins written to temp
dirs; HTTP through mockito; bundled notifiers run against mockito with a fake clock for the
grouping window; API tests (`/plugins`, test, reload, settings masking, refusal, approval,
setup-mode 503, secrets sweep); doctor check test; telemetry contract; UI node test for the
plugin form helpers.

**Target Platform**: Linux NAS daemon (Docker amd64/arm64 on Alpine musl), Snap
(amd64/arm64), Debian (cross arm64), Windows MSVC, macOS.

**Project Type**: Cargo workspace: daemon + HTTP API + web UI

**Performance Goals**: no work on the sync path: emit stays one broadcast send; a plugin
call never runs on a Tokio worker thread.

**Constraints**: UI cap 384 KB with ~38 KB free (the Plugins group gets about 10 KB);
setup mode answers 503 for every non-wizard route.

**Scale/Scope**: up to 32 plugins configured, names `[A-Za-z0-9_-]{1,40}`, so the `plugin`
metric label stays bounded. One OS thread per enabled plugin.

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Crate Boundaries | ✅ | Settings types and their masking live in `core` (config, assistant, api use them). Running Lua is its own concern in `putio-sync-plugins` (depends on core and telemetry). api, doctor and cli use it, like hooks. |
| II. Local-First Verification | ✅ | `make ci` covers the crate; vendored Lua needs no system package. Dockerfile crate list updated. |
| III. Tests Without Put.io | ✅ | Plugins are fed synthetic events; HTTP goes to mockito. |
| IV. Download Integrity | ✅ | Plugins only observe events; they cannot touch files or delay a download. |
| V. Observable by Default | ✅ | `putio_sync_plugin_runs_total{plugin,event,result}`, `putio_sync_plugin_run_duration_seconds{plugin}`, `putio_sync_plugin_events_dropped_total{plugin}`; each run logged with structured fields; plugin log lines carry `plugin`. |
| VI. Secrets Stay Secret | ✅ | Secret values in `secrets`, masked in every view and the export; scrubbed from plugin log lines and errors; HTTP errors name only the host; no `os.getenv`, so the daemon's environment (Put.io token) is out of reach; secrets sweep extended. |
| VII. Simplicity | ⚠️ justified | One new dependency (`mlua`) and one new crate; see Complexity Tracking. No install from URL, no `require`, no env-var secrets. |

Post-design re-check: unchanged.

## Project Structure

### Documentation (this feature)

```text
specs/028-lua-plugins/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── manifest.md     # plugin.toml
│   ├── host-api.md     # the putio table, in JSON terms (shared with 029)
│   ├── config.md       # plugins_dir, [[plugins]]
│   └── api.md          # /plugins, /plugins/test, /plugins/reload, /settings/plugins
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/src/plugins.rs              # new: PluginSettings, validate, mask/unmask
putio-sync-core/src/config.rs               # plugins_dir, plugins; validate
putio-sync-core/src/events.rs, events/schema.json   # plugin.test
putio-sync-core/src/hooks.rs                # `*` skips plugin.test
putio-sync-core/src/assistant.rs            # export/import masking of plugin secrets
putio-sync-telemetry/src/{lib,plugins}.rs, tests/contract.rs   # three families
putio-sync-plugins/                         # new crate
  src/lib.rs          # PluginRun, RunResult, PluginStatus, public API
  src/manifest.rs     # plugin.toml parsing and validation, needed hosts
  src/discover.rs     # bundled + folder discovery, duplicate handling
  src/sandbox.rs      # Lua state: safe libs, load/print wrappers, limits
  src/host.rs         # the putio table: log, config, http, store, time, json
  src/store.rs        # JSON file key-value store with limits
  src/worker.rs       # per-plugin thread, queue, deadline, rebuild on OOM
  src/service.rs      # PluginsService: start, configure, reload, test, status, wants
  bundled/{ntfy,discord,telegram,pushover}/{plugin.toml,main.lua}
putio-sync-doctor/src/{report.rs,lib.rs,checks/mod.rs,checks/plugins.rs}
putio-sync-api/src/lib.rs                   # AppState.plugins, with_plugins, routes
putio-sync-api/src/routes/system/plugins.rs # new
putio-sync-api/src/routes/system/settings.rs   # plugins group
putio-sync-api/ui/plugins.js                # new: DOM-free form helpers
putio-sync-api/ui/pages/settings.js         # Plugins group
putio-sync-cli/src/commands/{daemon,server}.rs # build and start plugins
PLUGINS.md                                  # new: guide and host API reference
EVENTS.md, README.md, config.example.toml, example-config.toml, CHANGELOG.md,
Dockerfile, Cargo.toml, ROADMAP.md
```

**Structure Decision**: existing workspace layout plus one crate (`putio-sync-plugins`).

## Complexity Tracking

| Addition | Why needed | Simpler alternative rejected because |
|----------|------------|--------------------------------------|
| `mlua` (vendored Lua 5.4) | The feature is an embedded Lua runtime; mlua gives memory limits, instruction hooks and serde conversion | Writing bindings to the Lua C API by hand is unsafe code for no gain; `rlua` is unmaintained; Luau needs a C++ toolchain on every target and has no per-instruction hook |
| New crate `putio-sync-plugins` | A scripting runtime with threads and HTTP is a concern of its own, used by api, doctor and cli | In `core` every crate would compile Lua; in `hooks` it would mix two runtimes with different limits and lifecycles |
| One OS thread per enabled plugin | A Lua state is `!Send` without mlua's `send` feature, and a call may block on HTTP or loop until its deadline | Running on Tokio workers would let a looping script stall the runtime that serves downloads; a shared thread pool would let one slow plugin delay others |
