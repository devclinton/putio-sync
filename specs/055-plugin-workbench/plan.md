# Implementation Plan: Plugin Workbench

**Feature**: `specs/055-plugin-workbench` | **Date**: 2026-11-11 | **Spec**: [spec.md](spec.md)

## Summary

Add a workbench to Settings › Plugins: a Run panel that calls any plugin function (an event, a
sync hook, `on_tick`, `health` or an action) on a worker of its own, as a dry run by default, and
a plain editor for Lua plugin folders behind an `plugin_editor` setting. A dry run records HTTP
requests and file writes instead of doing them and works on a scratch copy of the store. The
runner reuses the plugin worker and the JSON host shared by Lua and WebAssembly, so both runtimes
are covered. Three starters (webhook notifier, store counter, tick poller) ship in the binary and
in `examples/plugins/`. The docs book gains a "Writing and debugging plugins" page, and the UI
links to the docs site.

## Technical Context

**Language/Version**: Rust 1.96 (workspace); plain ES modules for the UI
**Primary Dependencies**: none new (mlua, wasmi, axum, serde, tokio already in use)
**Storage**: `plugin_editor = true|false` at the top of `config.toml`; plugin files in the
plugins folder; Run panel history in memory only
**Testing**: cargo test (`putio-sync-plugins` unit tests for the tap and source checks,
`tests/workbench.rs` for dry and real runs of Lua and WebAssembly plugins, `tests/examples.rs`
for the starters, `putio-sync-api/tests/plugins_workbench.rs` for the routes), node tests for the
UI logic, `ui_assets` for the size budget
**Target Platform**: every release target
**Constraints**: the static UI is 21 B under the 488 KiB cap, so the workbench is one module
loaded with `import()`; static files may only grow by what is trimmed elsewhere
**Scale/Scope**: one person editing at a time; runs are interactive, one per plugin at a time

## Constitution Check

- Simple, self-contained daemon: no new crate, no new dependency, no editor library. ✓
- Safe by default: the editor is off; a run is dry unless **Send for real** is chosen; a dry
  run never sends a request, writes a file or changes the real store; writes stay inside the
  plugin's own folder; secrets are masked in every part of a result. ✓
- Tests with the change: tap, source checks, runs of both runtimes, starters and routes. ✓
- Docs and CHANGELOG with every change. ✓

## Project Structure

```text
putio-sync-core/src/config.rs              # plugin_editor
putio-sync-plugins/
  src/workbench.rs                         # new: Tap, Call, RunRequest, Run, runner, history
  src/source.rs                            # new: starters, read, check, save (.bak), copy
  src/host.rs                              # Http/Files take the tap; log cap per thread
  src/worker.rs                            # Env.tap; scratch store; memory; no metrics for taps
  src/wasm.rs                              # Instance::memory_bytes
  src/service.rs                           # reload_one; lookup/begin/env for the workbench
  starters/{webhook-notifier,store-counter,tick-poller}/{plugin.toml,main.lua}
  tests/workbench.rs
examples/plugins/{webhook-notifier,store-counter,tick-poller}/  # same files plus README.md
putio-sync-api/src/routes/system/plugins.rs  # /plugins/workbench/* routes
putio-sync-api/ui/settings/workbench.js      # new, lazy: Run panel, editor, starters, links
putio-sync-api/ui/workbench.js               # new: pure logic (lines, problems, masks, links)
putio-sync-api/ui/settings/plugins.js        # the lazy "Plugin workbench" panel
putio-sync-api/ui/settings/integrations.js   # EVENTS.md becomes a docs link
site/book/src/plugin-workbench.md, SUMMARY.md
```

## Phases

- Phase 0: [research.md](research.md): runner design, dry-run seams, memory, source writes,
  starters in the binary, docs links, UI budget.
- Phase 1: [data-model.md](data-model.md), [contracts/workbench-api.md](contracts/workbench-api.md),
  [quickstart.md](quickstart.md).
- Phase 2: [tasks.md](tasks.md).

## Complexity Tracking

The editor toggle lives in the lazy workbench panel and has its own route instead of a field
on the Plugins form: a form field would cost static UI bytes, and a `PUT /settings/plugins`
from the panel would have to resend every plugin's settings.
