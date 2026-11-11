# Tasks: WebAssembly Plugins

**Input**: `specs/029-wasm-plugins/` (plan.md, spec.md, research.md, data-model.md, contracts/)

**Tests**: requested by the spec (FR-008, SC-001 to SC-003).

## Phase 1: Setup

- [X] T001 Measure wasmi against wasmtime (size, build time) and record it in research.md
- [X] T002 Add `wasmi` (`std`, `validate`, `auto-dispatch`) and `wasmi_core` to putio-sync-plugins/Cargo.toml; `wat`, `tempfile`, `mockito` as dev-dependencies

## Phase 2: Foundational

- [X] T003 `runtime`, default `entry` per runtime and `permissions.files` with `paths_needed` in putio-sync-plugins/src/manifest.rs, with tests
- [X] T004 `approved_paths` in putio-sync-core/src/plugins.rs; Needs approval when folders are missing in putio-sync-plugins/src/service.rs
- [X] T005 JSON host dispatch for the whole API v1 in putio-sync-plugins/src/host.rs, shared with Lua; `Files` with checked read/write/list/exists and `putio.fs` for Lua
- [X] T006 Discovery: wasm entry size cap and runtime check in putio-sync-plugins/src/discover.rs

## Phase 3: US1 run a plugin in another language (P1)

- [X] T007 [US1] putio-sync-plugins/src/wasm.rs: eager engine with fuel, memory limiter, `putio.call`/`reply`/`fail`, WASI shim, `_initialize`, result reading, failure text
- [X] T008 [US1] Runtime switch in putio-sync-plugins/src/worker.rs: events, ticks, actions, health and sync hooks through the same jobs; rebuild after a timeout, trap or memory failure
- [X] T009 [US1] putio-sync-plugins/tests/wasm.rs: timeout and rebuild, memory limit, `fail` and `proc_exit`, host call and reply, Broken modules

## Phase 4: US2 permissions (P1)

- [X] T010 [US2] Host tests: JSON calls match the Lua API, need their permission; files stay in their folders (`..`, links, relative paths, write-only); `{download_dir}` folders need a download dir
- [X] T011 [US2] UI: folders named on the card and in the Enable wording, `approved_paths` kept on save (putio-sync-api/ui/plugins.js, settings/plugins.js, tests/ui/plugins.test.mjs)

## Phase 5: US3 install (P2)

- [X] T012 [US3] putio-sync-plugins/src/install.rs: unpack and check a package, write through a hidden folder, replace only when asked; unit tests
- [X] T013 [US3] `POST /plugins/install` in putio-sync-api/src/routes/system/plugins.rs: URL (module, or manifest with its entry beside it), base64 module, manifest with entry; same-host redirects, size and time caps
- [X] T014 [US3] API tests in putio-sync-api/tests/plugins.rs; route in the auth sweep (tests/secrets_sweep.rs)
- [X] T015 [US3] Install form loaded on first open (putio-sync-api/ui/settings/plugin_install.js), registered in src/ui.rs; `make i18n`

## Phase 6: US4 examples (P3)

- [X] T016 [P] [US4] examples/plugins/rust-clean-names/ with its binding, embedded manifest, checked-in plugin.wasm and README
- [X] T017 [P] [US4] examples/plugins/go-min-size-filter/ with its binding and README (Go and TinyGo build steps); plugin.wasm built by the test
- [X] T018 [US4] putio-sync-plugins/tests/examples.rs: build the Go example when `go` is installed, load both, check an event and a sync rule

## Phase 7: Polish

- [X] T019 PLUGINS.md (WebAssembly plugins, file access, install), README (plugins, API table), config.example.toml, examples/plugins/README.md
- [X] T020 CHANGELOG under Unreleased; ROADMAP 029 done
- [X] T021 UI under 488 KiB (tests/ui_assets.rs)
- [X] T022 `cargo fmt`, clippy, `cargo test --workspace --no-fail-fast`

## Deviations

- The Go example's `plugin.wasm` is built by the test instead of checked in: Go output
  is about 4.4 MB and changes with each Go release. Without Go the test skips it and says so.
- TinyGo was not installed here, so the TinyGo build step is documented but untested; the
  example is built and tested with stock Go 1.27.
- No JavaScript example (spec assumption).
