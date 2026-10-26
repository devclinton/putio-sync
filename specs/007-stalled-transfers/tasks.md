---

description: "Task list for feature 007: Stalled Put.io Transfer Detection and Re-add"
---

# Tasks: Stalled Put.io Transfer Detection and Re-add

**Input**: Design documents from `specs/007-stalled-transfers/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/api.md](contracts/api.md), [quickstart.md](quickstart.md)

**Tests**: included (Constitution III). Write each test first and see it fail.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Foundational

- [x] T001 [P] Test then add to `Transfer` in `putio-sync-client/src/models.rs`: `magneturi`,
  `torrent_link`, `source`, `transfer_type` (`type`), `status_message`, `tracker_message`;
  `current_ratio` accepts number, numeric string or null (R9); `Transfer::magnet()` (R7).
- [x] T002 [P] Test then add `putio_sync_transfers_stalled` gauge and
  `Telemetry::set_transfers_stalled` in `putio-sync-telemetry/src/lib.rs`; add the family to
  `BASIC_FAMILIES` in `putio-sync-telemetry/tests/contract.rs` and to the 003 metrics contract
  doc.
- [x] T003 [P] Add `[transfers]` `TransfersConfig` (`stall_threshold_hours` 6,
  `stall_check_interval_secs` 600) to `putio-sync-core/src/config.rs` and
  `config.example.toml`.

## Phase 2: User Story 1 — See which transfers are stalled (P1) 🎯 MVP

- [x] T004 [US1] Tests then `StallTracker` in `putio-sync-core/src/stall.rs` (data-model
  transitions, reasons, injected clock); export from `lib.rs`.
- [x] T005 [US1] Daemon check task in `putio-sync-daemon/src/daemon.rs`: every interval, list
  transfers, `observe`, set the gauge; on error log and keep state. Accessor
  `stall_tracker()`. Test against mockito.
- [x] T006 [US1] `AppState.stall_tracker` + `with_stall_tracker()` in
  `putio-sync-api/src/lib.rs`; wire in `putio-sync-cli/src/commands/daemon.rs` (config →
  `DaemonConfig`, tracker → state).
- [x] T007 [US1] `TransferResponse` gains `availability`, `magnet`, `stall` in
  `putio-sync-api/src/models/transfers.rs`; list/get handlers attach the tracker's verdict.
  API test: seeded tracker → `stall` in `GET /putio/transfers`.
- [x] T008 [US1] Stalled badge and reason in `putio-sync-api/ui/pages/transfers.js`.

## Phase 3: User Story 2 — Copy magnet (P2)

- [x] T009 [US2] Copy magnet button (clipboard API, toast) in `transfers.js` when `magnet`
  present.

## Phase 4: User Story 3 — Re-add (P2)

- [x] T010 [US3] Tests `putio-sync-api/tests/transfer_readd.rs`: success (one cancel, one add
  with magnet + folder, new transfer returned), no magnet → 400 and no cancel, add fails after
  cancel → 500 with magnet in details, cancel fails → no add.
- [x] T011 [US3] `readd_transfer` handler + `POST /:id/readd` route; info log without magnet.
- [x] T012 [US3] Re-add button with confirm in `transfers.js` (shown when stalled and magnet).

## Phase 5: Polish

- [x] T013 [P] `putio-sync-api/examples/mock_ui.rs`: stalled, normal and magnet-less
  transfers; seed the tracker.
- [x] T014 [P] README: route, metric, `[transfers]` config.
- [x] T015 Run `make ci`; click through `mock_ui` (quickstart).

## Dependencies

T001–T003 first (parallel). T004 → T005, T006 → T007 → T008. T009 needs T007. T010 → T011 →
T012. T013–T015 last.
