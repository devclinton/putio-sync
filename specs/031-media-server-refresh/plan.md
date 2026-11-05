# Implementation Plan: Media Server Refresh

**Branch**: `031-media-server-refresh` (worktree branch `claude/distracted-darwin-51cb86`) | **Date**: 2026-11-04 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/031-media-server-refresh/spec.md`

## Summary

Three bundled Lua plugins, `plex`, `jellyfin` and `emby`, built on 028's plugin host and
sharing `bundled/mediaserver.lua` (path mapping helpers, the per-folder grouping window,
library matching), the way the notifiers share `notifier.lua`. On `download.finished` a
plugin maps the file's folder, finds the library holding it and marks the folder pending;
`on_tick` (every 5 s) refreshes folders that have been quiet for `delay_seconds` or have
waited `max_wait_seconds`; `sync.completed` refreshes all pending folders at once.

The plugin host gains six general features, all additive (API version stays 1):

1. **Actions**: `[[actions]]` in the manifest, `plugin.on_action(id, input)` in the
   script, `POST /plugins/action` in the API. An action runs on a worker of its own
   (like a test of a disabled plugin) with the saved settings plus the form's unsaved
   non-secret values, the hosts needed for them allowed for that call. The result
   (message, code, link, choices, next step, values to save) is checked by the service;
   values to save are written to the config by the API through the normal settings path.
2. **Choices**: `choices = "<action id>"` on a config field; the UI's Load button calls
   that action and offers the returned `{value, label}` list.
3. **`path_map` field type** and `putio.path.{map,parent,within}`.
4. **`tick_seconds`** per manifest (5–3600); the service ticker wakes every 5 s.
5. **`health()`**: `PluginsService::health(name)` runs it on a worker of its own; the
   doctor's Plugins check reports its items.
6. **`putio.metrics.count`** → `putio_sync_plugin_counter_total{plugin,name}`, 16 names
   per plugin.

Plus `putio.download_dir` and one shared store object per plugin (`Arc<Mutex<Store>>`
kept by the service), so action, health and test workers never write a stale copy.

## Technical Context

**Language/Version**: Rust (workspace toolchain, `rust-toolchain.toml`), Lua 5.4 (mlua 0.11
vendored), plain ES modules for the UI

**Primary Dependencies**: existing only: mlua, reqwest, tokio, serde_json, axum, prometheus

**Storage**: `[[plugins]]` in `config.toml` (path_map values as arrays of `{from, to}`
tables); plugin store JSON in `plugin-data/<name>.json` (Plex client id)

**Testing**: `cargo test` with mockito servers standing in for plex.tv, Plex, Jellyfin and
Emby (Constitution III); node tests for `ui/plugins.js`; API tests for `/plugins/action`;
the secrets sweep extended to a signed-in Plex plugin

**Target Platform**: Linux NAS (Docker, Snap), macOS for development

**Project Type**: daemon + HTTP API + web UI (Cargo workspace)

**Performance Goals**: a finished file is asked for within `delay_seconds` + 5 s (default
25 s); one request per folder per window

**Constraints**: UI bundle under 384 KB (about 34 KB left); tokens never logged; a down
server never slows downloads (plugin thread and queue, from 028)

**Scale/Scope**: one to three media servers per install; dozens of folders per sync run

## Constitution Check

| Principle | Status |
|-----------|--------|
| I. Crate boundaries | Host features in `putio-sync-plugins`; settings shape in `core/plugins.rs`; route glue in `api`; counter family in `telemetry`; doctor items in `doctor`. No integration code outside the bundled Lua. Pass |
| II. Local-first verification | `make ci` covers all new tests (Rust + node UI tests). Pass |
| III. Tests without Put.io | Media servers and plex.tv are mockito servers; no real network. Pass |
| IV. Download integrity | Nothing touches downloads; refreshes run after `download.finished`. Pass |
| V. Observable by default | Plugin counters with bounded labels (plugin name × ≤16 names); runs already logged with structured fields; ticks of the media plugins stay quiet on success. Pass |
| VI. Secrets stay secret | Tokens are secrets (masked, scrubbed); action results are scrubbed with the plugin's secrets and the newly saved ones; tokens go in headers only; secrets sweep extended. Pass |

Post-design re-check: unchanged, pass.

## Project Structure

### Documentation (this feature)

```text
specs/031-media-server-refresh/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── host-api.md      # additions to 028's host API v1
│   ├── manifest.md      # actions, choices, path_map, tick_seconds
│   ├── api.md           # POST /plugins/action, GET /plugins additions
│   └── media-plugins.md # settings, actions, HTTP calls of plex/jellyfin/emby
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/src/plugins.rs          # config values may be arrays of {from,to}
putio-sync-plugins/
├── src/manifest.rs                     # FieldType::PathMap, ConfigField.choices, Action, tick_seconds
├── src/host.rs                         # putio.path, putio.metrics, putio.download_dir; shared store
├── src/paths.rs                        # path mapping rules (new; unit tested)
├── src/worker.rs                       # Job::Call (action / health), tick bookkeeping
├── src/service.rs                      # action(), health(), tick_due(), set_download_dir(), stores
├── src/action.rs                       # ActionResult parsing and checks (new)
├── bundled/mediaserver.lua             # shared: grouping, library match (new)
├── bundled/plex/{plugin.toml,main.lua}
├── bundled/jellyfin/{plugin.toml,main.lua}
├── bundled/emby/{plugin.toml,main.lua}
└── tests/{actions.rs,mediaserver.rs}   # new
putio-sync-telemetry/src/plugins.rs     # putio_sync_plugin_counter_total
putio-sync-doctor/src/checks/plugins.rs # health items
putio-sync-api/src/routes/system/plugins.rs # POST /plugins/action
putio-sync-api/ui/plugins.js            # path_map text, action result handling (no DOM)
putio-sync-api/ui/pages/settings.js     # action buttons, Load choices
putio-sync-api/tests/{plugins.rs,secrets_sweep.rs,ui/plugins.test.mjs}
putio-sync-cli/src/commands/daemon.rs   # download_dir into PluginsOptions (and server mode)
PLUGINS.md, README.md, config.example.toml, example-config.toml, CHANGELOG.md, ROADMAP.md
```

**Structure Decision**: everything lives in the existing crates; two new source files in
`putio-sync-plugins` keep `host.rs` and `service.rs` from growing past reading size.

## Complexity Tracking

None. No native integration code; each host feature is general and documented for any
plugin.
