# Tasks: Configuration Assistant

**Input**: Design documents from `specs/015-config-assistant/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/settings-assistant-api.md

**Tests**: included; the constitution (Principle III) requires tested behaviour and the plan
names the test files.

## Phase 1: Setup

- [X] T001 Add `pub mod assistant;` with a module doc to `putio-sync-core/src/lib.rs` and create `putio-sync-core/src/assistant.rs`; make `settings_table` `pub(crate)` in `putio-sync-core/src/config.rs`

## Phase 2: Foundational

- [X] T002 In `putio-sync-api/src/routes/system/settings.rs`, move each PUT's closure body into `apply_download`, `apply_watcher`, `apply_logging`, `apply_notifications`, `apply_cleanup`, `apply_precheck` (`fn(body, &mut Config) -> Result<(), ApiError>`, enum parsing included), and the live-field logic into `live_<group>(&AppState) -> (&'static [&'static str], &'static str)`; PUT handlers call both and keep their messages and side effects unchanged
- [X] T003 Run `cargo test -p putio-sync-api --test settings` to confirm the refactor changes no PUT behaviour

## Phase 3: User Story 1 - Preview before saving (P1) 🎯 MVP

**Goal**: each card shows what a save would change, whether it would be refused, and what needs a restart.

**Independent test**: preview a Workers change and a missing watch folder; the file is unchanged.

- [X] T004 [US1] Add `Change { key, before, after, live }` (Serialize) to `putio-sync-core/src/assistant.rs`
- [X] T005 [US1] Add `preview()` and `POST /settings/{group}/preview` for the six groups in `putio-sync-api/src/routes/system/settings.rs`: clone the config under a read lock, `apply_<group>`, `validate`; return `{ valid, error?, changes, when, risks }` (`changes` from the group view diff with `live` from `live_<group>`), never write
- [X] T006 [P] [US1] Integration tests in `putio-sync-api/tests/settings_assistant.rs`: preview lists exactly the changed fields with before/after/live; a refused value gives `valid:false` with the PUT's error text; no change gives empty `changes`; the config file and running config are unchanged after previews; malformed body is 400; needs the token
- [X] T007 [US1] Add `previewSettings(group, body)` to `putio-sync-api/ui/api.js`
- [X] T008 [US1] In `putio-sync-api/ui/pages/settings.js` `form()`: debounce (400 ms) a preview on input/change, render a preview panel (changes `field before → after` with an "applies now"/"needs a restart" badge, the refusal reason, or "Nothing would change") above the form footer, sending the same body as Save (precheck without `effective`); add CSS in `putio-sync-api/ui/app.css`

## Phase 4: User Story 2 - Risk warnings (P1)

**Goal**: risky combinations are flagged in previews, on the Settings page and in the doctor.

**Independent test**: delete-after-download with a `/tmp` download folder warns in all three places; the default config has no warnings.

- [X] T009 [US2] Implement `Risk { id, level, keys, message }`, `Level { Danger, Warning }` (serialised `danger`/`warning`), `Host { in_container, root_dev }`, `risks(&Config)` and `risks_in(&Config, &Host)` in `putio-sync-core/src/assistant.rs` with the seven rules of research R4 (nearest existing parent for missing folders; canonicalize when possible); danger first, then by id
- [X] T010 [P] [US2] Unit tests in `putio-sync-core/src/assistant.rs` for each rule firing and not firing, the default config giving none, and the container rule using a fake `Host`
- [X] T011 [US2] Include `risks` (of the candidate config) in the group preview response in `putio-sync-api/src/routes/system/settings.rs`; test in `putio-sync-api/tests/settings_assistant.rs`
- [X] T012 [US2] Add the risks of the running config to the doctor config check in `putio-sync-doctor/src/checks/local.rs` (each as an item, Warning status, the lint and the risks together); unit test there
- [X] T013 [US2] Show the preview's risks in the preview panel and a warnings banner (from `GET /settings/assistant` `risks`, refreshed after each save) at the top of the Settings page in `putio-sync-api/ui/pages/settings.js`

## Phase 5: User Story 3 - What each setting affects (P2)

**Goal**: each field says what it affects and whether it applies now.

**Independent test**: every field of the six groups has an "Affects" line and a badge.

