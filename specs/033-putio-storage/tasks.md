# Tasks: Put.io Storage Management

**Input**: Design documents from `specs/033-putio-storage/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/storage-api.md

**Tests**: included (constitution III and IV; deletion rules are tested before any route or UI uses them).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependencies)
- **[Story]**: US1 view, US2 warning, US3 free up space, US4 deleted list, US5 automatic run

## Phase 1: Setup

- [X] T001 Create crate `putio-sync-storage` (`Cargo.toml` with client, core, db, telemetry, tokio, serde, chrono, tracing, uuid, thiserror; dev: mockito, tempfile, serde_json), add it to workspace `members` in `Cargo.toml`, and add its manifest COPY, stub `src/lib.rs` and source COPY lines to `Dockerfile`
- [X] T002 [P] Raise the UI bundle cap to 384 KB (393,216 bytes) in `putio-sync-api/tests/ui_assets.rs`

## Phase 2: Foundational (blocks all stories)

- [X] T003 [P] `StorageConfig` (`warn_free_percent` 10, `target_free_percent` 20, `auto_free` false, `run_at` "02:30") with `validate()` (1 ≤ warn < target ≤ 90, `run_at` HH:MM) and `run_time()`, `Config.storage` (`#[serde(default)]`) validated in `Config::validate`, with tests (defaults when missing, partial section, bad levels, bad time) in `putio-sync-core/src/config.rs`; document `[storage]` in `config.example.toml`
- [X] T004 [P] Tables `storage_runs` and `storage_run_files` (+ index) in `putio-sync-db/src/schema.rs`, created in `putio-sync-db/src/lib.rs` (and in the migrate test's table list)
- [X] T005 `StorageRepository` in `putio-sync-db/src/storage.rs` (`record_run(trigger, files, free_after) -> id` in one transaction with its file rows, `recent_runs(limit)` with files, newest first), `db.storage()`; `DownloadRepository::completed()` (file_id, size, local_path for completed rows) in `downloads.rs`; the existing `QueueRepository::claimed()` (pending, downloading, paused) gives the queued ids; unit tests for each
- [X] T006 [P] Telemetry in `putio-sync-telemetry/src/storage.rs` + registration in `lib.rs`: gauges `putio_storage_size_bytes`, `putio_storage_used_bytes`; counters `storage_free_runs_total{trigger,result}`, `storage_freed_files_total`, `storage_freed_bytes_total`; global helpers; a test that the names appear in the exposition
- [X] T007 [P] `WalkedFile.is_shared` (serde-skipped) filled from the listing in `putio-sync-client/src/walk.rs`
- [X] T008 Planner unit tests first in `putio-sync-storage/src/plan.rs`: only files with a completed record of the same size and a present local file of that size; queued, shared (self or ancestor), and under-unfinished-transfer files excluded; folders excluded; oldest first by `created_at` then id, missing dates last; stops once the total reaches the needed bytes; empty when free ≥ target; shortfall when not enough; 1,000-file cap
- [X] T009 Implement `plan.rs`: `Inputs` (walked files, completed records map, active queue ids, unfinished save folder ids, local size lookup), `eligible()`, `select(candidates, needed) -> (files, total)` and target maths (`target_bytes`, `needed_bytes`)

## Phase 3: User Story 1 - See storage use and what fills it (P1) 🎯 MVP

**Goal**: the Put.io page shows used/free/total and the biggest top-level items.

**Independent test**: mock 80 GB used of 100 GB in three root folders; `GET /putio/storage` and the card show them largest first.

- [X] T010 [US1] `StorageService` in `putio-sync-storage/src/lib.rs`: `from_config(client, db, &Config)`, `configure(&Config)`, `config()`, `summary()` (account info + root listing, top 10, `low`, sets the gauges); `StorageError` (AlreadyRunning, NoDatabase, PlanExpired, Client, Db); unit test with mockito
- [X] T011 [US1] `AppState.storage` built in `AppState::new` and configured where cleanup is (`settings.rs` saves, `assistant.rs` import) in `putio-sync-api/src/lib.rs`; route `GET /putio/storage` in `putio-sync-api/src/routes/putio/storage.rs` (register in `routes/putio/mod.rs`, nest behind auth)
- [X] T012 [P] [US1] API tests in `putio-sync-api/tests/storage.rs`: summary numbers and order, 401 without a token; setup mode 503 for `/putio/storage` in `putio-sync-api/tests/setup_routes.rs`
- [X] T013 [US1] UI module `putio-sync-api/ui/storage.js` (`storageCard()`: bar, numbers, top items linking to `#/putio/<id>`, error state), embed it in `putio-sync-api/src/ui.rs`, mount it at the top of `putio-sync-api/ui/pages/putio.js`; bar styles in `putio-sync-api/ui/app.css`

## Phase 4: User Story 2 - Be warned when free space is low (P1)

**Goal**: the card, the Status page and the doctor warn below the level; metrics are exported.

**Independent test**: 95 GB used of 100 GB with warn 10%: card and Status warn, doctor check is a warning; 50 GB: nothing.

- [X] T014 [US2] Doctor storage check uses `[storage] warn_free_percent` (warning) and 2% free (problem), suggestion names Free up space; unit tests in `putio-sync-doctor/src/checks/putio.rs` (and wherever the doctor holds its config)
- [X] T015 [US2] Card warning with free amount and level and a "Free up space" button in `ui/storage.js`; `storageNotice()` on the Status page in `ui/pages/status.js` linking to `#/putio`
- [X] T016 [US2] Daemon storage loop `run_storage(service)` in `putio-sync-daemon/src/schedule.rs` (gauges every 15 minutes), started in `putio-sync-daemon/src/daemon.rs` via `with_storage`, wired in `putio-sync-cli/src/commands/daemon.rs` sharing the API's service

## Phase 5: User Story 3 - Free space by deleting old downloaded files (P2)

**Goal**: preview, confirm, delete exactly the previewed files.

**Independent test**: 95% full, five downloaded and two not; preview lists only downloaded ones oldest first to reach 20%; confirm deletes exactly those; cancel deletes nothing.

- [X] T017 [US3] Service tests in `putio-sync-storage/src/lib.rs` (mockito + temp db + temp files): `plan()` picks verified files; `free()` deletes only plan ids, skips a file gone from Put.io / size changed / local copy removed / newly queued / under a now-unfinished transfer, records failures of a failed delete batch, refuses unknown and expired plans and a second concurrent run, records nothing when nothing was deleted
- [X] T018 [US3] Implement `plan()` (walk 4 at a time, 120 s, transfers, db reads, planner, store latest plan 30 min) and `free(trigger, plan_id, ids)` (re-checks, batches of 100, free_after from account info, logging and metrics) in `putio-sync-storage/src/lib.rs`
- [X] T019 [US3] Routes `POST /putio/storage/plan` and `POST /putio/storage/free` with error mapping (409 running/expired, 503 no database, 400 bad body) in `putio-sync-api/src/routes/putio/storage.rs`; API tests in `putio-sync-api/tests/storage.rs` (plan lists only verified files; free deletes exactly them per mock; stale plan 409; empty ids 400)
- [X] T020 [US3] UI: "Free up space" runs the plan, shows the list (path, size, added, local path), totals and shortfall in the confirm dialog, confirm posts the plan and shows deleted/skipped/failed, then refreshes the card, in `ui/storage.js`
- [X] T021 [US3] CLI `files free-space [--dry-run] [-y]` in `putio-sync-cli/src/commands/free_space.rs`, registered in `commands/mod.rs`, `cli.rs` and the command dispatch

## Phase 6: User Story 4 - See what was deleted (P2)

**Goal**: every run that deleted files is listed with its files and in History.

**Independent test**: a run deleting two files shows in recent runs with both files; History shows both as deleted.

- [X] T022 [US4] `free()` records the run (`storage().record_run`) and History `deleted` rows; `runs(limit)` on the service; tests in `putio-sync-storage/src/lib.rs`
- [X] T023 [US4] Route `GET /putio/storage/runs` (`[]` without a database) and API test (run listed with files; History has deleted rows) in `putio-sync-api/tests/storage.rs`
- [X] T024 [US4] UI recent runs list with expandable files in `ui/storage.js`

## Phase 7: User Story 5 - Free space automatically (P3)

**Goal**: opt-in daily run when free space is below the warning level.

**Independent test**: auto on and below the level → a `schedule` run is recorded; above → nothing; off → nothing.

- [X] T025 [US5] `auto_run()` on the service (skips when off, when size unknown, or free ≥ warn level; else plan + free all, trigger `schedule`); tests in `putio-sync-storage/src/lib.rs`
- [X] T026 [US5] Daemon loop calls `auto_run()` at `run_at` using `due_date`, in `putio-sync-daemon/src/schedule.rs`; startup log line in `daemon.rs`
- [X] T027 [US5] Settings group `storage`: `StorageSettings` view/apply, GET/PUT/preview routes, `groups()` entry and `live_storage()` in `putio-sync-api/src/routes/system/settings.rs`; help strings in `putio-sync-api/src/routes/system/assistant.rs`; settings API test (save, invalid levels 400) in `putio-sync-api/tests/settings.rs`
- [X] T028 [US5] Settings page `storage` group in `putio-sync-api/ui/pages/settings.js`

## Phase 8: Polish

- [ ] T029 [P] README: a "Put.io storage" section (card, levels, free up space rules, auto run, CLI) in `README.md`
- [ ] T030 [P] `CHANGELOG.md` entry under Unreleased → Added; remove 033 from `ROADMAP.md`
- [ ] T031 Check the UI in a browser against a mocked backend (quickstart) and the bundle size test
- [ ] T032 Merge `origin/main`, run `TMPDIR=<scratchpad> make ci`, fix anything it finds, push

## Dependencies

- T001 → T008–T010. T003–T007 → T009/T010. T010 → T011 → T012/T013.
- US2 (T014–T016) after T010; US3 (T017–T021) after T009/T010; US4 after T018; US5 after T018 and T016.
- T029/T030 any time; T031 → T032 last.

## Analysis (2026-11-03)

`/speckit-analyze`: every FR maps to a task (FR-001–002 T010/T013, FR-003 T003/T027/T028,
FR-004 T014/T015, FR-005 T006/T016, FR-006 T008/T009, FR-007–009 T018/T020, FR-010 T022,
FR-011 T024, FR-012 T025/T026, FR-013 T017/T018, FR-014 T021, FR-015 T011/T012, FR-016
T006/T018, FR-017 T002). No CRITICAL or HIGH findings. MEDIUM, fixed: the plan's
1,000-file cap was missing from the spec (added to Edge Cases). LOW: the "Free up space"
button is always on the card and stands out when storage is low (T015/T020).
