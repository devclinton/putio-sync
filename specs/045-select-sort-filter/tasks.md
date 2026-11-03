---
description: "Task list for 045: select all, sort and filter on the file and transfer lists"
---

# Tasks: Select All, Sort and Filter on the File and Transfer Lists

**Input**: Design documents from `/specs/045-select-sort-filter/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/filter-grammar.md

**Tests**: Requested (FR-011, test-first). Parser and sort tests are written before `query.js`.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [ ] T001 Add `putio-sync-api/tests/ui_js.rs`: run `node --test` on `putio-sync-api/tests/ui/`, fail on a non-zero exit with its output, and pass with an `eprintln!` when `node` is not on `PATH`
- [ ] T002 Register `query.js` and `listctl.js` as assets in `putio-sync-api/src/ui.rs`

## Phase 2: Foundational (blocks every story)

- [ ] T003 Write failing tests in `putio-sync-api/tests/ui/query.test.mjs` for every row of the examples table and every rule in `contracts/filter-grammar.md` (AND, `OR`/`or`/`|`, OR tighter than AND, `-` and `NOT`, `AND` ignored, quotes incl. unclosed, anchored globs, substrings, brackets and regex characters literal, stray operators ignored, empty filter matches all)
- [ ] T004 Write failing tests in the same file for `parseSort`/`formatSort` (unknown key → null) and `sortRows` (asc/desc, numbers, dates, null lowest, folders first in both directions, ties by name then original order, input not mutated, null sort keeps order)
- [ ] T005 Implement `parseFilter`, `filterMatcher`, `parseSort`, `formatSort`, `sortRows` in `putio-sync-api/ui/query.js` (no imports, no DOM) until T003–T004 pass
- [ ] T006 Implement `putio-sync-api/ui/listctl.js`: `listControls({ query, columns, onChange })` returning the filter input, a Sort `<select>`, `th(key, label, attrs)` sortable headings (a button inside the `<th>`, `aria-sort`), `apply(rows, { folder })` → shown rows, `carry()` (the `sort=` param for folder links), and address sync via `history.replaceState` of `q`/`sort` keeping the rest of the hash query
- [ ] T007 [P] Add sort-heading and arrow styles to `putio-sync-api/ui/app.css`

## Phase 3: User Story 1 – Filter with patterns (P1) 🎯 MVP

**Independent test**: `-*photoshop* *exe*` on a list of `Photoshop.exe`, `setup.exe`, `movie.mkv` shows only `setup.exe` with "1 of 3".

- [ ] T008 [US1] Put.io: replace the "contains" filter with `listControls` in `putio-sync-api/ui/pages/putio.js`; restore `q` from the route query; heading "N of M items"; empty state "Nothing matches the filter"
- [ ] T009 [US1] Transfers: add the filter to the list card toolbar in `putio-sync-api/ui/pages/transfers.js`; the unchanged-JSON skip also keys on filter and sort; filter change re-renders from `current`; "Transfers (N of M)"
- [ ] T010 [US1] Local files: add the filter in `putio-sync-api/ui/pages/files.js`, keeping `path` in the address; "N of M items"; filtering while renaming leaves the edit alone (no re-render while `editing`)

## Phase 4: User Story 2 – Select all of what the filter shows (P1)

**Independent test**: on Local files, filter to two of five, Select all, Delete → confirm names two; three remain.

- [ ] T011 [US2] Put.io: `sel.reset` with shown, non-queued rows only (already true; confirm after T008) in `putio-sync-api/ui/pages/putio.js`
- [ ] T012 [US2] Transfers: `sel.reset` with shown rows only, and `selectedTransfers` reads from shown rows, in `putio-sync-api/ui/pages/transfers.js`
- [ ] T013 [US2] Local files: add `selection()` with a Delete action, a checkbox column keyed by `e.path`, rows not `valid_name` or `in_use` disabled, in `putio-sync-api/ui/pages/files.js`
- [ ] T014 [US2] Local files bulk Delete in `putio-sync-api/ui/pages/files.js`: one confirm (count, up to five names, "folders go with everything in them"), then `DELETE` per entry in turn, stop on 401/429, one toast with failures, clear selection, reload

## Phase 5: User Story 3 – Sort by a column (P2)

**Independent test**: on Put.io, click Size twice → largest first, folders on top.

- [ ] T015 [P] [US3] Put.io sortable Name, Size, Date (folders first); folder links and breadcrumbs carry `sort` in `putio-sync-api/ui/pages/putio.js`
- [ ] T016 [P] [US3] Transfers sortable Name, Status, Progress, Size, Added in `putio-sync-api/ui/pages/transfers.js`
- [ ] T017 [P] [US3] Local files sortable Name, Size (folders none), Modified (folders first); `href()` carries `sort`, in `putio-sync-api/ui/pages/files.js`

## Phase 6: Polish

- [ ] T018 Add a CHANGELOG.md Unreleased line; note in ROADMAP.md that 045 built the list filter language and 020 can reuse `query.js`
- [ ] T019 Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p putio-sync-api` (incl. the size budget), then the workspace tests
- [ ] T020 Browser check per `quickstart.md` against a local daemon; mark spec status

## Dependencies

- T001–T002 → T003–T005 → T006 → stories. US1 before US2 on the same page (shared render code); US3 tasks touch different pages and can run in parallel with each other after their page's US1/US2 tasks.
- Within a page, tasks are sequential (same file).

## Implementation Strategy

MVP is Phase 1–3: the filter language on all three pages. US2 then makes it act; US3 adds order.
