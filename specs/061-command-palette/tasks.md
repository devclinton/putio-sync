# Tasks: Command palette

**Input**: design documents in `specs/061-command-palette/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/api.md

**Tests**: tests are included. Constitution III requires them, and quickstart.md lists them.

Paths are relative to the repo root. `ui/` means `putio-sync-api/ui/`.

## Phase 1: Setup

- [ ] T001 Add a "061 Command palette" entry to ROADMAP.md, linking `specs/061-command-palette/`
- [ ] T002 [P] Add an [Unreleased] line for the command palette to CHANGELOG.md

## Phase 2: Foundational (blocks every story)

- [ ] T003 [P] Write node tests for the scorer in `putio-sync-api/tests/ui/fuzzy.test.mjs`.
  Cover: letters in order match, ignoring case; out-of-order letters don't match; word-start
  and adjacent matches rank higher; an empty query matches everything.
- [ ] T004 [P] Implement `match(query, text) → score | null` and `rank(query, items, key,
  limit = 8)` in `ui/fuzzy.js`, as specified in research R5
- [ ] T005 Add the `<dialog id="palette">` markup to `ui/index.html`, and add palette styles
  to `ui/app.css`: phone width, both themes, a selected row, group headings
- [ ] T006 Write the palette shell in `ui/palette.js`:
  - `open()` and `close()`, with focus restored on close (FR-002)
  - a combobox input and a listbox with `aria-activedescendant` (FR-022)
  - arrow keys, Enter, click and tap (FR-005)
  - groups shown in this order and hidden when empty: actions, pages, settings, queue,
    putio, local (FR-003)
  - a `register(group, source)` hook, where a source is `(query, signal) → results |
    Promise`
  - "Nothing matched."
  - all text through the i18n `t()` (FR-021)
- [ ] T007 Add the window keydown listener in `ui/app.js` (FR-001):
  - Cmd+K on macOS, Ctrl+K elsewhere, with `preventDefault`
  - does nothing on the `login`, `setup` and `signin` routes, and while `#confirm` is open
  - imports and mounts `palette.js`

**Checkpoint**: the shortcut opens an empty palette, and Esc closes it.

## Phase 3: User Story 1 - Jump to a page, setting or action (P1) 🎯 MVP

**Goal**: from any page, find and open pages and settings, and run actions.
**Independent test**: quickstart steps 1–4 and 8.

- [ ] T008 [P] [US1] Export each settings group object (the ones with `fields`) from
  `ui/settings/downloads.js`, `general.js`, `putio.js`, `filters.js`, `integrations.js`,
  `advanced.js`, `media.js`, `subtitles.js`, `translation.js` and `plugins.js`, as
  `export const groups = [...]`. Leave their card rendering unchanged.
- [ ] T009 [P] [US1] Write node tests in `putio-sync-api/tests/ui/settings_catalog.test.mjs`:
  - every card in `SECTIONS` has an entry
  - every group field has an entry with the address `<key>.<name>`
  - every address resolves through `fieldId`
- [ ] T010 [US1] Implement `ui/settings/catalog.js`, which builds `{ section, card, address,
  label, hint }` entries from `SECTIONS` and the exported groups (research R1)
- [ ] T011 [US1] Move the Settings search in `ui/pages/settings.js` onto `catalog.js` and
  `fuzzy.js`, so it no longer scans the DOM. Keep its look and its `navigate(settingsHref(...))`
  jump.
- [ ] T012 [US1] Register a Pages source in `ui/palette.js` (FR-006). Its list is every link in
  `#nav`, plus Update and News, and picking one calls `navigate("#/<page>")`.
- [ ] T013 [US1] Register a Settings source in `ui/palette.js` from `catalog.js` (FR-007).
  Picking one navigates to `settingsHref(section, address)`.
- [ ] T014 [US1] Register an Actions source in `ui/palette.js`, using the table in research R8
  (FR-008–FR-011):
  - Clear completed confirms "Remove completed items?" first.
  - Sign out is hidden for a Tailscale-only identity.
  - Check for updates navigates to `#/update`, then calls `post("/update/check")`.
  - Errors go through `toastError`.
- [ ] T015 [US1] Add the new strings to the English catalog in `ui/i18n/`, and run the 051
  translate script for the other languages

**Checkpoint**: US1 works on its own. This is the MVP.

## Phase 4: User Story 2 - Find a queue item (P2)

**Goal**: queue items are found by file name.
**Independent test**: quickstart step 5.

- [ ] T016 [US2] Register a Queue source in `ui/palette.js` (FR-012):
  - `GET /local/queue` once per opening
  - match on `filename`
  - picking one navigates to `#/queue?file=<file_id>`
  - if the request fails or there is no database, the group is left out

