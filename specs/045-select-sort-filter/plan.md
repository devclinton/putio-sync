# Implementation Plan: Select All, Sort and Filter on the File and Transfer Lists

**Branch**: `045-select-sort-filter` | **Date**: 2026-11-03 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/045-select-sort-filter/spec.md`

## Summary

One pure ES module, `ui/query.js`, parses the filter language into AND-ed groups of OR-ed
glob terms and sorts rows by a column. A small DOM helper, `ui/listctl.js`, gives each page a
filter box, a Sort menu, clickable column headings and the `?q=&sort=` address sync. The
Put.io, Transfers and Local files pages filter and sort their already-loaded rows through it
and reset their selection to the rows shown. Local files gets the `selection()` bar from
feature 008 with a bulk Delete that calls the existing single-entry delete per entry. No
server change. The parser is unit-tested with `node --test`, run from a cargo test so
`make ci` covers it ([research.md](research.md) R1).

## Technical Context

**Language/Version**: plain ES modules, no build step; Rust stable for the test that runs them

**Primary Dependencies**: none new. Node (already on the dev machine and GitHub runners) runs
the JS tests; the cargo test skips with a message when `node` is missing

**Storage**: none; filter and sort live in the address (`location.hash`)

**Testing**: `node --test putio-sync-api/tests/ui/` for `query.js`, run by
`putio-sync-api/tests/ui_js.rs`; existing `ui_assets.rs` checks imports resolve and the size
budget; manual browser check per [quickstart.md](quickstart.md)

**Target Platform**: the embedded web UI in desktop and phone browsers

**Project Type**: workspace of crates; daemon with HTTP API and embedded web UI

**Performance Goals**: filter and re-render 1,000 rows per keystroke without visible lag
(SC-004): a linear scan with precompiled regexes

**Constraints**: UI assets stay under the 260 KB budget in `tests/ui_assets.rs`; 251.5 KB is
used, so about 14 KB remains for this feature

**Scale/Scope**: lists of tens to low thousands of rows

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | UI-only change inside `putio-sync-api/ui`; no crate changes |
| II. Local-first verification | The JS tests run inside `cargo test`, so `make ci` runs them; CI runs nothing extra |
| III. Tests without Put.io | The parser and sort are pure functions tested without a browser or network |
| IV. Download integrity | Not touched. Bulk Delete on Local files uses the existing guarded delete route, which still refuses in-use partials |
| V. Observable by default | No new server behaviour; each delete is already logged server-side |
| VI. Secrets | No new data leaves the browser; the filter goes in the URL hash, which is never sent to the server |
| VII. Simplicity | No dependency, no server route; one shared module instead of three copies |

Gate: pass. Re-checked after design: pass.

## Project Structure

### Documentation (this feature)

```text
specs/045-select-sort-filter/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   └── filter-grammar.md
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-api/
├── src/ui.rs                 # register query.js and listctl.js as assets
├── ui/
│   ├── query.js              # NEW: parseFilter, matcher, sortRows, sort key parsing (no DOM)
│   ├── listctl.js            # NEW: filter box, Sort menu, sortable <th>, address sync
│   ├── app.css               # sort-heading styles
│   └── pages/
│       ├── putio.js          # filter language, sort, keep sort on folder links
│       ├── transfers.js      # filter, sort, Select all over shown rows
│       └── files.js          # filter, sort, selection bar, bulk Delete
└── tests/
    ├── ui/query.test.mjs     # NEW: node --test cases for the grammar and sort
    └── ui_js.rs              # NEW: runs node --test, skips without node
```

**Structure Decision**: The pure logic is split from the DOM helper so Node can import it
without a `document`. Tests sit under `putio-sync-api/tests/` so they are never served.

## Complexity Tracking

None.
