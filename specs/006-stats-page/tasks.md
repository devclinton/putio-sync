---

description: "Task list for feature 006: Stats Page"
---

# Tasks: Stats Page

**Input**: Design documents from `specs/006-stats-page/`

**Prerequisites**:
- [plan.md](plan.md)
- [spec.md](spec.md)
- [research.md](research.md) (R1–R10)
- [data-model.md](data-model.md)
- contracts: [api.md](contracts/api.md), [ui.md](contracts/ui.md)
- [quickstart.md](quickstart.md)

**Tests**: included. Constitution III requires the API and UI to be exercisable against a
mocked backend, and Principle VI needs a test that a tracker passkey never leaks. Within
each story, write the tests first and confirm they fail before implementing.

**Organization**: grouped by user story in spec priority: US1 (P1, MVP), US2 (P2),
US3 (P2), US4 (P3).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: US1–US4 from spec.md
- Paths are relative to the repo root.

## Ground rules for every task

- Rust edition 2021, so no let-chains. `make ci` = fmt, clippy `-D warnings`, test and a
  release build.
- Response shape from [contracts/api.md](contracts/api.md); page behaviour from
  [contracts/ui.md](contracts/ui.md). Don't invent fields.
- Never store, log or return a full tracker URL, magnet URI or transfer source (R5).
- UI: no third-party code; text through text nodes (`h()`), never `innerHTML`.

---

## Phase 1: Setup

- [x] T001 Register `pages/stats.js` in the embedded assets list in `putio-sync-api/src/ui.rs` (start the file as a stub exporting `mount`/`unmount`) and add the "Stats" nav link after History in `putio-sync-api/ui/index.html`

---

## Phase 2: Foundational (blocks every story)

**Purpose**: the aggregation module with range and bucket handling, the endpoint, the
router's query support, seeding helpers and the page shell with filters.

- [x] T002 [P] Add `HistoryRepository::insert_download_at(file_id, filename, file_size, duration_seconds, average_speed, timestamp)` in `putio-sync-db/src/history.rs`, with a unit test that the stored `timestamp` is the given one
- [x] T003 Write failing unit tests in `putio-sync-db/src/analytics.rs` for: range presets → `since`; bucket choice (24h→1h/24, 7d→6h/28, 30d→1d/30, 90d→1d/90, 1y→1w/53, all→fits ≤120); local alignment with a non-zero `tz` (day buckets start at local midnight, weeks on Monday); empty history gives zeroed totals and empty-but-complete series
- [x] T004 Implement `StatsQuery`, `RangePreset`, `BucketSize`, the row type, `StatsRepository::summary(&StatsQuery, now)` (one range query with `LEFT JOIN download_sources` once T020 lands; until then select NULL source columns) and the `range` section of `aggregate()` in `putio-sync-db/src/analytics.rs`; export from `putio-sync-db/src/lib.rs`
- [x] T005 Write failing API tests in `putio-sync-api/tests/stats_summary.rs`: 401 without token; 400 for bad `range`, `tz` (non-number, > 840), `category`, `source`, `private`, over-long `ext`; 200 with every top-level section present on an empty DB
- [x] T006 Implement `GET /stats/summary` in `putio-sync-api/src/routes/system/stats.rs`: parse query into `StatsQuery` (empty value = absent), call `summary`, map DB errors to 500
- [x] T007 [P] Router: in `putio-sync-api/ui/app.js` match the page on the part before `?` and pass `query: URLSearchParams` in the page params; keep the query in the remembered page
- [x] T008 [P] Page shell in `putio-sync-api/ui/pages/stats.js`: head, filter toolbar (range segmented buttons; type, source, tracker, privacy selects; Clear), state ⇄ hash via `history.replaceState`, fetch with a sequence guard, error card with Retry, Refresh button; CSS for the segmented control and tiles in `putio-sync-api/ui/app.css`
- [x] T009 Seed the mock server in `putio-sync-api/examples/mock_ui.rs`: ~120 days of downloads with an evening-heavy pattern and a mix of extensions (video, audio, archive, subtitle, document, none) via `insert_download_at`

