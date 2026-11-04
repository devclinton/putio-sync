# Tasks: What's New

**Input**: Design documents from `specs/017-whats-new/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/whats-new-api.md

**Tests**: included (constitution III; the API and parser are tested before the UI uses them).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependencies)
- **[Story]**: US1 (panel after upgrade), US2 (full changelog page)

## Phase 1: Setup

- [ ] T001 Copy `CHANGELOG.md` into the builder stage before `cargo build` in `Dockerfile` (research R8)

## Phase 2: Foundational (blocks both stories)

- [ ] T002 [P] Add `CREATE_WHATS_NEW_SEEN_TABLE` to `putio-sync-db/src/schema.rs` and create it in the table list in `putio-sync-db/src/lib.rs`; add `whats_new_seen` to the pre-039…043 migration test's table list
- [ ] T003 Add `WhatsNewRepository` (`seen_ids() -> HashSet<String>`, `mark_seen(&[String])` with `INSERT OR IGNORE` in one transaction) in `putio-sync-db/src/whats_new.rs`, `db.whats_new()` in `putio-sync-db/src/lib.rs`, with unit tests (empty, mark twice, ids returned)
- [ ] T004 [P] Write parser unit tests first in `putio-sync-api/src/changelog.rs`: releases with and without dates, kinds, multi-line entries joined, intro and link references ignored, empty Unreleased kept, id stable under re-wrapping and the same across sections, FNV-1a known value, and the built-in changelog parses with at least one entry and no empty entry text
- [ ] T005 Implement `parse(&str) -> Vec<Release>`, `entry_id(&str)`, and `builtin()` (`include_str!("../../CHANGELOG.md")` behind a `OnceLock`) in `putio-sync-api/src/changelog.rs`; register the module in `putio-sync-api/src/lib.rs`

## Phase 3: User Story 1 - See what changed after an upgrade (P1) 🎯 MVP

**Goal**: after an upgrade, the first signed-in page load shows a panel with the new entries; closing it marks them seen for every browser.

**Independent test**: delete two rows from `whats_new_seen`, reload: the panel lists exactly those two; close and reload: no panel.

- [ ] T006 [P] [US1] API tests in `putio-sync-api/tests/whats_new.rs`: first GET records a baseline with `new_count` 0; removing a seen row makes that entry `new` and `new_count` 1; `POST /whats-new/seen` clears it; unknown ids ignored; over 1000 ids → 400; both routes 401 without a token; no database → `new_count` 0 and POST 204; in setup mode `GET /whats-new` is not served (503, as other data routes)
- [ ] T007 [US1] Implement `GET /whats-new` and `POST /whats-new/seen` per the contract in `putio-sync-api/src/routes/system/whats_new.rs` (register in `routes/system/mod.rs`), and nest `/whats-new` behind auth in `putio-sync-api/src/lib.rs`
- [ ] T008 [US1] Add `pages/news.js` with the inline renderer (code, links with safe URLs, bold; everything else text) and `checkNews()`: fetch `/whats-new`, and when `new_count > 0` show a modal panel with up to 20 new entries grouped by release and kind, "and N more" with a link to `#/news`, and post every new id on close; embed it in `putio-sync-api/src/ui.rs`
- [ ] T009 [US1] In `putio-sync-api/ui/app.js` call `checkNews()` once per page load on the first signed-in route (not setup mode, not the sign-in page)
- [ ] T010 [P] [US1] Panel styles in `putio-sync-api/ui/app.css` (reuse the confirm dialog's look; a "new" mark)

## Phase 4: User Story 2 - Read the whole changelog any time (P2)

**Goal**: a What's new page with the full built-in changelog, reachable from the Status page.

**Independent test**: open `#/news`: every section and entry shows, newest first, with new entries marked; the Status page's version links there.

- [ ] T011 [US2] The page in `putio-sync-api/ui/pages/news.js` (`mount`/`unmount`): the running version and commit, every release with entries (Unreleased labelled as this build's changes), kinds, new marks, and an empty state when there are no notes; add `news` to `PAGES` and `TITLES` in `putio-sync-api/ui/app.js`
- [ ] T012 [US2] Link the version on the Status page's build card to `#/news` in `putio-sync-api/ui/pages/status.js`

## Phase 5: Polish

- [ ] T013 [P] README: a short "What's new" note (where to find it, first-run behaviour) in `README.md`
- [ ] T014 [P] `CHANGELOG.md` entry under Unreleased → Added; remove 017 from `ROADMAP.md`
- [ ] T015 Check the UI stays under the size cap (`cargo test -p putio-sync-api --test ui_assets`) and check the panel and page in a browser against a scratch `server` on port 15999 (quickstart)
- [ ] T016 Merge `origin/main`, run `TMPDIR=<scratchpad> make ci`, fix anything it finds, push

## Dependencies

- T001 independent. T002 → T003. T004 → T005. T003 and T005 → T006/T007 → T008 → T009, T011 → T012.
- T010, T013, T014 can run any time; T015 → T016 last.

## Analysis (2026-11-03)

`/speckit-analyze`: every FR maps to a task (FR-001 T001/T005, FR-002–003 T004–T005,
FR-004–005 T002–T003/T007, FR-006–007 T008–T009, FR-008 T011, FR-009 T008, FR-010 T006–T007,
FR-011 T012, FR-012 T001). One HIGH gap fixed: FR-010's setup-mode rule had no test; T006
now covers it. No CRITICAL findings. LOW: the inline renderer (FR-009) is only checked by
`node --check` and in the browser, as the rest of the UI is.
