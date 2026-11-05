# Tasks: Media Server Refresh

**Input**: design documents in `specs/031-media-server-refresh/`
**Tests**: required (Constitution III; spec user stories name their independent tests).

## Phase 1: Setup

- [x] T001 Confirm the 028 plugin host builds and its tests pass before changes (`cargo test -p putio-sync-plugins`)

## Phase 2: Foundational (host additions, blocking every story)

- [x] T002 [P] Add path rules (`map`, `parent`, `within`, rule validation: "≤ 16 rules; `from` starts with `/`; `to` starts with `/`, `X:\`, `X:/` or `\\`") with unit tests in putio-sync-plugins/src/paths.rs
- [x] T003 Add `FieldType::PathMap`, `ConfigField.choices`, `Action` (`id` "`[a-z0-9_]{1,40}`, unique; ≤ 8 actions; label 1–40 chars", optional `hidden`), `tick_seconds` ("5–3600; default 60") and their checks in putio-sync-plugins/src/manifest.rs
- [x] T004 Allow config values that are arrays of `{from, to}` string tables in putio-sync-core/src/plugins.rs (validation, mask/unmask untouched) with a test
- [x] T005 Check `path_map` values against the manifest (type checks of config values) in putio-sync-plugins/src/service.rs `check_values`
- [x] T006 Share one store per plugin (`Arc<Mutex<Store>>` map in the service, handed to every worker) in putio-sync-plugins/src/store.rs, putio-sync-plugins/src/host.rs, putio-sync-plugins/src/worker.rs
- [x] T007 Add `putio.path`, `putio.download_dir`, `putio.metrics.count` ("≤ 16 names per plugin; extra names are refused with a Lua error") to putio-sync-plugins/src/host.rs, and `set_download_dir` / options in putio-sync-plugins/src/service.rs
- [x] T008 [P] Add `putio_sync_plugin_counter_total{plugin,name}` and `record_plugin_counter` in putio-sync-telemetry/src/plugins.rs
- [x] T009 Per-plugin tick interval: 5 s base ticker, `tick_due` per slot, in putio-sync-plugins/src/service.rs and the daemon ticker in putio-sync-cli
- [x] T010 `Job::Call` (run a named Lua function with args on a worker, return run + value) in putio-sync-plugins/src/worker.rs
- [x] T011 `ActionResult` parsing and checks (message ≤ 500, code ≤ 32, open_url http(s), choices ≤ 200, next action declared and after 1–30 s default 2, save keys known) and scrubbing in putio-sync-plugins/src/action.rs
- [x] T012 `PluginsService::action(name, id, input, overrides)` and `health(name)` on workers of their own, one call per plugin at a time (busy → error), in putio-sync-plugins/src/service.rs
- [x] T013 Host tests: actions, choices, health, ticks, counters, shared store, path API in putio-sync-plugins/tests/actions.rs
- [x] T014 Pass the download folder into the plugins service from the daemon in putio-sync-cli/src/commands/daemon.rs

## Phase 3: User Story 1 - New episodes show up in Plex within seconds (P1)

**Goal**: a finished download makes Plex rescan its folder, grouped per folder.
**Independent test**: mock Plex; 20 files in one folder → one refresh request for that folder in that library.

- [x] T015 [US1] Shared media code (pending folders, due rules, library match by longest folder, counters, waiting folders kept in the store across a reload) in putio-sync-plugins/bundled/mediaserver.lua
- [x] T016 [US1] Plex plugin manifest (fields, actions, `tick_seconds = 5`, hosts `plex.tv` + `server`) in putio-sync-plugins/bundled/plex/plugin.toml
- [x] T017 [US1] Plex refresh, events, ticks and test in putio-sync-plugins/bundled/plex/main.lua; register in putio-sync-plugins/src/discover.rs
- [x] T018 [US1] Tests: grouping, max wait, flush on sync.completed, retry then drop, skipped outside libraries, path mapping, token only in header in putio-sync-plugins/tests/mediaserver.rs

