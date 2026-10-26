---

description: "Task list for feature 008: Multi-select and Bulk Actions"
---

# Tasks: Multi-select and Bulk Actions

**Input**: Design documents from `specs/008-bulk-actions/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/api.md](contracts/api.md), [quickstart.md](quickstart.md)

**Tests**: included (Constitution III). Write each test first and see it fail.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Foundational

- [x] T001 [P] Test then add `PutioApiClient::clean_transfers(ids) -> Vec<i64>` in
  `putio-sync-client/src/transfers.rs` (`POST /transfers/clean {transfer_ids}`, answers
  `deleted_ids`; missing field → every id sent) (R1, R3).
- [x] T002 [P] `unique_ids` helper and empty-list 400 in `putio-sync-api/src/handlers/mod.rs`
  (R8).

## Phase 2: User Story 1 — Bulk delete / download on the Put.io page (P1) 🎯 MVP

- [x] T003 [US1] Tests `putio-sync-api/tests/bulk_files.rs`: one Put.io call with every id;
  any queued/downloading/paused id → 409 naming it, Put.io not called; empty → 400; dedup;
  without db the guard is skipped; token required.
- [x] T004 [US1] `POST /delete` in `putio-sync-api/src/routes/putio/files.rs` (R6).
- [x] T005 [US1] Checkbox column, select-all (mixed state), selection toolbar with Download
  (sequential `POST /local/queue`, summed toast), Delete (one confirm with the count) and Clear
  selection in `putio-sync-api/ui/pages/putio.js` via a shared `ui/select.js` (embedded in
  `src/ui.rs`); queued rows disabled; selection narrowed by
  the filter, cleared on folder change. Styles in `app.css`.

## Phase 3: User Story 2 — Bulk remove / retry on the Transfers page (P1)

- [x] T006 [US2] Tests `putio-sync-api/tests/bulk_transfers.rs`: remove → one cancel with
  every id; retry → one call per id, refusals reported per id; empty → 400.
- [x] T007 [US2] Handlers + `POST /remove`, `POST /retry` in
  `putio-sync-api/src/routes/putio/transfers.rs`.
- [x] T008 [US2] Checkbox column, select-all, selection toolbar (Retry, Remove, Clear
  selection) in `putio-sync-api/ui/pages/transfers.js`; selection survives the refresh.

## Phase 4: User Story 3 — Clear by status (P2)

- [x] T009 [US3] Tests in `bulk_transfers.rs`: dry run lists only the group and calls nothing
  but the list; real run sends one clean with the group's ids; leftovers go in one cancel;
  nothing matching → no clean; unknown status / missing `dry_run` → 400.
- [x] T010 [US3] `StatusGroup`, `ClearRequest`, `ClearResponse` in
  `putio-sync-api/src/models/transfers.rs`; `clear_transfers` handler; `POST /clear` route.
- [x] T011 [US3] Clear completed / failed / all finished buttons (preview → confirm with count
  → run) in `transfers.js`.

## Phase 5: Polish

- [x] T012 [P] `putio-sync-api/examples/mock_ui.rs`: completed and seeding transfers, clean
  mock; doc comment.
- [x] T013 [P] README HTTP API table.
- [x] T014 Run `make ci`; click through `mock_ui` (quickstart).

## Dependencies

T001–T002 first (parallel). T003 → T004 → T005. T006 → T007 → T008. T001 + T009 → T010 →
T011. T012–T014 last.
