# Tasks: More plugin hooks and events (057)

**Input**: `specs/057-plugin-hooks-events/` (spec, plan D1–D9) and the 057 entry in `ROADMAP.md`.
**Tests**: included; each new event has a test that triggers it (FR-029). No network in tests.
**Format**: `- [x] T### [P] [US#] description with file path`.

## Phase 1: Foundational (blocks all stories)

- [x] T001 Add the 20 event types to `EventType` (`ALL`, `as_str`), their payload structs, `Payload` variants and one sample each in `putio-sync-core/src/events.rs`; add `is_opt_in` (`download.progress`) and `matched_by`, used by hooks and plugins for `"*"`
- [x] T002 [P] Add the same types to `putio-sync-core/src/events/schema.json`; the unit tests validate every sample against it
- [x] T003 [P] Add a section per type to `EVENTS.md` (fields table and example); `putio-sync-core/tests/events_doc.rs` keeps doc, samples and schema in step
- [x] T004 [P] Add the types to `EVENT_TYPES` in `putio-sync-telemetry/src/hooks.rs` and the labels to `EVENTS` in `putio-sync-api/ui/hooks.js`
- [x] T005 Match `download.progress` by name only in `putio-sync-core/src/hooks.rs` (`wants`), `putio-sync-hooks/src/service.rs` and `putio-sync-plugins/src/manifest.rs` (`wants`)

**Checkpoint**: `cargo test -p putio-sync-core` passes; the event picker lists 36 types.

## Phase 2: User Story 1 - Know when an item is fully ready (P1) 🎯 MVP

- [x] T006 [US1] Send `pipeline.finished` once per item from `putio-sync-pipeline/src/runner.rs` (success, warnings, skipped), with folder, files under it, warnings and duration; store helpers in `putio-sync-pipeline/src/store.rs`
- [x] T007 [US1] Tests in `putio-sync-pipeline/tests/plugin_steps.rs`: one event per item for success, warnings and a `pre_download` skip; none while an item waits for the operator
- [x] T008 [US1] `putio-sync-plugins/bundled/mediaserver.lua`: refresh an item's folder on `pipeline.finished`, hold its files until then, `max_wait_seconds` as the way out, `wait_for_pipeline` setting; subscribe plex, jellyfin and emby (`plugin.toml`) to `pipeline.finished` and `daemon.started`
- [x] T009 [US1] Tests in `putio-sync-plugins/tests/mediaserver.rs`: one refresh per item under the pipeline; per-folder grouping when `wait_for_pipeline` is off

**Checkpoint**: a season pack gives one `pipeline.finished` and one refresh (SC-001).

## Phase 3: User Story 2 - Write a pipeline step as a plugin (P1)

- [x] T010 [US2] `putio-sync-core/src/step_plugins.rs`: `StepPlugins`, `StepAnswer`, `parse_answer` with unit tests
- [x] T011 [US2] `plugin` on `HookConfig`, `HookKind::Plugin`, validation (one target; `stage` needed; a valid plugin name) and tests in `putio-sync-core/src/hooks.rs`
- [x] T012 [US2] Run a plugin step in `putio-sync-pipeline/src/runner.rs` through `Pipeline::set_step_plugins` (`putio-sync-pipeline/src/lib.rs`): done, skip (`pre_download` only), failed, timeout, with the usual required/optional rules
- [x] T013 [US2] Tests with a fake `StepPlugins` in `putio-sync-pipeline/tests/plugin_steps.rs`
- [x] T014 [US2] `step = true` in `putio-sync-plugins/src/manifest.rs`; `on_item` call on the plugin's queue in `putio-sync-plugins/src/worker.rs`, `service.rs` and the new `steps.rs`; run kind `item` in `putio-sync-telemetry/src/plugins.rs`
- [x] T015 [US2] Tests in `putio-sync-plugins/tests/lifecycle.rs`: done, skip, fail, error, timeout, an odd return; a plugin that is missing, off, broken or has no `on_item` is named
- [x] T016 [P] [US2] Doctor: a pipeline step whose plugin cannot run is a warning, in `putio-sync-doctor/src/checks/plugins.rs` with a test in `putio-sync-doctor/tests/plugins.rs`
- [x] T017 [US2] Hook editor: **Runs**: Plugin for a pipeline step in `putio-sync-api/ui/hooks.js` (`kindChoices`, `fromSettings`, `toSettings`, `targetLabel`) and `putio-sync-api/ui/settings/integrations.js`; test in `putio-sync-api/tests/ui/hooks.test.mjs`
- [x] T018 [US2] Hand the plugins service to the pipeline in `putio-sync-cli/src/commands/daemon.rs`
- [x] T019 [P] [US2] Example `examples/plugins/small-item-step/` (manifest, script, README) with a test in `putio-sync-plugins/tests/examples.rs`

