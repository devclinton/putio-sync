# Tasks: Organise TV Shows on Put.io

**Input**: `specs/023-organise-tv-putio/` (plan, research, data-model, contracts, quickstart)

**Tests**: included; the constitution requires Put.io behaviour under a mock (Principle III).

## Phase 1: Setup

- [X] T001 Move `putio-sync-subtitles/src/release.rs` to `putio-sync-core/src/release.rs`, add `pub mod release;` in `putio-sync-core/src/lib.rs`, and replace `pub mod release;` with `pub use putio_sync_core::release;` in `putio-sync-subtitles/src/lib.rs`

## Phase 2: Foundational

- [X] T002 Add `Episode::parse` (show in its own case, year, season, first and last episode, title words) and `Release::show` for folder names to `putio-sync-core/src/release.rs`, with unit tests for every row of contracts/naming.md
- [X] T003 Add `putio-sync-core/src/organise.rs`: `OrganiseConfig` (`tv` false, `tv_folder` "TV Shows", `apply` preview|auto, `use_plugins` true), `OrganiseKind::Tv`, `Target`, `plan_target(config, name, folder)`, `join_folder`; validation (`tv_folder` must not start with `/` or contain `..`); unit tests
- [X] T004 Add `organise: OrganiseConfig` to `Config` in `putio-sync-core/src/config.rs` (serde default, skipped when default) and call its validation from `Config::validate`
- [X] T005 [P] Add `rename_file(id, name)` and `move_files(ids, parent_id)` to `putio-sync-client/src/files.rs`
- [X] T006 [P] Add the `organise_moves` table to `putio-sync-db/src/schema.rs` and `lib.rs`, and `putio-sync-db/src/organise_moves.rs` (`plan` insert-if-absent, `get`, `list`, `applied`, `undone`, `failed`, `remove`, `has`), with unit tests
- [X] T007 [P] Add `putio_sync_organise_moves_total{kind,result}` (`planned`, `applied`, `undone`, `failed`) to `putio-sync-telemetry` and its contract test

## Phase 3: User Story 1 — preview and apply (P1) 🎯 MVP

**Independent test**: a preview pass plans one move and makes no write call; apply makes the folder, rename and move calls.

- [X] T008 [US1] Write `putio-sync-download/tests/organise.rs`: preview pass plans with no rename/move; apply creates folders, renames, moves and marks applied; existing target name fails; download-mode target refused; gone file fails; non-episode not planned; dismiss removes
- [X] T009 [US1] Implement `Organiser` (`plan`, `apply`, `dismiss`, `list`, `configure`) and `OrganiseStep` (`ArrivedStep`, name `organise`) in `putio-sync-download/src/organise.rs`; export from `lib.rs`; make `hook_file`/`hooks_for` crate-visible in `manager.rs`
- [X] T010 [US1] Add `putio-sync-api/src/routes/system/organise.rs` (`GET /organise`, `POST /organise/apply|dismiss`, `GET/PUT /settings/organise`, preview), `AppState.organiser` + `with_organiser` in `lib.rs`, and the settings group in `settings.rs`
- [X] T011 [US1] Build the organiser in `putio-sync-cli/src/commands/daemon.rs`, register `OrganiseStep` after `Mp4Step`, attach it to the API state
- [ ] T012 [US1] Add `putio-sync-api/ui/organise.js` (settings form + moves list with tick boxes, Apply, Dismiss) registered in `putio-sync-api/src/ui.rs`, and an "Organise" button in `putio-sync-api/ui/pages/putio.js` that imports it on demand

## Phase 4: User Story 2 — undo (P1)

- [X] T013 [US2] Tests in `putio-sync-download/tests/organise.rs`: undo renames and moves back, marks undone; undo with the old folder gone fails and changes nothing; an undone move applies again
- [X] T014 [US2] Implement `Organiser::undo` and `POST /organise/undo`; Undo button in `ui/organise.js`

## Phase 5: User Story 3 — move at once (P2)

- [X] T015 [US3] Test: `apply = "auto"` moves in the pass; a Put.io error retries the step
- [X] T016 [US3] Apply in `OrganiseStep::run` when `apply = auto`; `Retry` on transient errors

## Phase 6: User Story 4 — find episodes already there (P2)

- [X] T017 [US4] Test: `scan` plans for known streamed files without a move, never twice
- [X] T018 [US4] Add `StreamFilesRepository::all` in `putio-sync-db/src/stream_files.rs`, `Organiser::scan`, `POST /organise/scan`, "Find episodes" button

## Phase 7: User Story 5 — plugins (P3)

- [X] T019 [US5] Test with a fake `SyncHooks`: a folder answer and a name answer replace the built-in target and set `by_plugin`; `use_plugins = false` asks nothing
- [X] T020 [US5] Ask the hooks in `Organiser::plan`

## Phase 8: Polish

- [X] T021 [P] API test `putio-sync-api/tests/organise.rs` (settings round trip, 503 without organiser, list) and node test `putio-sync-api/tests/ui/organise.test.mjs` for the pure helpers
- [X] T022 [P] `doctor` lint in `putio-sync-core/src/config_lint.rs`: `organise.tv` on with nothing streamed
- [X] T023 [P] Docs: `config.example.toml` `[organise]`, README stream section, PLUGINS.md (hooks also place streamed files), CHANGELOG Unreleased/Added, ROADMAP 023 marked done
- [X] T024 Run `make i18n`; translations via `make i18n-translate` if atranslate-cli exists
- [ ] T025 Run fmt, clippy and the tests of touched crates; confirm the eager UI is under 488 KiB

## Dependencies

T001 → T002 → T003 → T004; T005–T007 parallel after T001. US1 (T008–T012) needs Phase 2. US2–US5 need US1's `Organiser`. Polish last.

## Parallel examples

- T005, T006, T007 touch different crates.
- T021, T022, T023 touch different files.

## Implementation strategy

MVP is US1 + US2 (preview, apply, undo). US3–US5 are small additions to the same service.
