# Tasks: Per-folder file rules

**Input**: Design documents from `specs/009-folder-rules/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/api.md, quickstart.md

**Tests**: Required (Constitution III and the task brief): unit tests for resolution, mockito tests for the manager and routes.

## Phase 1: Setup

- [X] T001 Create `putio-sync-core/src/rules.rs` with module docs and `pub mod rules;` in `putio-sync-core/src/lib.rs`

## Phase 2: Foundational (blocks every story)

- [X] T002 In `putio-sync-core/src/rules.rs`: `FolderRule { path, preset: Option<String>, allow: Vec<String>, block: Vec<String>, skip, delete_blocked, delete_after_download, skip_existing: Option<bool> }` and `Preset { only: Option<Vec<String>>, block: Vec<String> }` with serde (empty vecs and `None` skipped); built-in presets `video`, `audio`, `ebooks`, `no-binaries`, `everything` with labels and the lists from spec Assumptions
- [X] T003 In `putio-sync-core/src/rules.rs`: `normalize_path` (split on `/`, drop empty segments, replace `/ \ : * ? " < > |` with `_`; refuse empty, `.`, `..`), `normalize_extension` (strip leading `.`, lowercase; refuse empty, `/`, `\`, whitespace), preset name check `[a-z0-9][a-z0-9_-]{0,31}`, `validate(rules, presets)` refusing unknown preset, duplicate path, "only must be non-empty", both `only` and `block` set, naming the offending rule or preset
- [X] T004 In `putio-sync-core/src/rules.rs`: `RuleSet::from_config(&DownloadConfig)` (global filter + booleans, rules by normalised path, presets merged), `resolve(folder_path) -> Effective` per research R2/R3 with a source per part, `decide(folder_path, file_name) -> Decision` (`Download` / `Blocked { reason, rule }` / `Skipped { rule }`), `Reason` with a human `Display`
- [X] T005 Unit tests in `putio-sync-core/src/rules.rs`: no rules = today's blocklist; preset replaces; deeper rule wins; `allow` alone unblocks a global extension; `block` alone keeps the global list; preset + allow; no-extension files; case-insensitive and last-extension; skip and un-skip; per-setting inheritance with sources; validation errors; path normalisation
- [X] T006 In `putio-sync-core/src/config.rs`: `DownloadConfig.rules: Vec<FolderRule>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`) and `presets: BTreeMap<String, Preset>` (same), defaults, `DownloadConfig::validate` calls `rules::validate`; tests that a config with `[[download.rules]]` and `[download.presets.x]` loads, and a bad one is refused at load
- [X] T007 In `putio-sync-core/src/config.rs` `edit_table`: arrays of tables (document `[[...]]` edited in place by index, extra appended, surplus removed; a new non-empty array of tables written as `[[...]]`); test with `save_keeping_file_credentials` that adding, editing and removing a rule keeps the file's comments and `[auth]`
- [X] T008 [P] In `putio-sync-telemetry/src/labels.rs`, `lib.rs`, `sync.rs`: label enum `RuleDecision { Blocked => "blocked", Skipped => "skipped", Deleted => "deleted" }`, counter `rule_decisions_total{decision}`, `telemetry::sync::rule_decision(RuleDecision)`; help text of `files_blocked_total` now "blocked or skipped by a rule"; test the exposition
- [X] T009 [P] In `putio-sync-db/src/schema.rs`, `lib.rs`, new `putio-sync-db/src/decisions.rs`: `rule_decisions` table exactly as data-model.md (`decision TEXT NOT NULL CHECK(decision IN ('blocked', 'skipped', 'deleted'))`, `rule_path TEXT` NULL = global) + index on `decided_at`; `RuleDecisionsRepository { record, remove, recent(limit), clear }` via `db.rule_decisions()`; tests

**Checkpoint**: rules resolve and persist; nothing applies them yet.

## Phase 3: User Story 1 - Only download the right kind of file (P1) 🎯 MVP

**Goal**: The sync and manual queueing use folder rules for type filtering.

**Independent test**: mockito Put.io with mixed files and rules in the config; check the queue.

- [X] T010 [US1] In `putio-sync-download/src/manager.rs`: replace `DownloadConfig.blocked_extensions` with `rules: RuleSet` built in `From<&core DownloadConfig>`; fix every use (`putio-sync-api`, `putio-sync-daemon`, `putio-sync-cli`, tests)
- [X] T011 [US1] In `enqueue_tree` (`putio-sync-download/src/manager.rs`): `rules.decide(relative_path, name)`; blocked and skipped count as `files_blocked`; first time per process per file id: warn log with `file_id`, `name`, `folder`, `rule`, `reason`, both metrics, and `rule_decisions().record`; remove the decision row when a file is queued
- [X] T012 [US1] Tests in `putio-sync-download` (mockito): spec US1 scenarios 1–5 and "no rules = same as before"

