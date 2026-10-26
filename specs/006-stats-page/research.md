# Research: Stats Page

Decisions for feature 006. Each has the decision, why, and what else was considered.

## R1 — Data source: the daemon's own history, not Prometheus

**Decision**: The page reads only `download_history` (plus the new `download_sources`, R4)
through a new `/stats/summary` endpoint. No Prometheus queries.

**Rationale**:
- `download_history` already holds, per finished download: finish time (`timestamp`, UTC
  `YYYY-MM-DD HH:MM:SS`), file name, lower-case `extension`, `file_size`,
  `duration_seconds` and `average_speed`. That covers bandwidth over time, the heatmap and
  the type breakdown.
- Prometheus metrics (feature 003) are counters and histograms without file labels
  (Principle V forbids high-cardinality labels), so they can't be filtered by type or
  source. The daemon also has no Prometheus URL to query, and the page would break whenever
  the monitoring stack is down.
- Confirmed with the user (spec Clarifications).

**Alternatives**: Prometheus only (no per-type filters, external dependency); both (adds a
config knob and a second code path for one live-throughput panel the Status page already
covers).

## R2 — Bandwidth reconstruction

**Decision**: Each download contributes `file_size` bytes spread evenly over
`[finish − duration, finish]`, clipped to the range; a bucket gets the share of bytes whose
time falls inside it. A download with no or zero duration puts all bytes in its finish
bucket. File counts, totals, heatmap and breakdowns use the finish time only.

**Rationale**: Good enough to show load over time without logging samples. Duration is
wall time of the final, successful attempt (resumed downloads report the resumed part's
time), so throughput may read slightly high for resumed files; acceptable for a trend
chart.

**Alternatives**: A new periodic throughput sample table (more writes, a retention policy,
and no history for existing rows); bytes by finish time only (a 40 GB file shows as one
spike).

## R3 — Aggregate in Rust over a range-filtered row scan

**Decision**: One SQL query selects the slim columns (`timestamp`, `file_size`,
`extension`, `duration_seconds`, source columns via `LEFT JOIN download_sources`) for
`action = 'downloaded'` rows in the date range. A pure function in
`putio-sync-db/src/analytics.rs` applies the type/source filters and builds every section
of the summary in one pass. The filter options (extensions and trackers present) come from
the same unfiltered-by-type/source rows.

**Rationale**: 20,000 rows of five small columns is a few hundred KB in memory and
milliseconds to fold (SC-001). Pure Rust aggregation is unit-testable without SQL edge
cases (time-zone shifts, bucket splitting, week alignment), and keeps the SQL to one
indexed range scan (`idx_history_action_timestamp`).

**Alternatives**: Several `GROUP BY` queries with `strftime` (time-zone and spreading
logic in SQL is awkward and hard to test; one round trip per section).

## R4 — Recording the source of a download

**Decision**: A new table `download_sources` keyed by Put.io `file_id` holds
`source_type` (`torrent`, `url`, `other`), `tracker_host` (nullable) and `is_private`.
`DownloadManager::enqueue_tree` fetches `GET /transfers/list` lazily, the first time it is
about to add a file to the queue in that run (so a sync with nothing new costs no extra
call), maps each transfer's `file_id` to its source, and resolves each queued file by
walking up its parent chain (from the listing it already has) until it hits a transfer's
`file_id`. A match is written with `INSERT … ON CONFLICT(file_id) DO UPDATE`. A failed
transfer listing logs one warning and leaves sources unknown (FR-012).

**Rationale**:
- Put.io's transfer object (go-putio `Transfer`): `type` (`TORRENT`, `URL`, `PLAYLIST`, …),
  `tracker`, `magneturi`, `source`, `is_private`, `file_id`. There is nothing about DHT;
  `is_private` is the nearest signal, since private torrents disable DHT and PEX.
- A separate table, joined at query time, leaves the history insert sites and the queue
  untouched, and survives queue cleanup. Old rows simply have no match ("unknown").
- Matching by `file_id` ancestry is exact for the usual case (a transfer's file or folder
  is where the sync finds its files). Finished transfers that Put.io has already cleared
  from the list don't match; they show as unknown.

**Alternatives**: Columns on `download_queue` copied into history at completion (touches
both completion paths and the queue schema); polling transfers on a timer (extra calls
when nothing is queued).

## R5 — Tracker host only, reduced while parsing

**Decision**: `putio-sync-client` deserialises `tracker` and `magneturi` through a
deserializer that keeps only the host name (lower-case, no port, no user info, path or
query) and discards the rest immediately. `source` is not deserialised at all. If
`tracker` yields no host, the first `tr=` parameter of `magneturi` is used.

