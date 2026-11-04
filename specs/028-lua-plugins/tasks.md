# Tasks: Lua Plugins

**Input**: Design documents from `specs/028-lua-plugins/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

Tests come first in each task where behaviour changes (Constitution III).

## Phase 1: Setup

- [X] T001 Create the `putio-sync-plugins` crate (`Cargo.toml` with core, telemetry, `mlua = { version = "0.11", features = ["lua54", "vendored", "serialize"] }`, tokio, reqwest, serde, serde_json, toml, tracing, chrono; mockito and tempfile for tests; `src/lib.rs`), add it to the workspace `Cargo.toml` and both crate lists in `Dockerfile`

## Phase 2: Foundational

- [X] T002 [P] `putio-sync-core/src/events.rs`, `events/schema.json`, `EVENTS.md`, `putio-sync-telemetry/src/hooks.rs` `EVENT_TYPES`, `putio-sync-api/ui/hooks.js` stays as is: add `plugin.test` (`PluginTest { plugin, message }`); `HookConfig::matches` and hook validation treat it like `hook.test`; schema and EVENTS.md tests pass
- [X] T003 [P] `putio-sync-telemetry/src/plugins.rs` + `lib.rs`: `putio_sync_plugin_runs_total{plugin,event,result}`, `putio_sync_plugin_run_duration_seconds{plugin}` (buckets 0.01–60 s), `putio_sync_plugin_events_dropped_total{plugin}`; global recorders `record_configured`, `record_run`, `record_dropped`; add to `tests/contract.rs`
- [X] T004 `putio-sync-core/src/plugins.rs` + `config.rs`: `PluginSettings` per data-model (`enabled` default false, `timeout_secs` 10 in 1..=60, `memory_mb` 32 in 4..=256, `config: BTreeMap<String, serde_json::Value>`, `secrets: BTreeMap<String, String>`, `approved_hosts`), `validate` (name rule, duplicates, ≤ 32, host syntax, ranges, values are scalars or string lists), `MASK` reuse, `mask`, `unmask(new, current)`; `Config.plugins_dir: Option<PathBuf>`, `Config.plugins: Vec<PluginSettings>` (skip when empty), `Config::plugins_dir_for(config_path)`; unit tests per rule and for mask/unmask

**Checkpoint**: settings, events and metrics tested on their own.

## Phase 3: User Story 2 - Write my own plugin (P1) 🎯 MVP core

**Goal**: a folder plugin loads, runs on events, and uses the host API.

- [X] T005 [US2] `putio-sync-plugins/src/manifest.rs`: parse and validate `plugin.toml` per `contracts/manifest.md` and data-model (every rule with a message naming file and field; `wasm` refused "needs 029"), `Manifest::hosts_needed(&PluginSettings)`, `check_values(&Manifest, &PluginSettings)` (types, required, http(s) URL, unknown key, secret in `secrets`), `config_value(...)` merging defaults; unit tests per rule
- [X] T006 [US2] `putio-sync-plugins/src/discover.rs`: bundled list (`include_str!` of `bundled/*/`) plus each folder under the plugins dir; a folder replaces a bundled plugin of the same name (logged), two folders with one name are both broken; a missing dir is no error; tests with temp dirs
- [X] T007 [US2] `putio-sync-plugins/src/sandbox.rs`: `new_state(memory_mb)` per research R2 (safe libs, remove `dofile`, `loadfile`, `collectgarbage`, `string.dump`, text-only `load`, `print` → log), deadline hook (every 1 000 instructions); tests: forbidden globals are nil, binary chunk refused, `while true do end` stops at the deadline, allocation past the limit is a memory error
- [X] T008 [US2] `putio-sync-plugins/src/store.rs`: JSON file store per research R6 (limits key 128 B, value 64 KiB, file 1 MiB; atomic write; in-memory when no dir); tests for limits, persistence across instances
- [X] T009 [US2] `putio-sync-plugins/src/host.rs`: install the `putio` table per `contracts/host-api.md` (log with fields and capture, config, http only with permission and approved host, store only with permission, time, json with nulls as nil, plugin, version, api_version); secret scrubbing of log lines and errors (≥ 4 chars); HTTP through `Handle::block_on` with `timeout(min(requested, remaining))`, same-host redirects, 1 MiB cut, errors as `"<host>: …"`; tests with mockito: approved request works, unapproved host and `file://` refused, no-permission tables absent, secret scrubbed from log
- [X] T010 [US2] `putio-sync-plugins/src/worker.rs` + `service.rs` + `lib.rs`: `PluginRun`, `RunResult`, `PluginState`, `PluginStatus`; `PluginsService::new(options { dir, data_dir, settings })`, `start()` (dispatcher from `events::subscribe()`, one thread per enabled loaded plugin, queue 1 000 drop oldest with `record_dropped`, `Lagged` counted, 60 s ticker for `has_tick`), `configure(settings)`, `reload()`, `status()`, `wants(EventType)`, `test(name)`, `check(settings) -> Result<Vec<PluginSettings>, String>` (manifest checks, approval of needed hosts for enabled ones); `init`/`on_event`/`on_tick` with timeout and memory error → rebuild; each run logged and counted; tests in `putio-sync-plugins/tests/service.rs`: store plugin records three event ids and keeps them across reload, filter by events, `plugin.test` only via test, broken plugin listed with error and others run

**Checkpoint**: user plugins run on bus events.

## Phase 4: User Story 3 - A broken plugin never breaks the sync (P1)

- [X] T011 [US3] Tests in `putio-sync-plugins/tests/limits.rs`: a looping `on_event` ends as a timeout within limit + 1 s and the next event runs; a memory hog fails and the state is rebuilt; an `error()` is a failure and later events run; a slow plugin's queue drops oldest and a second plugin is not delayed; `emit` never blocks; a load failure leaves the service running
- [X] T012 [US3] Wiring: `putio-sync-api/src/lib.rs` `AppState.plugins: Arc<PluginsService>` (unstarted from config in `new`) and `with_plugins`; `putio-sync-cli/src/commands/{daemon,server}.rs` build one service (dir from config path), `start()` it, pass to `AppState` and the doctor; server ticker polls transfers when `plugins.wants` a `transfer.*`; add the crate to api, cli, doctor `Cargo.toml`

## Phase 5: User Story 1 - Phone notification when a download lands (P1)

- [ ] T013 [US1] `putio-sync-plugins/bundled/ntfy/{plugin.toml,main.lua}`: server (url, default `https://ntfy.sh`), topic (required), token (secret), notify (string_list of event types, default the five), group_minutes (default 5), priority; messages per event type; sync.completed only on failure; grouping per research R7
- [ ] T014 [P] [US1] `bundled/discord` (webhook_url secret url, username), `bundled/telegram` (api_url default `https://api.telegram.org`, bot_token secret, chat_id), `bundled/pushover` (api_url default `https://api.pushover.net`, app_token and user_key secrets, form body), same messages and grouping
- [ ] T015 [US1] Tests in `putio-sync-plugins/tests/bundled.rs`: each bundled manifest validates and loads; ntfy against mockito: one event sends one message with the token header; ten events then a tick past the window send one more summary naming the count (clock set through the service's test clock); a secret never shows in runs; discord/telegram/pushover send the right body to mockito

## Phase 6: User Story 4 - Manage plugins from Settings (P2)

- [ ] T016 [US4] `putio-sync-api/src/routes/system/plugins.rs`: `GET /plugins`, `POST /plugins/test` (404, 409, 422), `POST /plugins/reload` per `contracts/api.md`, registered in `lib.rs`; tests in `putio-sync-api/tests/plugins.rs` including 503 in setup mode
- [ ] T017 [US4] Settings group `plugins` in `putio-sync-api/src/routes/system/settings.rs` (view masks `secrets`; PUT and preview unmask, `state.plugins.check` then write, `configure`, reload on a dir change), assistant groups and export/import masking (`putio-sync-core/src/assistant.rs`, `routes/system/assistant.rs`, `tests/settings_assistant.rs` GROUPS); tests in `putio-sync-api/tests/settings.rs`: masked view, mask keeps the value, bad value refused naming plugin and field, enabling records approved hosts, save is live
- [ ] T018 [P] [US4] `putio-sync-api/ui/plugins.js` (DOM-free: `stateLabel`, `hostsFor(plugin, values)`, `fromStatus`/`toSettings` for form rows incl. masked secrets, `runLabel`) with `tests/ui/plugins.test.mjs`; add to `putio-sync-api/src/ui.rs` assets
- [ ] T019 [US4] `putio-sync-api/ui/pages/settings.js`: Plugins group: Reload button, one card per plugin (name, version, source, state, description, events, "Contacts: hosts"), Enabled, config fields by type (secret as "set" + Replace), timeout, last run and Send test event with logged lines; keep the bundle under 384 KB

## Phase 7: User Story 5 - Doctor and monitoring (P3)

- [ ] T020 [US5] `putio-sync-doctor`: `CheckId::Plugins` in `report.rs` (ALL count), `checks/plugins.rs` (broken enabled → problem; needs approval, missing config, settings for an unknown plugin, last run failed/timed out in 24 h → warning; none enabled → ok), `Ctx.plugins`, `Doctor::with_plugins`; without a service it builds an unstarted one from the config; tests in `putio-sync-doctor/tests/plugins.rs`
- [ ] T021 [US5] Extend `putio-sync-api/tests/secrets_sweep.rs` with a plugin secret (settings view, export, `/plugins`, doctor, metrics, test-run logs)

## Phase 8: Polish

- [ ] T022 `PLUGINS.md` (guide: folder layout, manifest, host API from `contracts/host-api.md`, sandbox and limits, approval, bundled plugins, 029 note), README sections (plugins, metrics, HTTP API, doctor, settings tables), `config.example.toml`, `example-config.toml`, `CHANGELOG.md`, `ROADMAP.md` (028 reduced to the extension-point follow-up; 030 lists the bundled channels)
- [ ] T023 Merge `origin/main`, `TMPDIR=<scratch> make ci`, push

## Dependencies

- T001 → everything. T002, T003, T004 in parallel, before Phase 3.
- Phase 3 (T005 → T006 → T007/T008 → T009 → T010) is the runtime every later phase uses.
- US3 (T011, T012) after T010. US1 (T013–T015) after T010; T014 parallel to T013 once the
  grouping code in T013 is settled. US4 (T016–T019) after T012. US5 (T020, T021) after T012.
- T022, T023 last.

## Implementation strategy

MVP = Phases 1–4 (a user plugin runs safely in the daemon), then the bundled notifiers
(the 030 proof), then Settings, doctor and docs.
