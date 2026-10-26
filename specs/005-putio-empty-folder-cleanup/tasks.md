# Tasks: Put.io Empty-Folder Cleanup

**Input**: Design documents from `/specs/005-putio-empty-folder-cleanup/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: Required (Constitution III; the task asks for tests against a mocked Put.io).

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Add `chrono = "0.4"` to `putio-sync-client/Cargo.toml`, `putio-sync-core/Cargo.toml` and `putio-sync-daemon/Cargo.toml`; add a `putio-sync-core` path dep to `putio-sync-daemon/Cargo.toml` (`CleanupTrigger` is re-exported from core, so no telemetry dep)

## Phase 2: Foundational

- [X] T002 [P] Add `CleanupTrigger { Schedule => "schedule", Manual => "manual" }` to `putio-sync-telemetry/src/labels.rs`; add families `empty_folder_runs_total{trigger,result}`, `empty_folders_deleted_total`, `empty_folder_delete_failures_total`, `empty_folder_last_success_timestamp_seconds` (label-less GaugeVec) to `putio-sync-telemetry/src/lib.rs` (register, zero-init every trigger × `SyncResult`)
- [X] T003 [P] Create `putio-sync-telemetry/src/cleanup.rs` with `Telemetry::{empty_folder_run, empty_folder_deleted, empty_folder_delete_failed}` and global free functions; extend `BASIC_FAMILIES` and add a recording test in `putio-sync-telemetry/tests/contract.rs`
- [X] T004 Add `CleanupConfig { enabled: false, run_at: "03:00", min_age_hours: 24 }` as `[cleanup]` (`#[serde(default)]`) to `putio-sync-core/src/config.rs`; `Config::validate` rejects `run_at` not 24h `HH:MM` and `min_age_hours > 8760`; `CleanupConfig::run_time() -> NaiveTime`; unit tests (defaults, missing section, bad values, save keeps comments)

## Phase 3: User Story 1 — Clean up by hand from the CLI (P1) 🎯 MVP

**Goal**: one walk that finds, reports and deletes empty folders, used by `files clean-empty`.

**Independent test**: client tests against `mockito`; CLI help/argument parsing.

- [X] T005 [US1] Tests first in `putio-sync-client/src/cleanup.rs` (`#[cfg(test)]`, `mockito`): nested empties removed with one delete of the top-most folder and all reported; folder with a file kept; protected id kept but its empty children removed; shared folder kept and not listed; too-new and unknown-age kept (parents too); unfinished-transfer `save_parent_id` kept; list error → failure, ancestors kept; delete error → failure; `cursor` → kept; dry run lists the same folders and sends no delete; transfer-list error fails the run before any delete
- [X] T006 [US1] Implement `EmptyFolderOptions`, `EmptyFolderReport`, `RemovedFolder`, `CleanupFailure` and `PutioApiClient::remove_empty_folders(&self, &EmptyFolderOptions)` in `putio-sync-client/src/cleanup.rs` (post-order walk, depth limit 64, `created_at` parsed as UTC `%Y-%m-%dT%H:%M:%S`, transfers not `COMPLETED`/`SEEDING` protect their `save_parent_id`); export from `putio-sync-client/src/lib.rs`
- [X] T007 [US1] Create `putio-sync-core/src/cleanup.rs`: `EmptyFolderCleanup { client, settings: RwLock<CleanupConfig + protected ids>, running: Mutex<()> }` with `from_config`, `configure(&Config)`, `config()`, `run(trigger, dry_run) -> Result<EmptyFolderReport, CleanupError>` (`try_lock` → `AlreadyRunning`; logs each removed folder with `folder_id`, `path`, `age_hours`, `dry_run`; summary log; metrics for non-dry runs); tests for guard and protected ids from config; export from `putio-sync-core/src/lib.rs`
- [X] T008 [US1] Add `FilesCommands::CleanEmpty { dry_run, yes }` to `putio-sync-cli/src/cli.rs`, dispatch in `putio-sync-cli/src/main.rs`, implement `putio-sync-cli/src/commands/clean_empty.rs` (dry run → print → confirm unless `--yes` → real run → print; non-zero exit on failures) and register in `putio-sync-cli/src/commands/mod.rs`; parsing test

