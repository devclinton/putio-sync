# Tasks: Stream Mode with DLNA

**Input**: Design documents from `/specs/022-stream-mode-dlna/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: Included; the constitution (II, III) needs every behaviour covered by `make ci`.

## Phase 1: Setup

- [X] T001 Add `RuleDecision::Streamed => "streamed"` and a `Mp4Request { Requested, Ready, Gone, Failed }` label in putio-sync-telemetry/src/labels.rs, and `stream_mp4_request(result)` counter `putio_sync_stream_mp4_requests_total{result}` in putio-sync-telemetry/src/sync.rs

## Phase 2: Foundational

- [X] T002 Add `SyncMode { Download, Stream }` (serde lowercase, default Download) and `DownloadConfig.mode`, `DownloadConfig.stream_mp4` (default true) in putio-sync-core/src/config.rs
- [X] T003 Add `FolderRule.mode: Option<SyncMode>`, `Effective.mode: Sourced<SyncMode>`, `Decision::Stream { rule }` after skip/patterns/type filter, `RuleSet::uses_stream()`, with unit tests (account stream, rule stream, deeper rule download, skip wins, blocked still blocked) in putio-sync-core/src/rules.rs
- [X] T004 Fix every exhaustive `Decision` match (putio-sync-download/src/dry_run.rs, putio-sync-api/src/routes/system/rules.rs, putio-sync-download/src/manager.rs) so `Stream` is "not downloaded"

## Phase 3: User Story 1 - Keep media on Put.io (P1) 🎯 MVP

**Goal**: In stream mode the sync pass keeps files on Put.io and records them; the media server shows the synced folder.

**Independent test**: putio-sync-download/tests/stream_mode.rs

- [X] T005 [P] [US1] Add `CREATE_STREAM_FILES_TABLE` (`file_id INTEGER PRIMARY KEY`, `filename TEXT NOT NULL`, `folder_path TEXT NOT NULL`, `size INTEGER NOT NULL`, `pending TEXT NOT NULL`, `attempts INTEGER NOT NULL DEFAULT 0`, `last_error TEXT`, `first_seen TIMESTAMP DEFAULT CURRENT_TIMESTAMP`) in putio-sync-db/src/schema.rs and create it in putio-sync-db/src/lib.rs
- [X] T006 [P] [US1] `StreamFilesRepository` (`ids`, `add`, `pending(limit)`, `finish_step`, `retry_step`, `remove`, `get`) with tests in putio-sync-db/src/stream_files.rs; `Database::stream_files()`
- [X] T007 [US1] `ArrivedFile`, `StepResult`, `ArrivedStep` trait and `run_pending(db, steps, limit)` in putio-sync-download/src/arrived.rs
- [X] T008 [US1] In putio-sync-download/src/manager.rs: `TreeDecision::Streamed`, `SyncStats.files_streamed`, `honour_stream` (sync pass true; `enqueue_tree` from the UI and `enqueue_tree_report` for the *arr false, logging once), record new stream files, prune rows no longer streamed after a full pass, run pending steps (20 per pass), `set_arrived_steps`
- [X] T009 [US1] `DlnaConfig::putio_root()` defaulting to 0 and drop the "putio_folder required" validation in putio-sync-core/src/config.rs; use it in putio-sync-dlna/src/lib.rs and putio-sync-api/src/routes/system/media_server.rs
- [X] T010 [US1] Integration tests: stream pass queues nothing, records files, second pass adds nothing, switch back to download queues, arr path downloads anyway, blocked files still deleted in putio-sync-download/tests/stream_mode.rs
- [X] T011 [P] [US1] Storage free-up never picks a never-downloaded file: confirm with a test in putio-sync-storage/src/plan.rs

## Phase 4: User Story 2 - Per-folder mode (P1)

- [X] T012 [US2] Expose `mode` in rule JSON, effective JSON and the preview (`stream` count) in putio-sync-api/src/routes/system/rules.rs; preview counts streamed in putio-sync-download/src/manager.rs `preview_rules`
- [X] T013 [US2] Rule editor mode select (Inherit/Download/Stream) and effective mode display in putio-sync-api/ui/rules.js
- [X] T014 [US2] Test mixed rules (`Movies` stream, `Movies/Keep` download) in putio-sync-download/tests/stream_mode.rs

## Phase 5: User Story 3 - MP4 versions (P2)

- [X] T015 [P] [US3] `request_mp4(id)` (`POST /files/{id}/mp4`) with mockito test in putio-sync-client/src/files.rs; `Mp4Status::in_progress()`
- [X] T016 [US3] `Mp4Step` (video non-MP4, NOT_AVAILABLE → request; ready/in progress → done; 404 → done; other → retry) in putio-sync-download/src/arrived.rs
- [X] T017 [US3] Register `Mp4Step` when `download.stream_mp4` in putio-sync-daemon/src/daemon.rs
- [X] T018 [US3] Test: `.mkv` requested once, `.mp4`/`.srt` not, second pass none, off → none, failure retried in putio-sync-download/tests/stream_mode.rs

## Phase 6: User Story 4 - Settings warning (P2)

- [X] T019 [P] [US4] Pure helpers `streamUsed(download)`, `mediaServerGap(dlna)`, `fixMediaServer(dlna)` in putio-sync-api/ui/stream_mode.js with tests in putio-sync-api/tests/ui/stream_mode.test.mjs
- [X] T020 [US4] Mode select, MP4 checkbox and the warning with its one-click fix in putio-sync-api/ui/settings/downloads.js
- [X] T021 [US4] Config check warnings (stream without the media server/Put.io root; the arr category check is deferred: `[arr]` has no category folder paths, and the run logs "Stream mode ignored" instead) with tests in putio-sync-core/src/config_lint.rs

## Phase 7: Polish

- [X] T022 `make i18n` to refresh putio-sync-api/ui/i18n/en.json; other catalogs deferred (atranslate-cli not installed; missing keys fall back to English)
- [X] T023 Docs: README.md media-server and folder rules sections, config.example.toml
- [X] T024 CHANGELOG.md entry; remove 022 from ROADMAP.md
- [X] T025 `cargo fmt`, `cargo clippy --all-targets`, `cargo test`, UI tests

## Dependencies

T001–T004 block everything. US1 (T005–T011) before US2–US4. US3 needs T007. US4 is UI-only after T002.

## Parallel examples

- T005, T006 with T009 (different crates).
- T015 with T012/T013.
- T019 with T021.

## Implementation strategy

MVP is US1: stream mode for the whole account, visible on the TV. US2–US4 add per-folder
choice, MP4 requests and the Settings guidance.
