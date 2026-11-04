# Tasks: Glob Filters with AND and OR

**Input**: Design documents from `specs/020-glob-filters/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/pattern-grammar.md, contracts/api.md

**Tests**: included; the constitution (III) and FR-001 require them.

## Phase 1: Setup

- [ ] T001 Create the shared case table `putio-sync-core/tests/data/filter-cases.json` (contract format `{ filter, names | paths, shown }`): every example of 045's grammar table and `putio-sync-api/tests/ui/query.test.mjs`, plus comma, braces, paths, quotes with `|` and `,`, `[YTS]` literal, case, `?` with a non-ASCII letter; at least 40 cases

## Phase 2: Foundational (blocks every story)

- [ ] T002 Create `putio-sync-core/src/pattern.rs`: tokenizer mirroring `query.js` (quoted with optional `-`, `|`/`,` runs as OR, bare with `{…}` groups), lenient `Pattern::parse(text)` mirroring `parseFilter`, brace expansion (no nesting; "more than 64 alternatives" caps), case-insensitive backtracking glob over `char`s, plain terms wrapped `*…*`, `/` terms tested against the path and its tails after each `/`; `Pattern::{as_str, is_empty, matches(name, path)}`, `PartialEq`/`Serialize` by text; unit tests; register `pub mod pattern` in `putio-sync-core/src/lib.rs`
- [ ] T003 Add `pattern::check(text) -> Result<Vec<String>, String>` in `putio-sync-core/src/pattern.rs`: errors "unclosed quote", "unbalanced { }", "nested { } are not supported", "more than 64 alternatives", "only operators, no pattern", "longer than 1000 characters"; warnings for leading / trailing / repeated OR, a dangling NOT or `-`, and `[Ss]` / `[0-9]` look-alikes; unit tests
- [ ] T004 [P] Add `putio-sync-core/tests/filter_cases.rs` running every case of the shared table against `Pattern` (name cases: path = name; path cases: name = last segment)
- [ ] T005 [P] Extend `putio-sync-api/ui/query.js`: `,` as OR, `{a,b}` alternatives (non-capturing group), `u` flag, `/` terms tested against `path` and its tails; `filterMatcher(text)` returns `(name, path = name) => bool`; update its header comment; run the shared table from `putio-sync-api/tests/ui/query.test.mjs`; update `specs/045-select-sort-filter/contracts/filter-grammar.md` to point at the 020 contract

**Checkpoint**: both matchers pass the same table.

## Phase 3: User Story 1 - Never download samples in a folder (P1) 🎯 MVP

**Goal**: a folder rule's Never download pattern refuses files. **Independent test**: rule `Movies` exclude `*sample*` refuses `Film.sample.mkv` only.

- [ ] T006 [US1] In `putio-sync-core/src/rules.rs`: `FolderRule.include` / `exclude: Option<String>` (serde default, skip if none), counted by `is_empty`; `Effective.include` / `exclude: Sourced<Pattern>` and a non-serialised `folder`; `RuleSet` root values from `whitelist_patterns` / `blacklist_patterns`; `apply` sets them (a rule's `""` resets to empty); `Effective::decide` order skip → exclude → include → type filter; `Reason::Excluded { pattern }` ("matches never-download pattern `P`") and `Reason::NotIncluded { pattern }` ("doesn't match only-download pattern `P`"); `validate` checks both patterns strictly ("Folder rule {path}: Never download: {error}"); tests: exclude, include, reset with `""`, deeper wins, path pattern `Sample/*`, never unblocks, no patterns = unchanged
- [ ] T007 [US1] In `putio-sync-api/src/routes/system/rules.rs`: `RuleBody.include` / `exclude` (trim; `""` kept, null inherit), `EffectiveView.include` / `exclude` as `{ value, from }`; `GlobalView.include` / `exclude`; fix every struct literal of `FolderRule` / `RuleBody` across the workspace
- [ ] T008 [US1] `RulePreview.downloads` (first 50 `{ file_id, name, path }`) in `putio-sync-download/src/manager.rs` and `PreviewResponse.downloads` in `putio-sync-api/src/routes/system/rules.rs`
- [ ] T009 [US1] Route tests in `putio-sync-api/tests/rules.rs`: PUT a rule with exclude, GET shows it in `rule` and `effective`, preview lists the sample as blocked with the pattern reason and the rest in `downloads`, `would_delete` with `delete_blocked`; a bad pattern is 400 naming "Never download"
- [ ] T010 [US1] Rule card and editor in `putio-sync-api/ui/rules.js`: "Only download" / "Never download" rows with source, two inputs (blank = inherit, "Don't inherit" checkbox sending `""` when one is inherited), `verdict()` checks exclude then include with `filterMatcher(value)(name, path)` before the type filter, preview shows `downloads`

**Checkpoint**: US1 works from the Put.io page.

## Phase 4: User Story 2 - AND, OR and NOT (P1)

**Goal**: combined patterns mean the same everywhere. **Independent test**: the shared table.

- [ ] T011 [US2] Rule tests in `putio-sync-core/src/rules.rs` for `*.mkv OR *.mp4 -sample` include and `Sample/* OR *.nfo` exclude, and an API preview test in `putio-sync-api/tests/rules.rs` with a nested `Sample` folder
- [ ] T012 [P] [US2] Review the filter-box hints and header comments in `putio-sync-api/ui/pages/putio.js`, `transfers.js` and `files.js` against the 020 grammar; fix any that contradict it (no change expected: "Try fewer terms, or * around a word." still holds)

## Phase 5: User Story 3 - Account-wide Only and Never download (P1)

**Goal**: the existing settings apply. **Independent test**: `whitelist_patterns = "*.{mkv,mp4}"` downloads only `a.mkv` of `a.mkv`, `b.avi`.

- [ ] T013 [US3] Validate `whitelist_patterns` ("[download] whitelist_patterns (Only download): …") and `blacklist_patterns` ("… (Never download): …") strictly in `Config::validate` (`putio-sync-core/src/config.rs`) and the wizard validation in `putio-sync-core/src/setup.rs`; tests: old comma and brace values load and match as before, an unclosed quote fails to load
- [ ] T014 [US3] Test in `putio-sync-download/tests/config_from_settings.rs` (or `enqueue_tree.rs`) that the root patterns refuse files in the sync decision, and that the refusal is a `Decision::Blocked` whose `reason()` names the pattern (FR-013: the existing log line, recorded decision and "blocked" metric use these)
- [ ] T015 [US3] `POST /rules/preview` accepts `global { include, exclude }` replacing the two settings in the candidate config, allowed on `folder_id: 0`, in `putio-sync-api/src/routes/system/rules.rs`; route test in `putio-sync-api/tests/rules.rs`; settings route test that a bad pattern is 400 in `putio-sync-api/tests/settings.rs`, and the same for a settings import carrying it
- [ ] T016 [US3] Settings Download group in `putio-sync-api/ui/pages/settings.js`: help text "Patterns, e.g. *.mkv OR *.mp4; empty for all" / "e.g. *sample* OR *.exe", and an `extra` with a Preview button that posts the typed values as `global` for the root and renders the result with the rules preview view (export it from `putio-sync-api/ui/rules.js`), with the note "Lists your whole Put.io account; this can take a while."

## Phase 6: User Story 4 - Bad and risky patterns (P2)

**Goal**: errors refuse saves; warnings show in Settings and the doctor. **Independent test**: rule include `*.mkv` with delete_blocked → doctor Config warns.

- [ ] T017 [US4] In `putio-sync-core/src/assistant.rs`: `pattern_warning` risks (Level::Warning; keys `download.whitelist_patterns`, `download.blacklist_patterns` or `download.rules`) for every check warning, and `delete_unmatched` (Level::Warning, `download.rules`) for a rule with `delete_blocked = true` whose effective include is non-empty: "Files under {path} that don't match `{p}` are deleted from Put.io at each sync"; tests
- [ ] T018 [US4] Doctor test in `putio-sync-doctor/src/checks/local.rs` that the config check lists `delete_unmatched`

## Phase 7: Polish

- [ ] T019 [P] README (folder rules, Download settings, the pattern language, the "now applied" note), `config.example.toml` and `example-config.toml` (`include` / `exclude` rule keys, comma / brace / AND / OR examples)
- [ ] T020 [P] `CHANGELOG.md` Unreleased entry (Added: patterns in folder rules and preview; Changed: Only / Never download now applied; Fixed: they were ignored)
- [ ] T021 UI bundle stays under 384 KB (`putio-sync-api/tests/ui_assets.rs`); `cargo fmt`, clippy
- [ ] T022 Remove 020 from `ROADMAP.md`; merge `origin/main`; `TMPDIR=<scratch> make ci`; push; hand-over note in the shared notes folder

## Dependencies

- T001 → T004, T005. T002 → T003 → T006 → (T007, T013, T017). T007 → T008 → T009, T010, T015. T015 → T016.
- US1 is the MVP; US2 is tests over US1; US3 needs T006; US4 needs T003 and T006.

## Parallel examples

- T004 and T005 (Rust and JS against the same table).
- T019 and T020 at the end.

## Implementation strategy

Foundational matcher first (both languages against the table), then the rule wiring (US1),
then the account-wide settings (US3), then warnings (US4), then docs.