## Phase 4: User Story 2 — Daily run in the daemon (P2)

**Independent test**: `is_due` unit tests with fixed times.

- [X] T009 [US2] Tests then `due_date(prev, now, run_at, last_run) -> Option<NaiveDate>` (the day the run is for) in `putio-sync-daemon/src/schedule.rs` (crossing today, across midnight, no catch-up, already ran today, DST gap and overlap) plus `run_daily(cleanup: Arc<EmptyFolderCleanup>)` loop ticking every 30 s, re-reading `enabled`/`run_at` each tick
- [X] T010 [US2] `PutioDaemon::with_cleanup(Arc<EmptyFolderCleanup>)` and spawn/abort the loop in `putio-sync-daemon/src/daemon.rs`; register module in `putio-sync-daemon/src/lib.rs`
- [X] T011 [US2] In `putio-sync-cli/src/commands/daemon.rs` build one `EmptyFolderCleanup` from the daemon's client and config, give it to the daemon and to `AppState::with_cleanup`; log the cleanup settings under `Daemon configuration:`

## Phase 5: User Story 4 — API endpoint (P3)

- [X] T012 [US4] `AppState.cleanup: Arc<EmptyFolderCleanup>` (built in `AppState::new`, replaced by `with_cleanup`) in `putio-sync-api/src/lib.rs`; route `POST /putio/cleanup/empty-folders` in new `putio-sync-api/src/routes/putio/cleanup.rs` (required `dry_run`, 400/409 per contracts/api.md) registered in `putio-sync-api/src/routes/putio/mod.rs`
- [X] T013 [US4] `putio-sync-api/tests/cleanup.rs`: 401 without token, 400 without `dry_run`, dry run returns report and no delete, real run deletes, 409 while a run holds the guard

## Phase 6: User Story 3 — Web UI button (P3)

- [X] T014 [US3] "Remove empty folders" button in the Put.io page toolbar in `putio-sync-api/ui/pages/putio.js`: dry run → "nothing to remove" toast or `confirm` with the count (and up to 5 paths) → real run → result toast → reload listing
- [X] T015 [US3] Check `putio-sync-api/examples/mock_ui.rs` already mocks every call the cleanup makes (it does: `Movies/Extras` is old and empty) and note the button in its docs; keep `putio-sync-api/tests/ui_assets.rs` passing

## Phase 7: User Story 5 — Settings (P4)

- [X] T016 [US5] `GET/PUT /settings/cleanup` in `putio-sync-api/src/routes/system/settings.rs` (view, `update`, `configure` the service, save message per engine); watcher PUT also calls `configure`; tests in `putio-sync-api/tests/cleanup.rs` (GET defaults, PUT saves and keeps comments, bad `run_at` 400 and file unchanged)
- [X] T017 [US5] "Put.io cleanup" card in `putio-sync-api/ui/pages/settings.js`

## Phase 8: Polish

- [X] T018 [P] `config.example.toml`: `[cleanup]` section per contracts/config.md
- [X] T019 [P] `README.md`: feature description, CLI usage, API endpoint, settings, metrics table rows, TZ note
- [X] T020 Run `TMPDIR=<scratchpad> make ci` and fix findings

## Dependencies

- T001 → everything. T002–T004 before US phases (T003 after T002).
- US1 (T005–T008) before US2, US4. US4 (T012) before US3 (T014) and US5 (T016).
- T018, T019 any time after T004.

## Parallel examples

- T002/T004 in parallel (different crates). T018/T019 in parallel.

## Implementation strategy

MVP = Phases 1–3 (CLI cleanup). Then daemon schedule, API, UI, settings, docs; `make ci`
after each phase.