**Checkpoint**: `/stats/summary` answers with the range section; the page loads, filters
round-trip through the URL.

---

## Phase 3: User Story 1 — Bandwidth over time (P1) 🎯 MVP

**Goal**: totals tiles and the bandwidth bar chart.

**Independent test**: seeded downloads over 30 days; totals and per-bucket bytes match.

- [x] T010 [US1] Failing tests in `putio-sync-db/src/analytics.rs`: totals (files, bytes, seconds, avg_bps); a 2-hour download across two hourly buckets splits bytes by overlap; a download starting before `since` only counts its in-range share in `series` but counts fully in totals; zero/NULL duration puts all bytes in the finish bucket; `deleted` rows ignored
- [x] T011 [US1] Implement `totals` and `series` in `aggregate()` in `putio-sync-db/src/analytics.rs`
- [x] T012 [US1] API test in `putio-sync-api/tests/stats_summary.rs`: seeded rows → expected totals and series length for `range=30d`
- [x] T013 [US1] Totals tiles and SVG bandwidth chart with HTML axis labels, pointer/tap/arrow-key selection and readout in `putio-sync-api/ui/pages/stats.js`; chart CSS in `putio-sync-api/ui/app.css`

**Checkpoint**: MVP — the page shows totals and the bandwidth chart for any range.

---

## Phase 4: User Story 2 — When downloads happen (P2)

**Goal**: 7 × 24 heatmap by bytes or files, in local time.

**Independent test**: downloads at known UTC times with `tz=-300` land in the expected
local cells.

- [x] T014 [US2] Failing test in `putio-sync-db/src/analytics.rs`: heatmap cells by local finish time (Monday row 0), for `tz` 0 and a negative offset that moves a download to the previous day
- [x] T015 [US2] Implement `heatmap` in `aggregate()` in `putio-sync-db/src/analytics.rs`
- [x] T016 [US2] Heatmap card (CSS grid, five-step legend, Bytes/Files toggle, selection readout, arrow keys) in `putio-sync-api/ui/pages/stats.js` and `putio-sync-api/ui/app.css`

---

## Phase 5: User Story 3 — File types (P2)

**Goal**: category/extension breakdown and the type filter.

**Independent test**: mixed extensions; breakdown and filtered totals match.

- [x] T017 [US3] Failing tests in `putio-sync-db/src/analytics.rs`: category mapping (incl. upper-case and missing extension → `(none)`/other); `types` sorted by bytes with ≤ 8 extensions plus `…other`; `category` and `ext` filters apply to totals, series and heatmap; `options.extensions` ignores the type filter
- [x] T018 [US3] Implement `Category`, the `types` section, the category/extension filters and `options.extensions` in `putio-sync-db/src/analytics.rs`
- [x] T019 [US3] File types card with clickable rows setting the type filter; type select filled from `options.extensions` in `putio-sync-api/ui/pages/stats.js`

---

## Phase 6: User Story 4 — Sources (P3)

**Goal**: record each queued file's transfer type, tracker host and private flag; source
breakdown and filters.

**Independent test**: mocked transfer list with a private-tracker torrent (passkey in
URL), a public torrent and a URL transfer; after a sync the sources are recorded with
hosts only and the page splits them.

