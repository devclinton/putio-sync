# Implementation Plan: Stalled Put.io Transfer Detection and Re-add

**Branch**: `007-stalled-transfers` | **Date**: 2026-10-26 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/007-stalled-transfers/spec.md`

## Summary

The daemon polls the Put.io transfer list every 10 minutes and feeds it to a `StallTracker`
(in `putio-sync-core`) that remembers, per downloading transfer, when its byte count last
changed. A transfer unchanged for 6 hours is stalled, with a reason (low availability, no
peers, or no progress). The count goes to a new gauge `putio_sync_transfers_stalled`. The API
adds `stall`, `availability` and `magnet` to each transfer and a `POST
/putio/transfers/{id}/readd` route (cancel, then add the same magnet to the same folder). The
Transfers page shows a Stalled badge, Copy magnet and Re-add.

The client model gains the transfer fields Put.io sends but we ignored (`magneturi`, `source`,
`type`, `status_message`, `tracker_message`, `torrent_link`) and accepts `current_ratio` as
text. Decisions are in [research.md](research.md) (R1–R10).

## Technical Context

**Language/Version**: Rust 1.96 (`rust-toolchain.toml`), edition 2021. Browser side: ES2020
modules, no build step (feature 004).

**Primary Dependencies**: existing only: tokio, reqwest, serde, axum, prometheus, tracing.

**Storage**: in memory (research R4); no schema change.

**Testing**: `cargo test`; mockito for Put.io (already used in client and api tests); injected
clock for the tracker.

**Target Platform**: Linux Docker on the NAS; desktop CLI builds unaffected.

**Project Type**: Cargo workspace (daemon + HTTP API + embedded web UI).

**Performance Goals**: one extra `GET /transfers/list` per check interval (default 600 s).

**Constraints**: magnets never logged; no per-transfer metric labels.

**Scale/Scope**: tens of transfers.

## Constitution Check

| Principle | Status |
|---|---|
| I. Crate boundaries | Tracker in `core` (shared by daemon and api); scheduling in `daemon`; Put.io fields in `client`; gauge in `telemetry`; route glue in `api`. No upward deps. ✅ |
| II. Local-first | `make ci` covers everything. ✅ |
| III. Tests without Put.io | mockito + injected clock; UI checked via `mock_ui`. ✅ |
| IV. Download integrity | Re-add deletes no files; it cancels one transfer on an explicit, confirmed user action. Not a bulk or automatic destructive operation, so no `--dry-run`. ✅ |
| V. Observable | New gauge, no labels; structured logs with `transfer_id`/`name`. ✅ |
| VI. Secrets | Route behind auth; magnet not logged (not a secret, but identifying). ✅ |
| VII. Simplicity | No new deps; two config keys required by FR-005; no persistence. ✅ |

Post-design re-check: unchanged, no violations.

## Project Structure

### Documentation (this feature)

```text
specs/007-stalled-transfers/
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
putio-sync-client/src/models.rs          # new Transfer fields, magnet(), lenient current_ratio
putio-sync-core/src/stall.rs             # StallTracker, Stall, StallReason (new)
putio-sync-core/src/config.rs            # [transfers] TransfersConfig
putio-sync-telemetry/src/lib.rs          # transfers_stalled gauge + setter
putio-sync-telemetry/tests/contract.rs   # family added to BASIC_FAMILIES
putio-sync-daemon/src/daemon.rs          # stall check loop, tracker accessor
putio-sync-cli/src/commands/daemon.rs    # pass config + tracker into AppState
putio-sync-api/src/lib.rs                # AppState.stall_tracker, with_stall_tracker()
putio-sync-api/src/models/transfers.rs   # availability, magnet, stall; ReaddResponse
putio-sync-api/src/handlers/transfers.rs # list/get attach stall; readd_transfer
putio-sync-api/src/routes/putio/transfers.rs # POST /:id/readd
putio-sync-api/tests/transfer_readd.rs   # route tests (new)
putio-sync-api/ui/pages/transfers.js     # badge, Copy magnet, Re-add
putio-sync-api/examples/mock_ui.rs       # stalled fixtures
config.example.toml, README.md           # [transfers], route, metric
```

**Structure Decision**: existing workspace layout; one new module (`core::stall`) and one new
test file.

## Complexity Tracking

None.
