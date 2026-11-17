# Research: Command palette

## R1 Settings from any page

- **Decision**: build the settings catalog from static data, without the DOM.
  - Cards come from `SECTIONS` in `ui/settings/sections.js`, which already lists each section's
    cards without touching the DOM.
  - Fields come from the group objects in the settings modules (`DOWNLOAD`, `BANDWIDTH`,
    `UPDATES` and the rest). Each group already declares `fields: [[name, label, hint, kind]]`
    and a config `key`.
  - Export those groups, and add `settings/catalog.js`, which turns them into
    `{ section, card, address, label, hint }` entries with the existing `?field=<key>.<name>`
    address.
  - Settings page search (`find()` in `pages/settings.js`) moves onto the same catalog. Custom
    cards with no group (Radarr/Sonarr, plugins, appearance) are found by their card title.
- **Rationale**: FR-007 needs settings to be findable from any page, so a list built from the
  DOM can't work. The labels are already data. Keeping one list means the two searches can't
  drift apart.
- **Alternatives considered**:
  - Mount the settings panels off-screen. This loads every card's settings over the network
    each time the palette opens.
  - A hand-kept list. It drifts as soon as someone adds a field.

## R2 put.io search

- **Decision**: add `PutioApiClient::search_files(query, per_page)` to `putio-sync-client`.
  - It calls put.io's v2 file search: `GET /files/search?query=<q>&per_page=<n>`, with the
    query URL-encoded.
  - It returns the first page only, capped at 50, and drops results whose parent is the trash.
  - The exact request shape and response fields (`files[]` with `id`, `name`, `parent_id`,
    `file_type`, `size`) MUST be checked against put.io's API docs before the client is
    written. The mockito test is then written from that shape.
  - Checked during implementation against put.io's own client, putdotio/putio-js
    (`src/domains/files.ts`): `GET /v2/files/search` with `query` and `per_page`, answering
    `{ cursor, files, status, total }`. Trash is a separate store that search does not
    cover, so there is nothing to drop.
  - The API route `GET /putio/files/search?q=` lives in `routes/putio/files.rs`. Axum matches
    the fixed `/search` segment before `/:id`.
- **Rationale**: put.io has an index already, and walking the whole account would mean
  thousands of calls. Constitution III: the test uses a mock, with no real account.
- **Alternatives considered**: the older `/files/search/{query}/page/{n}` form. It is kept
  only as a fallback if the docs show v2 search needs it.

## R3 Local search

- **Decision**: add `local_files::search(roots, query, limit, budget)` to
  `putio-sync-file-manager`, the crate that owns the filesystem (constitution I).
  - It walks breadth-first with `read_dir` and `symlink_metadata`, the way `summary()` does.
    Shortcuts are not followed, and names starting with `.` are skipped.
  - Matching is a case-insensitive substring match on the name, not fuzzy. Ranking happens in
    the UI.
  - It stops at `limit` hits (50) or once `budget` (2 s) has passed. The count and the clock
    are shared across all roots.
  - It returns `{ hits: [{ root, path, name, kind, size }], truncated }`.
  - The route `GET /local/files/search?q=` runs it in `tokio::task::spawn_blocking`. It uses
    the existing `roots(state)`, and each hit's `path` is relative to its root, so it can't
    point outside one. The UI labels each hit with its root's label.
- **Rationale**: there's no new dependency (`walkdir` would add one, constitution VII), the
  search can't block the async runtime, and it reuses the existing root handling.
- **Alternatives considered**:
  - Search the download history in the database. It misses files the user added themselves.
  - A cached index. It needs to be invalidated, which is more than this needs.

## R4 Highlighting the picked file

- **Decision**: add a `?select=<name>` query to the Put.io page (`#/putio/<folder>`) and the
  Files page (`#/files/<root>?path=<folder>`). After it renders, the page scrolls that row into
  view and marks it with the existing selected-row style. The Queue page keeps its existing
  `?file=<file_id>` filter.
- **Rationale**: both pages already take a folder in the URL, so this adds one optional
  parameter and nothing else.

## R5 Fuzzy matching

- **Decision**: a small `ui/fuzzy.js`. The typed letters must appear in the name in order,
  ignoring case. Scoring rewards matches at the start of a word and letters next to each
  other, and penalizes gaps. Each group shows up to 8 results. It has node tests next to the
  others in `tests/ui/`.
- **Rationale**: it's about 40 lines and needs no dependency, and node tests are how the other
  plain UI modules are tested (045).

## R6 Shortcut and dialog

- **Decision**: one `keydown` listener on `window` in `app.js`. On `(metaKey on macOS, else
  ctrlKey) && key === "k"`, it calls `preventDefault` and opens the palette. It does nothing
  when the current route is `login`, `setup` or `signin`, or when `#confirm` is open.
  - The palette is a `<dialog>` opened with `showModal()`, so the browser traps focus.
  - The dialog holds an `<input role="combobox" aria-expanded aria-controls>` and a
    `role="listbox"` with grouped `role="option"` rows, tracked with `aria-activedescendant`.
  - On close, focus returns to the element that had it before.
- **Rationale**: `<dialog>` already gives Esc, backdrop and focus trapping. The confirm dialog
  is built the same way.

## R7 Stale file results

- **Decision**: wait 250 ms after typing stops. Each search gets its own `AbortController`,
  and a new search aborts the last one. A result is drawn only if its query still matches the
  current text. While a group is loading it shows "Searching…". If it fails it shows "Couldn't
  search", and the other groups are unaffected.

## R8 Actions

| Action | Existing call |
|---|---|
| Sync now | `syncNow()` (`api.js`) |
| Pause downloads / Resume downloads | `setNasPaused(true/false)` (`app.js`) |
| Clear completed | `confirm({ title: "Remove completed items?" })` then `POST /local/queue/clear` |
| Check for updates | `navigate("#/update")` then `post("/update/check")` |
| Toggle theme | the setter in `theme.js` |
| Sign out | the sign-out handler in `app.js`; hidden when the identity is Tailscale-only (the `app.js:179` check) |