**Rationale**: Private tracker announce URLs carry passkeys (`/announce?passkey=…` or
`/<passkey>/announce`). Dropping everything but the host at parse time means the full
address never lives in a struct that could be logged with `{:?}`, stored or serialised
back out by `/putio/transfers` (FR-011, SC-005, Principle VI).

**Alternatives**: Keep the URL and strip it later (one careless `debug!` leaks it).

**Parsing rules**: take the first whitespace- or comma-separated token; parse it as a URL
(`reqwest::Url`) and take `host_str()`; if it doesn't parse as a URL, accept it only if it
is a bare host name (letters, digits, `.` and `-`, optional `:port` stripped). Anything
else is `None`.

## R6 — Time buckets and time zone

**Decision**: The client sends `tz` = minutes east of UTC (`-new Date().getTimezoneOffset()`).
The server shifts times by that fixed offset, aligns buckets to local boundaries (hour,
local midnight, Monday 00:00 for weeks) and returns bucket starts as UTC epoch seconds.
Bucket size is the smallest of hour, 6 hours, day, week and 4 weeks that gives at most 120
buckets for the span: `24h` → hour (24 bars), `7d` → 6 hours (28), `30d` → day (30),
`90d` → day (90), `1y` → week (53), `all` → whatever fits the oldest download. Six-hour
buckets start at local 00, 06, 12 and 18.

**Rationale**: One fixed offset is what the browser can cheaply provide and is exact
except across a DST change inside the range (spec Edge Cases accepts an hour's shift).

**Alternatives**: IANA zone name and `chrono-tz` (new dependency, Principle VII).

## R7 — Charts without a library

**Decision**: Hand-written SVG for the bandwidth bars; a CSS grid of 168 cells for the
heatmap; HTML bar rows for the type and source breakdowns. Axis and tick labels are HTML
outside the SVG so text never stretches. Each chart is one focusable element with an
`aria-label`, and pointer/tap or arrow keys move a selection whose values are shown in a
text line under the chart (`aria-live="polite"`). The heatmap uses one hue at five
opacity steps plus the text readout, so colour is never the only signal (FR-013).

**Rationale**: Three chart shapes, each under ~60 lines; a library would cost tens of KB
against a 150 KB budget (FR-014) and add a dependency.

**Alternatives**: `<canvas>` (no DOM for accessibility, needs DPR handling); uPlot or
Chart.js (40–200 KB).

## R8 — Filter state in the URL

**Decision**: The Stats route is `#/stats?range=7d&cat=video&ext=&source=&tracker=&private=`.
The router learns to ignore a `?query` when matching the page and passes it to the page as
`URLSearchParams`. The page updates the hash with `history.replaceState` (no `hashchange`,
so no remount) and the shell's "last page" memory keeps the query, so a reload restores the
filters (FR-008).

## R9 — File categories

**Decision**: A fixed extension → category table in `analytics.rs` (spec Assumptions). The
API accepts `category` or `ext` (not both; `ext` wins). Extensions are reported as stored
(lower case); `NULL` is reported as `(none)`.

## R10 — Seeding for tests and the mock UI

**Decision**: `HistoryRepository::insert_download_at(…, timestamp)` writes a download row
with an explicit finish time, and `SourcesRepository::record` writes sources. Tests and the
`mock_ui` example use them; the mock seeds ~120 days of downloads with an evening-heavy
time pattern, a mix of categories and sources (two public trackers, one private, URL
transfers, unknowns).

## R11 — Origin and Put.io folder (follow-up request)

**Decision**: A new table `transfer_origins(transfer_id, origin, watch_folder)` is written
by the two places that create transfers with a database at hand: the watcher, after a
successful upload (`origin = 'watch'`, `watch_folder` = the configured watch directory
that contains the file, else the file's parent), and `POST /putio/transfers`
(`origin = 'manual'`). At queue time a file's origin is: the matched transfer's recorded
origin; else `putio` when it matched a transfer putio-sync didn't create; else `unknown`.
`download_sources` gains `origin`, `watch_folder` and `putio_folder` (the first segment of
the file's relative path, `(root)` for files at the Put.io root), and is now written for
every queued file, with the transfer columns `NULL` when no transfer matched.

**Rationale**: The transfer id is the only key shared by "the watcher uploaded X" and
"the sync found file Y". Recording it at creation is one insert in each path and needs no
extra Put.io call. The Put.io folder is already known at queue time (relative path).

**Alternatives**: Matching watcher uploads by name (names collide and change on Put.io);
asking Put.io for a transfer's creator (not exposed).

**Limits**: the CLI `create` command has no database, so its transfers count as `putio`.
Put.io uploads and transfers cleared before the sync are both `unknown`.