- [x] T020 [P] [US4] `CREATE_DOWNLOAD_SOURCES_TABLE` in `putio-sync-db/src/schema.rs`, created on open in `putio-sync-db/src/lib.rs`; `DownloadSource`/`SourceType` in `putio-sync-db/src/models.rs`; `SourcesRepository::{record, get}` in new `putio-sync-db/src/sources.rs` with upsert tests; `db.sources()`
- [x] T021 [P] [US4] Failing tests then host-only deserializers in `putio-sync-client/src/models.rs`: `tracker` URL with passkey path/query → host; `host:port` → host; garbage → None; comma/space lists → first host; `magneturi` `tr=` fallback; `Transfer` serialises `transfer_type` and `tracker_host` but no magnet/source field; existing fixtures without these fields still parse
- [x] T022 [US4] Failing tests in `putio-sync-download/src/manager.rs` (mockito): `enqueue_tree` records sources for a file inside a transfer folder and for a single-file transfer; no `list_transfers` call when nothing is queued; a 500 from `/transfers/list` still queues files with no source
- [x] T023 [US4] Implement the lazy transfer map, `source_for` parent-chain walk and recording after a successful `queue.add` in `putio-sync-download/src/manager.rs`
- [x] T024 [US4] Failing tests then `sources` section (types, trackers top 10 + `other trackers` + `no tracker`, privacy), `source`/`tracker`/`private` filters and `options.trackers` in `putio-sync-db/src/analytics.rs`; switch the row query to the real `LEFT JOIN`
- [x] T025 [US4] Passkey sweep in `putio-sync-api/tests/secrets_sweep.rs`: a transfer whose tracker URL holds a passkey; `/putio/transfers` and `/stats/summary` bodies and the `download_sources` rows never contain it
- [x] T031 [US4] `CREATE_TRANSFER_ORIGINS_TABLE` and `TransferOriginsRepository::{record, get}` in `putio-sync-db`; `download_sources` gains `origin`, `watch_folder`, `putio_folder` with nullable transfer columns (R11)
- [x] T032 [US4] Watcher records `transfer_origins(id, 'watch', watch_dir)` after a successful upload in `putio-sync-file-manager/src/watcher.rs` (watch dir = configured dir containing the file, else its parent), with a test
- [x] T033 [US4] `POST /putio/transfers` records `transfer_origins(id, 'manual')` in `putio-sync-api/src/routes/putio/transfers.rs`, with a test
- [x] T034 [US4] `enqueue_tree` records a source row for every queued file: origin from `transfer_origins` (else `putio` when matched, else `unknown`) and `putio_folder` from the relative path; tests in `putio-sync-download/src/manager.rs`
- [x] T035 [US4] `origins`, `watch_folders`, `folders` sections, `origin`/`watch`/`folder` filters and options in `putio-sync-db/src/analytics.rs`, with tests
- [x] T026 [US4] Origin and Sources cards (three bar lists, clickable rows, DHT note) and tracker select in `putio-sync-api/ui/pages/stats.js`; seed sources in `putio-sync-api/examples/mock_ui.rs` (two public trackers, one private, URL, unknown; two watch folders, manual, Put.io; several Put.io folders) and add `type`/`tracker`/`is_private` to its mock transfers

---

## Phase 7: Polish & cross-cutting

- [x] T027 [P] `putio-sync-api/tests/ui_assets.rs`: `pages/stats.js` is served; total still < 150 KB
- [x] T028 [P] README: Stats page section (what it shows, where the data comes from, that sources start with this version, no DHT data) in `README.md`
- [x] T029 Browser check per [quickstart.md](quickstart.md) against `mock_ui` at desktop and 375 px widths, light and dark
- [x] T030 `TMPDIR=<scratchpad> make ci` green

---

## Dependencies

- T001 → T008. T002 → T009, T012. T003 → T004 → T006 (T005 before T006).
- Foundational (T002–T009) blocks all stories.
- US1 → nothing else; US2 and US3 extend `aggregate()` and stats.js, so run them after US1
  to avoid conflicting edits in the same files. US4's T020–T023 touch other crates and can
  run alongside US2/US3; T024–T026 come after US3.
- Polish last.

## Parallel examples

- Foundational: T002, T007 and T008 together (different files).
- US4: T020 and T021 together (db vs client crate).

## Implementation strategy

1. Setup + Foundational, then US1: a working Stats page with bandwidth — stop and check.
2. US2 and US3 add the heatmap and types to the same page and endpoint.
3. US4 last: it changes the sync path and needs the passkey sweep.
4. Polish: assets test, README, browser check, `make ci`.
