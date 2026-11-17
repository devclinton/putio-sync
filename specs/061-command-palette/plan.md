# Implementation Plan: Command palette

**Branch**: `061-command-palette` | **Date**: 2026-11-13 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/061-command-palette/spec.md`; agreed calls in
[design.md](design.md)

## Summary

The palette is a `<dialog>`-based search in `ui/palette.js`. Cmd/Ctrl+K opens it from any
signed-in page. It groups results in this order: actions, pages, settings, the queue, put.io
files and local files.

Actions, pages, settings and the queue are found in the browser. They are matched by a small
fuzzy scorer, against a settings catalog built from the settings modules' static field
declarations. The Settings page's own search moves onto that same catalog.

Two new authenticated routes search files by name:
- put.io's own search, through a new `putio-sync-client` method;
- a bounded walk of the sync roots, in `putio-sync-file-manager`, with 50 hits or 2 seconds
  shared across all roots, no symlinks and no hidden files.

The Put.io and Files pages gain a `?select=` parameter that marks the picked row. Details:
[research.md](research.md).

## Technical Context

**Language/Version**: Rust stable (edition 2021); plain ES modules for the UI
**Primary Dependencies**: none new (axum, tokio, reqwest and mockito are already in use)
**Storage**: N/A (nothing is stored)
**Testing**: `cargo test` (file-manager unit tests, client mockito test, the API test
`palette_search.rs`), `node --test` over `putio-sync-api/tests/ui/` (run from `ui_js.rs`)
**Target Platform**: the daemon on Linux/macOS/Windows; current desktop and mobile browsers
**Project Type**: Rust workspace serving a web UI
**Performance Goals**: in-browser groups update on every keystroke at a queue of 1,000 items;
file results within 1 s of a pause; local search hard-capped at 2 s
**Constraints**: no new dependency; search runs off the async runtime; no paths outside the
roots; all text translatable (051)
**Scale/Scope**: 1 new UI module plus 2 small helpers, 2 routes, 1 client method, 1
file-manager function, 3 page tweaks

## Constitution Check

| Principle | Check |
|---|---|
| I Crate boundaries | The put.io search is in `client`, the disk walk in `file-manager`, and `api` only wires routes. ✅ |
| II Local-first | Everything runs under `make ci`. ✅ |
| III Tests without Put.io | The client test uses mockito, the API test uses the mock client and temp-dir roots, and the UI logic has node tests. ✅ |
| IV Download integrity | Read-only; nothing is downloaded or deleted. ✅ |
| V Observable | The routes log a `tracing` debug line with the hit count, `truncated` and the duration. No new metrics: a search is not daemon work. ✅ |
| VI Secrets | Both routes are behind the existing auth middleware, and the token never reaches a response. ✅ |
| VII Simplicity | No dependency, no index, no config knob. The settings catalog reuses existing declarations. ✅ |

Re-checked after design: no change.

## Project Structure

### Documentation (this feature)

```text
specs/061-command-palette/
├── design.md        # agreed design and calls
├── spec.md
├── plan.md          # this file
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/api.md
├── checklists/requirements.md
└── tasks.md         # /speckit-tasks
```

### Source Code

```text
putio-sync-client/src/files.rs              # + search_files()
putio-sync-file-manager/src/local_files.rs  # + search()
putio-sync-api/src/routes/putio/files.rs    # + GET /putio/files/search
putio-sync-api/src/routes/local/files.rs    # + GET /local/files/search
putio-sync-api/tests/palette_search.rs      # new
putio-sync-api/ui/
├── palette.js                # new: dialog, groups, keyboard, fetches
├── fuzzy.js                  # new: scorer
├── settings/catalog.js       # new: settings entries from SECTIONS + groups
├── settings/*.js             # export their group objects
├── pages/settings.js         # search uses the catalog
├── pages/putio.js, files.js  # ?select=
├── app.js                    # shortcut listener, mount the palette
├── index.html, app.css       # <dialog id="palette">, styles
└── i18n/                     # new strings into the catalogs
putio-sync-api/tests/ui/fuzzy.test.mjs, settings_catalog.test.mjs   # new
CHANGELOG.md, ROADMAP.md, site/book/src/web-ui.md                    # docs
```

**Structure Decision**: the existing workspace layout. New behavior goes in the crate that
owns each concern, and the UI stays plain modules.

## Complexity Tracking

None.