## Phase 5: User Story 3 - Find a put.io file (P2)

**Goal**: put.io files are found by name and opened in their folder.
**Independent test**: quickstart step 6.

- [ ] T017 [P] [US3] Write a mockito test for `search_files` in `putio-sync-client/src/files.rs`.
  Check the request query, a cap of 50, and that trash parents are dropped. First check the
  shape of put.io's v2 search request and response against put.io's API docs (research R2).
- [ ] T018 [US3] Implement `PutioApiClient::search_files(query, per_page)` in
  `putio-sync-client/src/files.rs`
- [ ] T019 [P] [US3] Write an API test in `putio-sync-api/tests/palette_search.rs` for
  `GET /putio/files/search`. Cases: 401 without sign-in; 400 when `q` is shorter than 2 or
  longer than 200 characters after trimming; 200 with
  `{ files: [{ id, name, parent_id, is_folder, size }] }`; 502 when put.io fails.
- [ ] T020 [US3] Add the `GET /putio/files/search?q=` route in
  `putio-sync-api/src/routes/putio/files.rs`, before `/:id`, with a `tracing` debug line
  (count, duration)
- [ ] T021 [US3] Add `?select=<name>` to `ui/pages/putio.js`: after rendering, scroll to that
  row and mark it (research R4)
- [ ] T022 [US3] Register a Put.io source in `ui/palette.js` (FR-013, FR-014, FR-019):
  - starts at 2 or more characters, 250 ms after typing stops
  - an `AbortController` per search, and stale results are dropped (R7)
  - shows "Searching…" while waiting and "Couldn't search" on failure
  - picking one navigates to `#/putio/<parent_id>?select=<name>`

## Phase 6: User Story 4 - Find a local synced file (P3)

**Goal**: files under the sync folders are found by name and opened in their folder.
**Independent test**: quickstart step 7.

- [ ] T023 [P] [US4] Write unit tests for `search` in
  `putio-sync-file-manager/src/local_files.rs`, using temp dirs. Cases: a case-insensitive
  match; dotfiles and dot-folders skipped; symlinks not followed; the 50-hit limit sets
  `truncated`; a zero budget stops at once; several roots share one budget.
- [ ] T024 [US4] Implement `local_files::search(roots: &[(key, label, path)], query, limit,
  budget) → SearchResult { hits, truncated }` in `putio-sync-file-manager/src/local_files.rs`.
  It walks breadth-first with `symlink_metadata` and skips names that are not UTF-8
  (research R3).
- [ ] T025 [P] [US4] Add API tests for `GET /local/files/search` to
  `putio-sync-api/tests/palette_search.rs`:
  - 401 without sign-in
  - 400 when `q` is shorter than 2 or longer than 200 characters
  - 200 with `{ hits: [{ root, root_label, path, name, kind, size }], truncated }` over
    temp-dir roots
  - no roots → `{ hits: [], truncated: false }`
  - no path in a hit leaves its root
- [ ] T026 [US4] Add the `GET /local/files/search?q=` route in
  `putio-sync-api/src/routes/local/files.rs`, before `/:root`. It runs in `spawn_blocking`,
  uses `roots(state)`, and logs a `tracing` debug line (count, truncated, duration).
- [ ] T027 [US4] Add `?select=<name>` to `ui/pages/files.js` (research R4)
- [ ] T028 [US4] Register a Local source in `ui/palette.js` (FR-015–FR-019):
  - debounced and aborted, as in T022
  - the detail line shows the root label and the folder
  - when `truncated` is set, the group shows a "More not shown" row
  - picking one navigates to `#/files/<root>?path=<folder>&select=<name>`

## Phase 7: Polish

- [ ] T029 [P] Document the palette and its shortcut in `site/book/src/web-ui.md`
- [ ] T030 Run the hand checks in `specs/061-command-palette/quickstart.md`: phone width,
  dark theme, another language
- [ ] T031 Run `make ci`

## Dependencies

- Phase 2 blocks every story.
- US1 (T008–T015) is the MVP. US2, US3 and US4 each depend only on Phase 2, so they can follow
  in any order.
- Within a story: tests before code, the client or file-manager function before its route, and
  the route before the UI source.
- T011 depends on T010, which depends on T008.

## Parallel examples

- Phase 2: T003 and T004 can run alongside T005.
- US1: T008 and T009 together, then T010.
- Once Phase 2 is done: the backend of US3 (T017–T020) and the backend of US4 (T023–T026)
  touch different crates and can run at the same time. Both add to `palette_search.rs`, so
  merge with care.

## Implementation strategy

1. Phases 1–3 → a usable palette (MVP), shippable on its own.
2. Add US2 (one task).
3. Add US3 and US4. Each brings its server route, page tweak and palette source together.
4. Polish, then `make ci`.
