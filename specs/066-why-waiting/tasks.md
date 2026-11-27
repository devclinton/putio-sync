# Tasks: Say why something is waiting (066)

## Phase 1: Reason logic

- [x] T001 `waiting.rs`: `Waiting`, the code list, `bandwidth_hold`
- [x] T002 `QueueState::gate` and `for_row`, in the workers' order
- [x] T003 `for_pending_files` for an Active item's pending files
- [x] T004 Unit tests for every code and the order of the gates

## Phase 2: Daemon and API

- [x] T005 `DownloadManager::queue_state`; record the failure kind where a download fails
- [x] T006 `waiting` on `GET /local/queue` rows
- [x] T007 `PipelineItem.waiting` and `BuildInput.queue`; `waiting` on `GET /active` items
- [x] T008 Tests: pipeline items, queue route

## Phase 3: Web UI

- [x] T009 `ui/waiting.js`: words, setting link and guide for each code; registered in `ui.rs`
- [x] T010 `reasonLine` in `app.js`; shown on Active and Queue rows
- [x] T011 Line for skipped files and for a Put.io error on the Active page
- [x] T012 `tests/ui/waiting.test.mjs`

## Phase 4: Words and docs

- [x] T013 `make i18n`; new strings in all 26 locales
- [x] T014 README and CHANGELOG
- [x] T015 All checks pass