- [X] T014 [US3] Add the `AFFECTS` table (group → field → text) and `GET /settings/assistant` returning `groups.<group>.{affects, live, when, defaults}` and `risks` in new `putio-sync-api/src/routes/system/assistant.rs`, merged into the `/settings` router
- [X] T015 [P] [US3] Test in `putio-sync-api/tests/settings_assistant.rs` that every field of every group view has an `affects` entry, `live` matches the engine, and the response holds no token
- [X] T016 [US3] Load `/settings/assistant` once in `putio-sync-api/ui/pages/settings.js` and show an "Affects …" line with the applies-now / restart badge under each field

## Phase 6: User Story 4 - Reset to defaults (P2)

**Goal**: a Defaults button fills a card with its defaults without saving.

**Independent test**: Logging → Defaults shows `info` and previews the differences; reload shows saved values.

- [X] T017 [US4] Include `defaults` (each group view of `Config::default()`; precheck settings only) in `GET /settings/assistant` in `putio-sync-api/src/routes/system/assistant.rs`; test the logging defaults in `putio-sync-api/tests/settings_assistant.rs`
- [X] T018 [US4] Add a Defaults button to each group form in `putio-sync-api/ui/pages/settings.js` that rebuilds the form from the defaults (keeping non-view fields) and runs the preview

## Phase 7: User Story 5 - Export and import (P3)

**Goal**: back up and restore settings without credentials.

**Independent test**: export, change Workers, import: the preview lists `download.workers`; apply restores it; the export has no secrets.

- [X] T019 [US5] Implement `export_toml(&Config) -> CoreResult<String>` (header comment + `settings_table`), `Import { config, ignored, unknown }`, `import(current: &Config, text: &str) -> Result<Import, ImportError>` (strip and report `auth`, `arr`, `api.auth_token`; lint unknown keys with `config_lint::lint`; deep-merge over the current settings table; restore current `auth`, `arr`, `api.auth_token`; `expand_paths`; parse errors carry only message and line), and `diff(&Config, &Config) -> Vec<Change>` over flattened dotted keys in `putio-sync-core/src/assistant.rs`
- [X] T020 [P] [US5] Unit tests in `putio-sync-core/src/assistant.rs`: export has no `auth`/`arr`/`auth_token`; export then import of the same config gives no changes; a partial file changes only its keys; ignored and unknown are reported; a parse error message holds no file text
- [X] T021 [US5] Add `GET /settings/export` (text/plain, attachment) and `POST /settings/import` (`{toml, apply}`, 1 MB limit; preview or apply via `save_keeping_file_credentials`, then update the download manager, doctor and cleanup; info log with the change count) in `putio-sync-api/src/routes/system/assistant.rs`
- [X] T022 [P] [US5] Integration tests in `putio-sync-api/tests/settings_assistant.rs`: export has no secrets; import preview writes nothing; apply writes the file keeping comments and credentials; refused import is 400 and writes nothing; invalid TOML is 400; oversized body is 400
- [X] T023 [US5] Add `exportSettings()` and `importSettings(toml, apply)` to `putio-sync-api/ui/api.js` and a "Backup and restore" card in `putio-sync-api/ui/pages/settings.js` (Export downloads the file; Import from a file or pasted text, preview, Apply after confirm, reload the cards)

## Phase 8: Polish

- [X] T024 [P] Document the assistant (preview, warnings, Defaults, Backup and restore) in `README.md` under the settings section
- [X] T025 [P] Add a `CHANGELOG.md` entry for 015 and remove the 015 entry from `ROADMAP.md`
- [X] T026 Check the UI size with `cargo test -p putio-sync-api --test ui_assets` (cap 300 KB) and `node --check` on changed modules
- [ ] T027 Merge `origin/main`, run `TMPDIR=<scratchpad> make ci`, fix findings, commit and push

## Dependencies

- T001 → T004, T009, T019. T002 → T005 → T011, T014. T014 → T016, T017.
- US1 (T004–T008) is the MVP. US2 needs T005 for the preview part. US3 and US4 share the
  assistant route (T014). US5 needs T019 and the route module from T014.

## Parallel examples

- After T002: T009/T010 (core risks) alongside T005/T006 (preview route).
- T019/T020 (core import) alongside T014/T015 (assistant route).

## Implementation strategy

MVP is US1. Then US2 (data safety), US3, US4, US5, polish. Commit after each phase.
