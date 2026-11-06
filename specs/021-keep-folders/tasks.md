# Tasks: Keep folders out of empty-folder cleanup

**Input**: Design documents from `specs/021-keep-folders/`

**Tests**: Included (Constitution III; the cleanup walk is destructive).

## Phase 1: Foundational (blocks all stories)

- [x] T001 Add `KeepFn` callback (folder path → reason), `Keep::User(reason)`, `KeptFolder {id, path, reason}`, report fields `kept` and `kept_by_user`, and `EmptyFolderOptions.keep` in putio-sync-client/src/cleanup.rs; re-export from putio-sync-client/src/lib.rs
- [x] T002 Walk tests: rule-kept folder kept with reason, inherited for children, every kept empty folder listed (protected, too new, user), no keep settings = unchanged, in putio-sync-client/src/cleanup.rs

## Phase 2: User Story 1 - Keep one folder from the Put.io page (P1) 🎯 MVP

- [x] T003 [US1] Add `keep: Option<bool>` to `FolderRule` (counted by `is_empty`) and `keep: Sourced<bool>` to `Effective`, set in `RuleSet::apply`, with tests, in putio-sync-core/src/rules.rs
- [x] T004 [US1] Build the `KeepFn` from `RuleSet` (reason `kept by the rule on <path>`) in putio-sync-core/src/cleanup.rs; refresh on `configure`; log kept folders
- [x] T005 [US1] `RuleBody.keep`, `EffectiveView.keep`; call `state.cleanup.configure` after rule saves in putio-sync-api/src/routes/system/rules.rs
- [x] T006 [US1] Doctor empty-folders summary mentions `kept_by_user` in putio-sync-doctor/src/checks/putio.rs
- [x] T007 [US1] Add `keep` to FLAGS and a one-click Keep folder / Don't keep button on the rule card in putio-sync-api/ui/rules.js

## Phase 3: User Story 2 - Keep patterns in Settings (P2)

- [x] T008 [US2] `CleanupConfig.keep_patterns: Vec<String>` (default empty), validated with `pattern::check` in putio-sync-core/src/config.rs
- [x] T009 [US2] Match keep patterns against the folder and its ancestors (reason `matches keep pattern "<p>"`) in putio-sync-core/src/cleanup.rs, with tests
- [x] T010 [US2] `CleanupSettings.keep_patterns` (missing = `[]`), live field, in putio-sync-api/src/routes/system/settings.rs
- [x] T011 [US2] Keep patterns list field in the Put.io cleanup group in putio-sync-api/ui/settings/putio.js

## Phase 4: User Story 3 - See what was kept and why (P2)

- [x] T012 [US3] Remove empty folders preview names kept folders with reasons in putio-sync-api/ui/pages/putio.js
- [x] T013 [US3] `clean-empty` prints each kept folder and the user-kept count in putio-sync-cli/src/commands/clean_empty.rs
- [x] T014 [US3] Mock backend returns `kept` and rules with `keep` in putio-sync-api/examples/mock_ui.rs (no change needed: the mock runs the real cleanup and rules routes)

## Phase 5: Polish

- [x] T015 Document `keep` and `keep_patterns` in config.example.toml
- [x] T016 `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`; UI check against the mock backend
- [x] T017 CHANGELOG.md entry; remove 021 from ROADMAP.md

## Dependencies

T001 → everything. T003 → T004, T005, T007. T008 → T009, T010, T011. US3 needs T001 only.
UI tasks (T007, T011, T012) can go in parallel once their API tasks are done.

## Implementation strategy

MVP is Phase 1 + US1: a folder kept by a rule survives the cleanup and the doctor. US2 and
US3 add the settings list and the visible reasons.
