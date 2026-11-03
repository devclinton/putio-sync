# Research: Select All, Sort and Filter

## R1. How to unit-test UI JavaScript

- **Decision**: `node --test` over `putio-sync-api/tests/ui/*.test.mjs`, launched by a cargo
  integration test (`tests/ui_js.rs`). Without `node` on `PATH`, the test prints why and passes.
- **Rationale**: The UI has no JS tests or build step today. Node 22 has a built-in test runner
  and ES module support, so no npm packages are needed. Running it from `cargo test` keeps
  `make ci` the single gate (constitution II).
- **Alternatives**: an embedded JS engine crate (boa, quickjs) — a heavy new dependency for a
  few tests; a separate `make test-js` target — CI and `make ci` would need to learn it, and it
  would be easy to forget.

## R2. Client-side or server-side filtering

- **Decision**: client-side, over the rows each page already has.
- **Rationale**: `GET /putio/files?parent_id=` returns one Put.io listing page and the UI shows
  all of it; `/putio/transfers` returns every transfer; `/local/files/<root>` returns up to its
  2,000-entry cap. Sorting and filtering what is shown needs no new route, and select-all over
  the shown rows is then exactly what the user sees.
- **Alternatives**: server-side query parameters — they would need the same parser in Rust and
  would refetch on every keystroke.
- **Note**: the Put.io client ignores Put.io's `cursor`, so a folder with more items than one
  page lists only the first page. That limit is the same today and is outside this feature.

## R3. Operator precedence

- **Decision**: OR binds tighter than the implicit AND; no parentheses.
- **Rationale**: `*.mkv OR *.mp4 -sample` should mean "(mkv or mp4) and not sample", which is
  how search engines read it. With AND tighter, the `-sample` would only apply to mp4.
- **Alternatives**: full boolean expressions with parentheses — more to learn and test for no
  request.

## R4. Glob characters and anchoring

- **Decision**: `*` and `?` only, anchored when present; plain terms are substrings.
  Matching is case-insensitive. The pattern compiles to a `RegExp` with every other character
  escaped.
- **Rationale**: Keeps today's Put.io "contains" filter working unchanged for plain words, and
  makes `*.exe` mean "ends in .exe". Brackets stay literal because release names use them.

## R5. Where the filter and sort are kept

- **Decision**: `q` and `sort` in the hash query, written with `history.replaceState` (as the
  Stats page does for `range`), so typing does not trigger a route change or a remount.
- **Rationale**: Reload, Back and bookmarks keep the view. The hash is never sent to the server.
- Folder links (Put.io and Local files) carry `sort` and drop `q`, since a filter for one
  folder rarely fits the next.

## R6. Bulk delete on Local files

- **Decision**: call `DELETE /local/files/<root>?path=` per selected entry, in turn, after one
  confirm; one summary toast names failures.
- **Rationale**: The single delete already guards in-use partials and logs each delete. A bulk
  route would duplicate that for lists of a handful of entries.
- The per-folder size preview of the single delete is skipped for bulk (one request per
  folder before the confirm); the confirm says folders go with their contents instead.

## R7. Sort for transfers that refresh

- **Decision**: the Transfers page's "skip render when the JSON is unchanged" check also keys on
  the filter and sort, and a filter or sort change re-renders from the last list.
- Ties break on name, then id, so rows don't move between refreshes.
