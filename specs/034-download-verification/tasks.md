# Tasks: Download Verification

**Input**: Design documents from `specs/034-download-verification/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: included; the constitution (III, IV) asks for tests of download integrity
against local mocks.

## Phase 1: Setup

- [X] T001 Add the four `[download]` keys to `DownloadConfig` in `putio-sync-core/src/config.rs`: `verify` (bool, default `true`), `verify_max_redownloads` (u32, default `2`, "0..=5"), `verify_read_limit_mb_per_sec` (u64, default `50`, "1..=10000"), `verify_follow_bandwidth_pause` (bool, default `true`), each with a serde default; validation errors name the key (`download.verify_max_redownloads must be 0..=5 (got 9)`); update every `DownloadConfig { .. }` literal in the workspace; add parse/validate tests

## Phase 2: Foundational

- [X] T002 [P] Add `verify.passed`, `verify.failed` and `verify.redownload` to `EventType`, `Payload`, `samples()` and `putio-sync-core/src/events/schema.json` per `contracts/events.md` (`schema_version` stays 1); fix the `ALL` count and the telemetry event-type list if it mirrors `EventType::ALL`
- [X] T003 [P] Database: columns `downloads.crc32 TEXT`, `downloads.verify TEXT`, `downloads.verified_at TIMESTAMP`, `download_history.verify TEXT`, `download_history.redownloads INTEGER`, `download_queue.redownload INTEGER NOT NULL DEFAULT 0`; tables `verify_runs` and `verify_problems` per `data-model.md`, in `putio-sync-db/src/schema.rs` and `putio-sync-db/src/lib.rs` migrate; models in `putio-sync-db/src/models.rs`
- [X] T004 Add `VerifyRepository` in `putio-sync-db/src/verify.rs` (start/update/finish run, prune to the latest 10 runs, stop rows left `running`, add problem, mark fixed by file id, list problems for the UI and doctor) plus `downloads().set_verified`, `history().set_verify`, queue `redownload` read/write; repository tests
- [X] T005 [P] Metrics in `putio-sync-telemetry/src/verify.rs` and the registry in `putio-sync-telemetry/src/lib.rs`: `putio_sync_verify_total{trigger,method,result}`, `putio_sync_verify_redownloads_total{trigger}`, `putio_sync_verify_run_files_total{result}`, `putio_sync_verify_read_bytes_total`, `putio_sync_verify_run_active`
- [X] T006 `Crc32` digest newtype, `crc32_prefix(path, len, on_progress)` and `valid_crc32` (8 hex digits after trim) in `putio-sync-download/src/files.rs`, with tests (known value, prefix + rest equals whole)

## Phase 3: User Story 1 - Every new download is checked (P1) 🎯 MVP

**Goal**: fresh and resumed downloads are compared with Put.io's CRC32 before the rename, re-downloaded on a mismatch, and fail with `integrity` when the limit is spent.

**Independent test**: a mock serving bad bytes first and good bytes second yields two downloads, one file and one `download.finished` after `verify.passed`.

- [X] T007 [US1] Hash in the disk writer task of `stream_to_temp` (`Option<Crc32>` in, `crc32: Option<String>` in `StreamDownloadResult`) in `putio-sync-download/src/stream.rs`; update `stream_download` and every caller
- [X] T008 [US1] In `DownloadManager::transfer` (`putio-sync-download/src/manager.rs`) seed each attempt's digest from the hidden file's prefix when it resumes (phase `verifying` with progress, read bytes counted), return attempts and CRC32
- [X] T009 [US1] Replace 040's resumed-file whole-file re-read in `download_file` with the verify step and re-download loop of research R3: `verify.failed`, `verify.redownload`, fail message `Checksum mismatch: … after N downloads`, `verify_problems` row (`source = download`, `result = failed`), metrics; `verify.passed` just before `download.finished`; history `verify`/`redownloads` and downloads `crc32`/`verify`/`verified_at`; mark earlier problems of the file fixed on a pass; logs use structured fields (file_id, name, method, expected, actual)
- [X] T010 [US1] Honour the queue `redownload` flag: `DownloadFile.overwrite` (from the queue row) makes the existing-file choice Overwrite with no checksum read, in `putio-sync-download/src/manager.rs` and every `DownloadFile { .. }` literal
- [X] T011 [US1] Tests in `putio-sync-download/tests/verify.rs`: mismatch then match, always mismatch (`integrity`, attempts = max+1, nothing at the real name), `verify_max_redownloads = 0`, no CRC32 (size), verification off, resumed prefix hash, event order (`verify.passed` before `download.finished`), no `verifying` phase for a fresh download, no token in event or problem text (FR-016), and with `delete_after_download` a file that keeps failing is never deleted from Put.io (FR-018)

## Phase 4: User Story 2 - Verify everything (P2)

**Goal**: a pausable, rate-limited run over downloaded files that queues mismatches.

**Independent test**: good, corrupted and deleted files give `verified`, `mismatch` (queued) and `missing`.

- [X] T012 [US2] `VerifyRunner` in `putio-sync-download/src/verify.rs` per research R5: scope everything or paths, report only, one walk plus `get_file_details` fallback, result classes, read limiter (`verify_read_limit_mb_per_sec` × 1 000 000 B/s), waits while the shared limiter `is_paused()` and `verify_follow_bandwidth_pause`, pause/resume/stop via `watch`, run row updates at most once a second, events with `trigger = "run"` (no `verify.passed`), metrics; owned by `DownloadManager` with `verify()` accessor; startup marks stale `running` rows `stopped`
- [X] T013 [US2] Re-queue a mismatch in `putio-sync-download/src/verify.rs`: queue row with the relative folder from `local_path`, `save_as` when the saved name differs, `redownload = 1`
- [X] T014 [US2] Tests in `putio-sync-download/tests/verify_run.rs`: the three classes, report only, not on Put.io, busy, pause holds reads, stop ends `stopped`, a second start is refused, paths scope, read rate within 10% of a small limit (SC-003)
- [X] T015 [US2] Routes `GET /verify`, `POST /verify`, `POST /verify/pause|resume|stop` per `contracts/api.md` in `putio-sync-api/src/routes/system/verify.rs`, registered in the router and the API route list; setup mode 503; API tests

## Phase 5: User Story 3 - See what verification found (P2)

- [ ] T016 [US3] `verify` and `redownloads` on history rows in `putio-sync-api/src/routes/system/history.rs` (and the history model/serialiser)
- [ ] T017 [US3] Doctor check `CheckId::Verification` ("verification", "Download verification", Area::Sync) in `putio-sync-doctor/src/checks/verify.rs`, `checks/mod.rs`, `report.rs` per research R7; fix tests that count checks
- [ ] T018 [US3] Queue page "Verifying" phase and 039 pipeline state in `putio-sync-api/ui/pages/queue.js`, `putio-sync-download/src/pipeline.rs`, `putio-sync-api/ui/pages/active.js`
- [ ] T019 [P] [US3] DOM-free helpers in `putio-sync-api/ui/verify.js` (method label, summary line, progress percent, result labels) with node tests in `putio-sync-api/ui/verify.test.js`; asset list in `putio-sync-api/src/ui.rs`
- [ ] T020 [US3] History page: "Verified" column and the Verification panel (summary, Verify everything, Report only, Pause, Resume, Stop, 2 s polling while running, problem list) in `putio-sync-api/ui/pages/history.js` and `putio-sync-api/ui/api.js`
- [ ] T021 [US3] Local files (045) "Verify" bulk action on selected file rows in `putio-sync-api/ui/pages/files.js`
- [ ] T022 [US3] Settings › Download fields for the four keys in `putio-sync-api/ui/pages/settings.js` and `/settings/download` in `putio-sync-api/src/routes/system/settings.rs`

## Phase 6: User Story 4 - Hooks, plugins and monitoring (P3)

- [X] T023 [US4] Document the three events in `EVENTS.md` (fields, examples, `trigger`), keeping schema version 1; the docs/schema consistency test passes
- [ ] T024 [US4] Test that a bus subscriber receives `verify.failed` with the documented fields and that `/metrics` shows the `putio_sync_verify_*` series, in `putio-sync-download/tests/verify.rs` and the telemetry tests

## Phase 7: Polish

- [ ] T025 [P] README (Download verification section; HTTP API, metrics, settings and doctor tables), `config.example.toml`, `example-config.toml`
- [ ] T026 [P] `CHANGELOG.md` Unreleased › Added (and Changed: a resumed file's check reads only its prefix)
- [ ] T027 Remove 034 from `ROADMAP.md`
- [ ] T028 Merge `origin/main`, then `TMPDIR=<scratch> CARGO_PROFILE_RELEASE_STRIP=none make ci`; UI bundle under 384 KB

## Dependencies

- T001 → everything. T002–T006 before US1. T003/T004 before T009, T012.
- US1 (T007–T011) before US2 (the runner reuses the verify helpers and queue flag).
- US3 UI (T019–T021) after T015; T017 after T004.
- US4 after T002 and T005.

## Parallel examples

- T002, T003, T005 touch different crates.
- T019 alongside T016–T018.
- T025, T026 together at the end.

## Implementation strategy

MVP is US1: every new download is verified and re-downloaded. Then the run (US2), the
views (US3), and docs/tests for integrations (US4).
