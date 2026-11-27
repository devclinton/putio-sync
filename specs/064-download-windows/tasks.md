# Tasks: Download windows and a storage budget (064)

## Phase 1: Windows and disk pause

- [x] T001 `download_window.rs`: entries, `Windows`, `next_open`, `space_paused`, `validate`
- [x] T002 Config keys and validation; tests for midnight wrap, zones, both clock changes
- [x] T003 `gates.rs` and the rate limiter gate; `update_config` swaps the rules
- [x] T004 Waiting codes `download_window` and `disk_pause`; rows that were downloading

## Phase 2: Storage budget

- [x] T005 `plan::over_budget`; `build_plan` shared by the free-up and the budget
- [x] T006 `budget_preview` (dry run) and `budget_run`; run trigger `budget`; `cleanup.deleted`
- [x] T007 Daemon: `budget_run` after each storage refresh
- [x] T008 Tests: off by default, preview deletes nothing, oldest downloaded first, files
  without a confirmed local copy kept, recheck before delete

## Phase 3: Forecast

- [x] T009 `forecast.rs` and its tests
- [x] T010 `usage_samples` table and repository; a sample on each storage refresh
- [x] T011 `StorageService::forecast`; `GET /stats/forecast`

## Phase 4: API and web UI

- [x] T012 Settings fields, live lists, assistant help; `POST /putio/storage/budget/preview`
- [x] T013 `download_limits.js`; hours and disk pause card; budget switch with its preview lock
- [x] T014 Forecast card on Stats; storage card shows budget runs; `waiting.js` cases
- [x] T015 Node tests; mock UI seeds forecast samples

## Phase 5: Docs

- [x] T016 README, CHANGELOG, EVENTS.md, ROADMAP; 26 locales