**Checkpoint**: a plugin step skips or passes items with no shell script (SC-002).

## Phase 4: User Story 3 - Hear about trouble without watching the UI (P2)

- [x] T020 [US3] `putio-sync-core/src/health.rs`: `Outage`, `Once`, `Edge`, `PutioWatch`, with unit tests
- [x] T021 [US3] `observe_attempts` on the Put.io client in `putio-sync-client/src/client.rs` (exported from `lib.rs`), called after each attempt
- [x] T022 [US3] `putio.unreachable`, `putio.recovered` and `auth.expired` from `PutioWatch`; test against mockito in `putio-sync-core/tests/putio_health.rs` (one of each per outage; no token in the JSON); attach it in `putio-sync-cli/src/commands/daemon.rs`
- [x] T023 [P] [US3] `storage.recovered` on the way up in `putio-sync-storage/src/lib.rs`; test in `putio-sync-storage/src/tests.rs`
- [x] T024 [P] [US3] `local_disk.low` / `local_disk.recovered` from the Disk space check in `putio-sync-doctor/src/lib.rs`; test in `putio-sync-doctor/tests/run.rs`
- [x] T025 [P] [US3] `arr.unreachable` / `arr.recovered` per server in `putio-sync-arr/src/service.rs` (URL cut to scheme, host and port); test in `putio-sync-arr/tests/service.rs`
- [x] T026 [US3] `transfer.stalled` and `doctor.problem`: covered by the existing `transfer.stuck` and `doctor.problem`; no new type (see plan)

**Checkpoint**: an outage of any length gives one unreachable and one recovered (SC-003).

## Phase 5: User Story 4 - Follow each part of the work (P3)

- [x] T027 [US4] `sync.started` (same `started_at` as its `sync.completed`), `download.started` and the `download.progress` ticker (`set_progress_event_period`) in `putio-sync-download/src/manager.rs`
- [x] T028 [US4] `cleanup.deleted` after both delete-after-download paths in `putio-sync-download/src/manager.rs`
- [x] T029 [US4] Tests in `putio-sync-download/tests/activity_events.rs`: start before finished or failed and none for skips; a resumed start; progress throttled and not matched by `"*"`; the sync pair; both cleanup paths
- [x] T030 [P] [US4] `subtitles.found` / `subtitles.missing` in `putio-sync-pipeline/src/subtitles.rs`; test in `putio-sync-pipeline/tests/subtitles.rs`
- [x] T031 [P] [US4] `translation.finished` / `translation.failed` in `putio-sync-pipeline/src/translate.rs` and `callback.rs` (done, failed, callback done, callback failed, callback timeout); tests in `putio-sync-pipeline/tests/translate.rs` and `translate_callback.rs`
- [x] T032 [US4] `archive.extracted` / `archive.failed`: not built; putio-sync has no archive extraction (see plan)

## Phase 6: User Story 5 - React to the daemon and its settings (P3)

- [x] T033 [US5] `changed_sections` in `putio-sync-core/src/restart.rs` with a test that no value appears
- [x] T034 [US5] `daemon.started`, `daemon.stopping` (uptime) and `config.changed` (after the change is applied) in `putio-sync-cli/src/commands/daemon.rs`
- [x] T035 [US5] `on_config_changed(config)` after a plugin's own settings are saved and `on_unload()` before a reload, a turn-off and a stop, bounded by the plugin's time limit, in `putio-sync-plugins/src/worker.rs` and `service.rs` (`shutdown`); the daemon waits for it on stop
- [x] T036 [US5] Tests in `putio-sync-plugins/tests/lifecycle.rs`: `on_unload` on reload, turn-off and stop; bounded when it spins; skipped for a broken state; `on_config_changed` once, for that plugin only
- [x] T037 [P] [US5] Example `examples/plugins/batch-digest/` (manifest, script, README) with a test in `putio-sync-plugins/tests/examples.rs`

## Phase 7: Polish

- [x] T038 [P] `PLUGINS.md`: Pipeline steps, Lifecycle callbacks, **Wait for the pipeline** for the media servers; `examples/plugins/README.md`
- [x] T039 [P] `EVENTS.md`: the plugin target under Pipeline step hooks; `README.md`: the new events, plugin steps, metrics kinds, the doctor row
- [x] T040 [P] `make i18n` and the 21 new UI strings in the 26 catalogs under `putio-sync-api/ui/i18n/`
- [x] T041 `CHANGELOG.md` under Unreleased; mark 057 done in `ROADMAP.md`
- [x] T042 Run the checks: `cargo fmt --all --check`, both clippy runs, both test runs, `node --test putio-sync-api/tests/ui/*.test.mjs`, `node scripts/i18n-extract.mjs --check`
