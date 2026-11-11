# Tasks: Plugin Workbench

**Input**: `specs/055-plugin-workbench/` (plan.md, spec.md, research.md, data-model.md, contracts/)

**Tests**: requested by the spec (SC-002 to SC-006).

## Phase 1: Setup

- [x] T001 `plugin_editor` in putio-sync-core/src/config.rs (default false, skipped when false)

## Phase 2: Foundational (the tap)

- [x] T002 putio-sync-plugins/src/workbench.rs: `Tap` (dry or real, reply, recorded requests and writes, secrets), header and value masking
- [x] T003 host.rs: `Http` and `Files` take the tap (record or send; record writes in a dry run); per-thread log cap of 200 for workbench workers
- [x] T004 worker.rs: `Env.tap`; scratch store; memory in use (Lua `used_memory`, wasm.rs `memory_bytes`); no metrics or history for tapped runs

## Phase 3: US1 Run panel (P1) 🎯 MVP

- [x] T005 [US1] workbench.rs: `Call`, `RunRequest`, `Run`; `PluginsService::workbench_run` (hosts needed for dry, approved for real; store snapshots; sync hook answers; unsaved text) and the last 20 runs
- [x] T006 [US1] putio-sync-plugins/tests/workbench.rs: dry and real HTTP against a local server, scratch store, dry fs.write, error line, timeout, sync hook answer, WebAssembly example, history and re-run
- [x] T007 [US1] API: `GET /plugins/workbench`, `POST /plugins/workbench/run`, `GET /plugins/workbench/runs`, `POST /plugins/workbench/runs/:id`

## Phase 4: US2 Editor (P1)

- [x] T008 [US2] putio-sync-plugins/src/source.rs: starters, read with versions, check (manifest and Lua syntax), save with `.tmp` and `.bak`, copy bundled, new from starter; `PluginsService::reload_only`
- [x] T009 [US2] Starters in putio-sync-plugins/starters/ (webhook-notifier, store-counter, tick-poller)
- [x] T010 [US2] API: `PUT /plugins/workbench/editor`, `POST /new`, `GET|PUT /source/:name`, `POST /check`, `POST /copy/:name`; putio-sync-api/tests/plugins_workbench.rs (403 when off, 409 stale, .bak, reload)

## Phase 5: UI (US1, US2)

- [x] T011 putio-sync-api/ui/workbench.js (pure: line numbers, problem lines, docs links, request rows) with tests/ui/workbench.test.mjs
- [x] T012 putio-sync-api/ui/settings/workbench.js (lazy): plugin picker, Run panel, result, history, editor, New plugin, Copy to edit, WebAssembly view with install form, editor toggle, help links
- [x] T013 settings/plugins.js: the "Plugin workbench" panel loads it with `import()`; integrations.js EVENTS.md link; trim static bytes; register assets in src/ui.rs; ui_assets passes
- [x] T014 Check the panel in a browser against a local server

## Phase 6: US3 Docs and examples (P2)

- [x] T015 [US3] examples/plugins/{webhook-notifier,store-counter,tick-poller} with READMEs; examples README; tests/workbench.rs checks they equal the starters and each passes a dry run
- [x] T016 [US3] site/book/src/plugin-workbench.md and SUMMARY.md
- [x] T017 PLUGINS.md, CHANGELOG (Unreleased), ROADMAP 055 done, config.example.toml, README API table
- [x] T018 `make i18n` and stale keys removed from the other catalogs

## Phase 7: Polish

- [x] T019 Full test run with `--no-fail-fast` and `bash scripts/ci-local.sh` (ui_i18n coverage fails until `make i18n-translate`)
