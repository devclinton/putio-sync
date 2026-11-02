# Implementation Plan: Active Transfers Pipeline View

**Branch**: `039-transfer-pipeline-view` | **Date**: 2026-10-28 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/039-transfer-pipeline-view/spec.md`

## Summary

A new `pipeline` module in `putio-sync-download` joins three sources into one item per
transfer: the Put.io transfer list, the download queue, and `download_sources` (which already
maps each queued file to its transfer). Each item gets a stage (Put.io or NAS) and a state,
plus its progress, speed and time left. The Put.io list comes from a shared snapshot that is
at most 10 seconds old, so open pages never add Put.io calls.

The API serves `GET /active` and pause/resume routes. A new Active page shows the counts,
the rows and Recently finished. The Status page's Active downloads card becomes a short
Active transfers summary.

Pausing works at two levels:
- **One transfer or one file**: a real stop for a running download, which keeps its partial
  file for resume. Today pausing only changes the queue row and never stops a running file.
- **A transfer still on Put.io**: a hold. Put.io has no pause, so its files are queued paused
  when they arrive.

The Queue page's Pause all and Resume all become one lasting **Pause NAS downloads** switch,
stored in the database. A daemon loop records Recently finished, clears stale holds, and sets
new gauges. Paused Radarr/Sonarr items are reported to the *arr as paused.

Decisions are in [research.md](research.md) (R1–R17).

## Technical Context

**Language/Version**: Rust 1.96 (`rust-toolchain.toml`), edition 2021. Browser side: ES2020
modules, no build step (feature 004).

**Primary Dependencies**: existing only: tokio, reqwest, sqlx (SQLite), axum, serde, prometheus,
tracing. No new crates (R7).

**Storage**: SQLite. Three new tables: `pipeline_holds`, `pipeline_finished`, `daemon_flags`. No
change to existing tables.

**Testing**: `cargo test`. mockito for Put.io. Unit tests for `pipeline::build`. Synthetic local
HTTP payloads for stop/resume mid-file. API route tests. `mock_ui` for the pages.

**Target Platform**: Linux Docker on the NAS. Desktop CLI builds are unaffected.

**Project Type**: Cargo workspace (daemon, HTTP API, embedded web UI).

**Performance Goals**: `GET /active` in under 200 ms with 200 transfers and 5,000 queue rows. A
stop takes effect within 5 seconds (SC-007). At most one Put.io transfer list call every 10
seconds while pages are open, and every 60 seconds otherwise (R3, R13).

**Constraints**:
- No names or ids in metric labels.
- Pausing never cancels or removes anything on Put.io.
- A stopped download is never reported as complete.

**Scale/Scope**: tens of transfers normally; hundreds tested.

## Constitution Check

| Principle | Status |
|---|---|
| I. Crate boundaries | Pipeline build, holds, the stop signal and the switch live in `download` (owns the queue and enqueue). Tables and queries go in `db`. The loop goes in `daemon`, routes in `api`, gauges in `telemetry`, the paused view in `arr`, and `finished_retention_hours` in `core` config. No upward deps: `arr` already depends on `download`. ✅ |
| II. Local-first | Everything runs under `make ci`. ✅ |
| III. Tests without Put.io | `build` is pure and table-tested. mockito is used for the transfer list. A local HTTP server with a slow body tests stop and resume. A failing test is written before the stop fix (today's pause does not stop a running file). ✅ |
| IV. Download integrity | A stop keeps the partial under the temp name and never renames it. Resume reuses `check_resume`, which checks the size. A stopped file is never marked completed, and no Put.io delete runs for it. Pausing deletes nothing, so no `--dry-run` is needed. ✅ |
| V. Observable | Three new metric families with bounded labels (R15). Structured logs for pause, hold, resume and stop. ✅ |
| VI. Secrets | The new routes sit behind the existing API auth. No tokens or magnets in responses or logs. ✅ |
| VII. Simplicity | No new dependencies. One config key, which FR-026 requires. `QueueRepository::pause_all`/`resume_all` are removed instead of kept next to the switch. ✅ |

Post-design re-check (after data-model and contracts): unchanged, no violations.

## Project Structure

### Documentation (this feature)

```text
specs/039-transfer-pipeline-view/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/api.md
├── checklists/requirements.md
└── tasks.md              # /speckit-tasks, not created here
```

### Source Code (repository root)

```text
putio-sync-db/src/schema.rs              # pipeline_holds, pipeline_finished, daemon_flags
putio-sync-db/src/pipeline.rs            # holds, finished, flags repos; active rows join (new)
putio-sync-db/src/queue.rs               # pause_files/resume_files/retry_files, add(status); drop pause_all/resume_all
putio-sync-db/src/lib.rs                 # migrate + accessors
putio-sync-download/src/pipeline.rs      # PipelineItem, Stage, State, Counts, build() (new)
putio-sync-download/src/manager.rs       # transfers_snapshot, stop signals, hold on enqueue, switch check, pause/resume
putio-sync-download/src/stream.rs        # stop check between chunks and in limiter wait
putio-sync-download/src/error.rs         # DownloadError::Stopped
putio-sync-core/src/config.rs            # [transfers] finished_retention_hours
putio-sync-telemetry/src/pipeline.rs     # gauges + counter (new); contract test families
putio-sync-daemon/src/daemon.rs          # pipeline loop (10 s)
putio-sync-arr/src/view.rs, service.rs   # paused status for held/paused items
putio-sync-api/src/routes/active.rs      # GET /active, POST /active/pause, /active/resume (new)
putio-sync-api/src/routes/local/queue.rs # per-file pause/resume/retry; /pause,/resume = switch; ?transfer filter
putio-sync-api/src/routes/system/status.rs # `active` summary + `nas_paused`
putio-sync-api/tests/active.rs           # route tests (new)
putio-sync-api/ui/pages/active.js        # Active page (new)
putio-sync-api/ui/pages/status.js        # Active transfers card replaces Active downloads
putio-sync-api/ui/pages/queue.js         # switch button, per-file pause/resume/retry, transfer filter
putio-sync-api/ui/pages/transfers.js     # ?id= scrolls to and highlights the row
putio-sync-api/ui/index.html, app.js     # nav entry, route, switch banner
putio-sync-api/examples/mock_ui.rs       # fixtures for every state
config.example.toml, README.md, CHANGELOG.md
```

**Structure Decision**: existing workspace layout. New modules: `db::pipeline`,
`download::pipeline`, `telemetry::pipeline` and `api::routes::active`, plus one new test file
and one new UI page.

## Implementation order

1. **Stop a running download** (R7, R8): a failing test first, then the stop signal and
   `DownloadError::Stopped`. This fixes today's pause and underpins everything else.
2. **Database**: tables, repos, `pause_files`/`resume_files`, and `add` with a status.
3. **Pipeline**: `pipeline::build` with table tests for every row of R5 and R6, then the
   transfers snapshot.
4. **Holds and the switch** in the manager (R9–R11). Rewire the queue routes.
5. **API**: `/active` routes and the `/status` summary. **Daemon**: the loop, Recently
   finished and metrics.
6. **arr**: report paused items as paused (R14).
7. **UI**: Active page, Status card, Queue page changes and mock fixtures. Then docs and the
   changelog.

## Complexity Tracking

None.
