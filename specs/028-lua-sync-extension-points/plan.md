# Implementation Plan: Lua Plugins, Sync Extension Points

**Branch**: `claude/022-stream-mode-dlna-4b51d8` (stacked, no own branch) | **Date**: 2026-11-10 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/028-lua-sync-extension-points/spec.md`

## Summary

Plugins may declare `hooks = ["filter", "rename", "folder"]` in `plugin.toml`. The sync asks
them through a runtime-neutral trait, `putio_sync_core::sync_hooks::SyncHooks`, that the
download manager holds like its `PathRedirect` seam. `PluginsService` implements the trait:
each enabled plugin with hooks gets a second worker (own Lua state, same spec, shared store),
so hook calls never queue behind events. Each call has a per-plugin limit (`hook_timeout_ms`,
default 500). Failures, timeouts and bad answers fall back to the built-in result; three in a
row pause the plugin's hooks for five minutes. The filter runs in `enqueue_tree_inner` after
`Effective::decide`; rename and folder run in `download_file` before the pipeline redirect.

## Technical Context

**Language/Version**: Rust stable (edition 2021)

**Primary Dependencies**: existing `mlua` (Lua 5.4) sandbox in `putio-sync-plugins`; no new crates

**Storage**: none new (plugin stores are reused)

**Testing**: `cargo test` in `putio-sync-core`, `putio-sync-plugins` (examples, limits) and `putio-sync-download` (filter/rename/folder with a fake `SyncHooks`); UI node tests unchanged

**Target Platform**: NAS daemon (Docker), desktop CLI

**Project Type**: Cargo workspace (daemon + web UI without a build step)

**Performance Goals**: a pure-Lua hook answer in well under 1 ms; 1 000 files cost < 1 s (SC-001)

**Constraints**: per-call limit 50–5000 ms; a dead plugin costs at most 3 limits per 5 minutes

**Scale/Scope**: up to 32 plugins; syncs of thousands of files

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | Trait and answer checks in `core` (shared by `download` and `plugins`); `plugins` does not depend on `download`; wiring in `cli`. Pass |
| II. Local-first | Covered by `make ci`; no new gate. Pass |
| III. Tests without Put.io | Download tests use the existing mock server and a fake `SyncHooks`. Pass |
| IV. Download integrity | Rename/folder change only the final path before the temp file is made; the atomic rename and verification are unchanged. Pass |
| V. Observable | Calls counted in the existing `putio_sync_plugin_runs_total{plugin, event, result}` with `event` = `filter`/`rename`/`folder` (bounded); failures logged with structured fields. Pass |
| VI. Secrets | Same scrubbing as event runs. Pass |
| VII. Simplicity | One new setting (`hook_timeout_ms`), needed for the "short limit" requirement; no new dependency. Pass |

Post-design re-check: unchanged.

## Project Structure

### Documentation (this feature)

```text
specs/028-lua-sync-extension-points/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── sync-hooks.md     # trait, file description, answers, Lua functions
│   └── manifest.md       # `hooks` in plugin.toml, `hook_timeout_ms` in [[plugins]]
└── tasks.md
```

### Source Code

```text
putio-sync-core/src/sync_hooks.rs          # NEW: Hook, HookFile, FilterAnswer, SyncHooks, answer checks
putio-sync-core/src/plugins.rs             # hook_timeout_ms
putio-sync-plugins/src/manifest.rs         # hooks field and checks
putio-sync-plugins/src/worker.rs           # on_event optional for hook-only plugins; hook functions checked; per-job limit
putio-sync-plugins/src/hooks.rs            # NEW: hook slots, failure pause, SyncHooks impl
putio-sync-plugins/src/service.rs          # hook slots kept with the event slots; status shows hooks
putio-sync-download/src/manager.rs         # set_sync_hooks; filter in the sync pass; rename/folder at download
putio-sync-cli/src/commands/daemon.rs      # manager.set_sync_hooks(plugins)
putio-sync-api/ui/settings/plugins.js      # "Sync rules: …" line, time limit field
examples/plugins/{min-size-filter,clean-names,tv-folders}/   # NEW examples
putio-sync-plugins/tests/examples.rs       # NEW: loads and runs the examples
putio-sync-download/tests/sync_hooks.rs    # NEW: the sync honours / falls back
```

**Structure Decision**: extend the existing plugin crate and the download manager's seams; no
parallel plugin system.

## Complexity Tracking

None.