## Phase 4: User Story 2 - Set and clear a rule from the Put.io page (P1)

**Goal**: API + UI to view the effective rule of a folder and set/clear its own rule.

**Independent test**: route tests; in `mock_ui`, set a preset on a folder and see it inherited by a child.

- [X] T013 [US2] `DownloadManager::putio_folder_path(folder_id)` in `putio-sync-download/src/manager.rs` (path of the folder itself, `""` for 0, error when it is a file)
- [X] T014 [US2] Make `update()` in `putio-sync-api/src/routes/system/settings.rs` `pub(crate)`; new `putio-sync-api/src/routes/system/rules.rs` with `GET /rules`, `GET/PUT/DELETE /rules/folder/{id}`, `POST /rules/delete` per contracts/api.md, every save followed by `download_manager.update_config`; nest at `/rules` in `putio-sync-api/src/lib.rs`
- [X] T015 [US2] Route tests in `putio-sync-api/src/routes/system/rules.rs` (mockito Put.io, temp config file): set, inherited effective with sources, clear, root refused, unknown preset 400 with nothing saved, comments kept
- [X] T016 [US2] New `putio-sync-api/ui/rules.js` (preset labels, effective rendering "this folder / from X / global", rule form) and a rule card + editor in `putio-sync-api/ui/pages/putio.js`; "blocked"/"skipped" badges on files of the open folder from its effective filter; CSS in `putio-sync-api/ui/app.css`

## Phase 5: User Story 3 - Delete unwanted files from Put.io (P2)

- [X] T017 [US3] In `enqueue_tree`: collect type-blocked files with effective `delete_blocked` that are not in the queue, one `delete_files` call, record `deleted`, metric `deleted`, `SyncStats.files_deleted`; `deleted` in the `POST /local/queue` response (`putio-sync-api/src/models/queue.rs`, `routes/local/queue.rs`)
- [X] T018 [US3] `DownloadManager::preview_rules(folder_id, &RuleSet)` and `POST /rules/preview` in `putio-sync-api/src/routes/system/rules.rs` (candidate rule replaces the folder's; first 200 items; nothing changed)
- [X] T019 [US3] Tests: deletes only blocked, never queued or skipped files; preview sends no delete (`putio-sync-download`, `putio-sync-api`)
- [X] T020 [US3] UI: Preview button in the rule editor; saving with delete on asks to confirm with the preview's count (`putio-sync-api/ui/pages/putio.js`)

## Phase 6: User Story 4 - Skip folder, per-folder delete-after-download and skip-existing (P2)

- [X] T021 [US4] In `putio-sync-download/src/manager.rs`: `download_file` uses the effective `skip_existing` of `relative_path`; `process_queue` uses the effective `delete_after_download` of the queue row's `relative_path`
- [X] T022 [US4] Tests: spec US4 scenarios 1–4 (`putio-sync-download`)

## Phase 7: User Story 6 - Know why a file was not downloaded (P2)

- [X] T023 [US6] `GET /rules/decisions`, `POST /rules/decisions/clear` in `putio-sync-api/src/routes/system/rules.rs` + tests
- [X] T024 [US6] "Recently blocked" card on `putio-sync-api/ui/pages/settings.js`

## Phase 8: User Story 5 - Rules overview and presets in Settings (P3)

- [X] T025 [US5] `PUT/DELETE /rules/presets/{name}` (409 when used, naming the rules) in `putio-sync-api/src/routes/system/rules.rs` + tests
- [X] T026 [US5] "Folder rules" (list + Remove) and "Presets" (edit, reset, delete, add) cards in `putio-sync-api/ui/pages/settings.js`

## Phase 9: Polish

- [X] T027 [P] `putio-sync-api/examples/mock_ui.rs`: temp config file with a comment and a `Shows` "video" rule, `Example.Show.S01E01.nfo` in Shows, a seeded decision for `setup.exe`; update its doc comment
- [X] T028 [P] `config.example.toml` and `README.md`: rules, presets, inheritance, delete blocked, preview, metric
- [X] T029 Run `TMPDIR=<scratchpad> make ci`; check UI bundle size stays under 200 KB (raised from 150 KB)
- [X] T030 Browser check against `mock_ui` per quickstart.md (desktop, 375 px, dark mode, console clean)

## Dependencies

- Phase 2 blocks everything. T010 → T011 → (T017, T021). T013 → T014 → (T016, T018, T023, T025). UI tasks after their routes.
- US1 is the MVP; US2 makes it usable from the UI; US3/US4/US6 are independent of each other once US1 and T014 exist; US5 last.

## Parallel opportunities

- T008 and T009 alongside T002–T007.
- T027 and T028 alongside T029 preparation.

## Implementation strategy

MVP = Phases 1–3 (rules from config apply to the sync). Then US2 (UI), then US3/US4/US6, then US5 and polish; `make ci` after each phase.
