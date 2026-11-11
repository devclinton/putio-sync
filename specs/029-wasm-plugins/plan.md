# Implementation Plan: WebAssembly Plugins

**Feature**: `specs/029-wasm-plugins` | **Date**: 2026-11-10 | **Spec**: [spec.md](spec.md)

## Summary

Add a second plugin runtime next to Lua: WebAssembly core modules run by wasmi, behind the
same manifest, discovery, worker, event dispatch, sync hooks, limits and fallback. The host
API is shared: Lua's `putio.*` calls and WebAssembly's JSON `putio.call` go through the same
code in `host.rs`. Add file permissions for both runtimes, a plugin install endpoint and
form, and Rust and Go examples that the tests build or load.

## Technical Context

**Language/Version**: Rust 1.96 (workspace), examples in Rust (wasm32-unknown-unknown) and
Go 1.24+ (wasip1)
**Primary Dependencies**: wasmi 2.0 (new), mlua (existing), axum, reqwest
**Storage**: plugin folders under `plugins_dir`; `approved_paths` in `[[plugins]]`
**Testing**: cargo test (unit, `putio-sync-plugins/tests/{wasm,examples}.rs`,
`putio-sync-api/tests/plugins.rs`), node tests for the UI
**Target Platform**: every release target, including armv7 and musl
**Constraints**: binary growth ≤ 3 MB, no C or JIT toolchain, UI ≤ 488 KiB
**Scale/Scope**: a handful of plugins per install, small JSON calls

## Constitution Check

- Simple, self-contained daemon: one pure-Rust crate added; no new process or service. ✓
- Safe by default: plugins install off, every host, folder and WASI call is denied unless
  declared and approved. ✓
- Tests with the change: runtime, limits, ABI, permissions, install and examples are
  covered. ✓
- Docs and CHANGELOG with every change. ✓

## Project Structure

```text
putio-sync-plugins/src/
  wasm.rs        # new: engine, limiter, ABI, WASI shim, manifest section
  install.rs     # new: write a package into the plugins folder
  host.rs        # JSON dispatch shared by both runtimes; putio.fs and Files
  manifest.rs    # runtime, entry, permissions.files, paths_needed
  discover.rs    # wasm entry size cap, runtime check
  worker.rs      # runtime switch (Lua state or wasm Instance)
  service.rs     # approved_paths, install()
putio-sync-core/src/plugins.rs          # approved_paths setting
putio-sync-api/src/routes/system/plugins.rs  # POST /plugins/install
putio-sync-api/ui/plugins.js, settings/plugins.js, settings/plugin_install.js
examples/plugins/rust-clean-names/, go-min-size-filter/
```

## Phases

- Phase 0: [research.md](research.md) — runtime and interface choice, measured sizes.
- Phase 1: [data-model.md](data-model.md), [contracts/](contracts/),
  [quickstart.md](quickstart.md).
- Phase 2: [tasks.md](tasks.md).

## Complexity Tracking

None beyond the WASI shim, which exists only so stock toolchain output loads.
