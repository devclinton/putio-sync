# Implementation Plan: Stats Page

**Branch**: `006-stats-page` | **Date**: 2026-10-26 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/006-stats-page/spec.md`

## Summary

A new Stats page in the web UI charts bandwidth over time, a day-of-week × hour heatmap,
a file-type breakdown and a source breakdown, all filtered by date range, file type and
source. It reads one new authenticated endpoint, `GET /stats/summary`, which scans the
daemon's own `download_history` for the range and folds it into small pre-aggregated JSON
in Rust (no Prometheus).

To split by source, the folder sync now records, for each file it queues, the Put.io
transfer's type, tracker host name and private flag in a new `download_sources` table.
The tracker address is cut down to its host while it is parsed, so passkeys never reach
memory structs, storage, logs or responses.

Charts are hand-written SVG and CSS inside the existing framework-free UI.

Decisions are in [research.md](research.md) (R1–R10).

## Technical Context

**Language/Version**: Rust 1.96 (`rust-toolchain.toml`), edition 2021. Browser side:
HTML, CSS, ES2020 modules, no build step.

**Primary Dependencies**: existing only — axum 0.7, sqlx (SQLite), chrono (already in
`putio-sync-db`), serde, `reqwest::Url` (already in `putio-sync-client`) for host
parsing. No new crate, no chart library.

**Storage**: SQLite: new table `download_sources` (R4); `download_history` unchanged.

**Testing**: `cargo test`. Pure unit tests for the aggregation (`analytics.rs`), host
parsing and source matching; repository tests on temp SQLite; API integration tests with
`tower::ServiceExt::oneshot` and mockito; the `mock_ui` example seeded for a browser
check.

**Target Platform**: Linux container on Unraid; macOS for development; current Safari,
Chrome and Firefox on desktop and phone.

**Project Type**: Cargo workspace; the UI is static files embedded in `putio-sync-api`.

**Performance Goals**: `/stats/summary` under 200 ms for 20,000 history rows on the NAS;
responses under 50 KB (SC-001, SC-003).

**Constraints**: UI total < 150 KB (now ~83 KB; budget for this feature ~25 KB); no full
tracker URL anywhere (FR-011); a failed transfer listing never blocks a sync (FR-012).

**Scale/Scope**: one page, one new endpoint, one changed response, one new table, one
new Put.io call per sync that queues something.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Status |
|---|---|---|
| I. Crate Boundaries | Code in the owning crate; no upward deps | ✅ Host parsing and new `Transfer` fields in `client`; table, repository and aggregation in `db`; source matching in `download` (`enqueue_tree`); route in `api`; page in `api/ui`. No new crate edges. |
| II. Local-First Verification | `make ci` is the gate | ✅ All tests are `cargo test`; the UI asset size test already guards the budget. |
| III. Tests Without Put.io | mocks only | ✅ mockito transfer lists; seeded temp DBs; `mock_ui` for the browser check. |
| IV. Download Integrity | no false complete | ✅ Download and completion paths untouched; source recording happens after a successful queue add and its failure is only logged. |
| V. Observable by Default | structured logs, bounded metrics | ✅ One structured `warn!` when the transfer list fails; no new metric labels (the existing Put.io API call metrics count the extra call). |
| VI. Secrets Stay Secret | no secrets in logs/responses; auth required | ✅ `/stats/summary` sits behind the token middleware. Tracker URLs reduced to host at parse time (R5); a test sweeps DB rows and responses for a seeded passkey. |
| VII. Simplicity | no new deps or knobs | ✅ No new dependency, no config setting. One table rather than columns on two tables (R4). |

**Post-design re-check**: passes; no Complexity Tracking entries.

## Project Structure

### Documentation (this feature)

```text
specs/006-stats-page/
├── spec.md
├── plan.md              # this file
├── research.md          # R1–R10
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── api.md           # GET /stats/summary, transfer fields
│   └── ui.md            # the Stats page
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-client/src/models.rs        # Transfer: transfer_type, tracker_host, magnet_tracker_host; host-only deserializers

putio-sync-db/src/
├── schema.rs                          # CREATE_DOWNLOAD_SOURCES_TABLE
├── lib.rs                             # create table on open; db.sources(); re-exports
├── models.rs                          # DownloadSource, SourceType
├── sources.rs                         # NEW: SourcesRepository::record / get
├── history.rs                         # insert_download_at (explicit finish time)
└── analytics.rs                       # NEW: StatsQuery, Category, BucketSize, aggregate(), StatsRepository::summary()

putio-sync-download/src/manager.rs     # enqueue_tree: lazy transfer map, parent-chain match, record source

putio-sync-api/
├── src/routes/system/stats.rs         # GET /stats/summary (query parsing → db summary)
├── tests/stats_summary.rs             # NEW: endpoint tests
├── tests/secrets_sweep.rs             # + tracker passkey never stored or returned
├── tests/ui_assets.rs                 # + stats.js served; budget still < 150 KB
├── examples/mock_ui.rs                # seed ~120 days of history and sources
└── ui/
    ├── index.html                     # nav link
    ├── app.js                         # router: ignore ?query for matching, pass params.query
    ├── app.css                        # chart, heatmap, tiles, segmented control
    └── pages/stats.js                 # NEW: the page

README.md                              # Stats page section
```

**Structure Decision**: follows the crate split above; no new crates.

## Implementation Notes

- **Matching (R4)**: `enqueue_tree` keeps the full listing (folders included) to build a
  `parent_id` map before it filters to files. The transfer map is fetched the first time a
  file is about to be queued. `fn source_for(file_id, parents, transfers)` walks up at
  most 64 levels. Recording happens only when `queue.add` succeeds.
- **Aggregation (R2, R3, R6)**: `aggregate(rows, &StatsQuery, now)` is pure and returns
  `StatsSummary`; `StatsRepository::summary(&StatsQuery, now)` runs the range query and
  calls it. Local time = UTC + offset; bucket index from the aligned local start.
- **Route**: parses the query into `StatsQuery` (400 on bad values) and returns the
  summary as JSON; `now` is `Utc::now()`.
- **UI size**: stats.js target ≤ 18 KB, CSS additions ≤ 3 KB.

## Complexity Tracking

None.