## Phase 4: User Story 2 - Sign in to Plex without copying a token (P1)

**Goal**: "Sign in with Plex" PIN flow saves the token.
**Independent test**: mock plex.tv; sign_in returns code + link + next; poll after approval saves `token`.

- [x] T019 [US2] `sign_in` / `sign_in_poll` actions with the stored client id in putio-sync-plugins/bundled/plex/main.lua
- [x] T020 [US2] `POST /plugins/action` (save through the settings path, 404/409/422) and `actions`/`choices` in `GET /plugins` in putio-sync-api/src/routes/system/plugins.rs
- [x] T021 [US2] API tests for the action route and the sign-in save in putio-sync-api/tests/plugins.rs
- [x] T022 [US2] Action buttons and the `next` loop (10 min limit) in putio-sync-api/ui/pages/settings.js with helpers in putio-sync-api/ui/plugins.js and tests in putio-sync-api/tests/ui/plugins.test.mjs
- [x] T023 [US2] Plex PIN flow test against mock plex.tv in putio-sync-plugins/tests/mediaserver.rs

## Phase 5: User Story 3 - Pick the server and libraries from a list (P2)

**Goal**: Load buttons fill Server and Libraries; path rules edited as rows.
**Independent test**: `load_servers` / `load_libraries` return choices from mocks.

- [x] T024 [US3] `load_servers`, `load_libraries` in putio-sync-plugins/bundled/plex/main.lua with tests in putio-sync-plugins/tests/mediaserver.rs
- [x] T025 [US3] Load buttons (select / ticks) and the `path_map` textarea (`from => to` lines) in putio-sync-api/ui/pages/settings.js and putio-sync-api/ui/plugins.js, with node tests

## Phase 6: User Story 4 - Jellyfin and Emby (P2)

**Goal**: the same refresh for Jellyfin and Emby with an API key.
**Independent test**: mock Jellyfin/Emby; due folders sent in one `/Library/Media/Updated` request with the right auth header.

- [x] T026 [P] [US4] Shared Jellyfin/Emby script and manifests in putio-sync-plugins/bundled/jellyfin/ and putio-sync-plugins/bundled/emby/; register in putio-sync-plugins/src/discover.rs
- [x] T027 [US4] Tests for both in putio-sync-plugins/tests/mediaserver.rs

## Phase 7: User Story 5 - Know it works: test button, doctor, metrics (P2)

**Goal**: health items in the doctor, test logs, counters.
**Independent test**: doctor Plugins check reports a fail item from a plugin's `health()`.

- [x] T028 [US5] `health()` and the test event in the three media plugins (putio-sync-plugins/bundled/*/main.lua)
- [x] T029 [US5] Doctor Plugins check calls `health` for enabled plugins (busy → "busy, skipped") in putio-sync-doctor/src/checks/plugins.rs with tests
- [x] T030 [US5] Secrets sweep: a signed-in Plex token never reaches a response, log or action answer in putio-sync-api/tests/secrets_sweep.rs (or the plugin secrets test)

## Phase 8: Polish

- [x] T031 [P] PLUGINS.md (host additions, media plugins), README.md, config.example.toml, example-config.toml
- [x] T032 [P] CHANGELOG.md entry; remove 031 from ROADMAP.md
- [x] T033 Check the UI bundle stays under 384 KB and run `make ci`

## Dependencies

Phase 2 blocks all stories. US1 → US2 (the plex plugin) → US3; US4 needs only Phase 2 and
T015; US5 needs the plugins from US1/US4. Polish last.

## Parallel examples

- T002, T008 together (different crates). T026 alongside T019–T025.

## Implementation strategy

MVP is Phase 2 + US1 (a pasted token works). Then sign-in, pickers, Jellyfin/Emby,
health, docs.
