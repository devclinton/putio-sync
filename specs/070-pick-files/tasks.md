# Tasks: Pick files inside a transfer (070)

## Phase 1: Storage and matching

- [x] T001 Tables `transfer_choices` and `transfer_choice_files`; `ChoicesRepository`
- [x] T002 `choice.rs`: `locate`, `Choices::wanted`, `torrent_paths`, with unit tests
- [x] T003 `sources().transfer_ids()` and `decisions().remove_reason()`

## Phase 2: Sync and API

- [x] T004 The sync pass asks the choice before the rules; left-out files are recorded
- [x] T005 `transfer_files`, `set_choice` (applies to the queue), `clear_choice`
- [x] T006 Choices forgotten with their transfer or files
- [x] T007 `PipelineItem.chosen`; codes `choose_files` and `not_chosen`
- [x] T008 Routes: files, choice (PUT, DELETE), preview; `choice` and `choose_first` on add
- [x] T009 `choice` on listed transfers, `transfer_id` on queue rows
- [x] T010 Tests: `file_choice.rs`, `transfer_choice.rs`, pipeline unit test

## Phase 3: Web UI

- [x] T011 `ui/pick.js` and `tests/ui/pick.test.mjs`
- [x] T012 `ui/pick_dialog.js`: tree, Only this, All but this, hold, forget; in `ui.rs`
- [x] T013 "Choose files" on Transfers, Active and Queue rows
- [x] T014 "Choose files first" on the add form (`.torrent` preview, magnet hold)
- [x] T015 `waiting.js`: the two codes and the skipped line of a chosen transfer
- [x] T016 Checked in a browser on the mock UI (fixtures in `mock_ui.rs`)

## Phase 4: Words and docs

- [x] T017 `make i18n`; new strings in all 26 locales
- [x] T018 README, CHANGELOG, ROADMAP
- [x] T019 All checks pass
