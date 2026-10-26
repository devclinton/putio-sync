# Implementation Plan: Multi-select and Bulk Actions

**Branch**: `008-bulk-actions` | **Date**: 2026-10-26 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/008-bulk-actions/spec.md`

## Summary

Four new API routes, each turning a list of ids into as few Put.io calls as Put.io allows:
`POST /putio/files/delete` (queue guard for every id, then one `/files/delete`),
`POST /putio/transfers/remove` (one `/transfers/cancel`), `POST /putio/transfers/retry` (one
`/transfers/retry` per id, per-id result) and `POST /putio/transfers/clear` (list, filter by
status group, one `/transfers/clean`, one `/transfers/cancel` for leftovers; dry run first).
The client gains `clean_transfers`. The Put.io and Transfers pages get a checkbox column,
select-all, and a selection toolbar; the Transfers page also gets Clear completed / failed /
all finished. Decisions are in [research.md](research.md) (R1–R8).

## Technical Context

**Language/Version**: Rust 1.96 (`rust-toolchain.toml`), edition 2021. Browser side: ES2020
modules, no build step (feature 004).

**Primary Dependencies**: existing only: tokio, reqwest, serde, axum.

**Storage**: none new; the queue table is read for the delete guard.

**Testing**: `cargo test`; mockito for Put.io in client and API tests; UI checked by hand via
`mock_ui`.

**Target Platform**: Linux Docker on the NAS; desktop CLI builds unaffected.

**Project Type**: Cargo workspace (daemon + HTTP API + embedded web UI).

**Performance Goals**: one Put.io call per bulk delete/remove; at most three per clear
(list, clean, cancel).

**Constraints**: names not logged for bulk actions, only counts.

**Scale/Scope**: tens to a few hundred rows.

## Constitution Check

| Principle | Status |
|---|---|
| I. Crate boundaries | `clean_transfers` in `client`; route glue and status grouping in `api`; UI in `api/ui`. ✅ |
| II. Local-first | `make ci` covers everything. ✅ |
| III. Tests without Put.io | mockito for every route and the client call; UI via `mock_ui`. ✅ |
| IV. Download integrity | Bulk delete keeps the queue guard (409 for the whole request). Clear chooses ids on the server, so it has a required `dry_run` (R5). Bulk delete/remove act on ids the operator picked and confirmed with the count, like their single-row versions. ✅ |
| V. Observable | Info logs with `count`; no new metrics needed (Put.io call metrics already cover them). ✅ |
| VI. Secrets | All routes under the token middleware. ✅ |
| VII. Simplicity | No new deps or config; existing request models reused; bulk Download reuses `POST /local/queue` (R7). ✅ |

Post-design re-check: unchanged, no violations.

## Project Structure

### Documentation (this feature)

```text
specs/008-bulk-actions/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/api.md
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-client/src/transfers.rs          # clean_transfers
putio-sync-api/src/models/transfers.rs      # StatusGroup, ClearRequest, ClearResponse, RetryManyResponse
putio-sync-api/src/handlers/transfers.rs    # remove_transfers, retry_transfers, clear_transfers
putio-sync-api/src/handlers/mod.rs          # unique_ids (dedup, empty → 400)
putio-sync-api/src/routes/putio/files.rs    # POST /delete with queue guard
putio-sync-api/src/routes/putio/transfers.rs # POST /remove, /retry, /clear
putio-sync-api/tests/bulk_files.rs          # new
putio-sync-api/tests/bulk_transfers.rs      # new
putio-sync-api/ui/pages/putio.js            # checkboxes, select-all, selection toolbar
putio-sync-api/ui/pages/transfers.js        # same + clear by status
putio-sync-api/ui/select.js                 # shared selection bar + row checkboxes (new)
putio-sync-api/ui/app.css                   # checkbox column, selection bar
putio-sync-api/src/ui.rs                    # embed select.js
putio-sync-api/examples/mock_ui.rs          # completed/seeding transfers, clean mock
README.md                                   # HTTP API table
```

**Structure Decision**: existing layout; two new test files.

## Complexity Tracking

None.
