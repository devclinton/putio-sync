# Tasks: Local Files Page

**Input**: Design documents from `/specs/044-local-files/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/api.md

**Tests**: Requested (API tests in `putio-sync-api/tests/`, path-check unit tests).

## Phase 1: Setup

- [x] T001 Create `putio-sync-file-manager/src/local_files.rs` with a `LocalFileError` enum (`Invalid`, `NotFound`, `Conflict`, `Io`) and export it from `putio-sync-file-manager/src/lib.rs`

## Phase 2: Foundational

- [x] T002 Implement `resolve(root, rel)` in `putio-sync-file-manager/src/local_files.rs` per research R1: reject absolute paths, `.`/`..`/empty middle segments, `\` and NUL; canonicalize root and the target's parent; require the parent under the canonical root; do not follow the last segment
- [x] T003 Unit tests for `resolve` in `putio-sync-file-manager/src/local_files.rs`: `..`, absolute, backslash, symlinked parent that escapes, symlinked root allowed, link as last segment not followed
- [x] T004 [P] Add `DownloadManager::partial_paths_in_use()` returning the `.downloading` path of each target in `paths_in_use` in `putio-sync-download/src/manager.rs`
- [x] T005 Rewrite `putio-sync-api/src/routes/local/files.rs`: roots from live config (`downloads`, `watch-N` only with `[watch] enabled`), error mapping 400/404/409, and remove the old unchecked handlers

## Phase 3: User Story 1 - See what is on the NAS (P1) 🎯 MVP

**Independent test**: `GET /local/files` and `GET /local/files/{root}?path=` over temp roots return roots and entries with kind, size, modified, `partial`, `rejected`, `in_use`.

- [x] T006 [US1] Implement `list(dir, limit)` (2,000 cap, folders first, case-insensitive names, `partial`/`rejected` marks, `valid_name`) in `putio-sync-file-manager/src/local_files.rs`
- [x] T007 [US1] Add `GET /local/files` and `GET /local/files/:root` in `putio-sync-api/src/routes/local/files.rs`, setting `in_use` from T004
- [x] T008 [P] [US1] API tests in `putio-sync-api/tests/local_files.rs`: roots with watch on/off, missing root, listing marks, traversal refused, auth required
- [x] T009 [US1] Page `putio-sync-api/ui/pages/files.js`: root sections, path bar, table with name, badges (Partial, Rejected, In use, Link), size, modified; register in `putio-sync-api/src/ui.rs`, `putio-sync-api/ui/app.js` and the nav in `putio-sync-api/ui/index.html`

## Phase 4: User Story 2 - Delete (P1)

**Independent test**: DELETE removes a file, a folder and a link (not its target); refuses the root, traversal and in-use partials.

- [x] T010 [US2] Implement `summary()` and `delete()` in `putio-sync-file-manager/src/local_files.rs` (no link following; `remove_dir_all` for real folders)
- [x] T011 [US2] Add `GET /local/files/:root/summary` and `DELETE /local/files/:root` with the in-use check and structured logs in `putio-sync-api/src/routes/local/files.rs`
- [x] T012 [P] [US2] API tests in `putio-sync-api/tests/local_files.rs` for delete cases above
- [x] T013 [US2] Delete button with `confirm()` naming the entry and, for folders, files and size from summary, in `putio-sync-api/ui/pages/files.js`

## Phase 5: User Story 3 - Re-queue a rejected watch file (P2)

**Independent test**: a recorded `x.magnet.rejected` becomes `x.magnet` and is no longer processed.

- [x] T014 [P] [US3] Add `ProcessedTorrentsRepository::forget(checksum)` in `putio-sync-db/src/torrents.rs`
- [x] T015 [P] [US3] Add `TorrentWatcher::forget(path)` in `putio-sync-file-manager/src/watcher.rs`
- [x] T016 [US3] Implement `requeue(path, database, watcher)` per research R4 in `putio-sync-file-manager/src/local_files.rs`
- [x] T017 [US3] Add `POST /local/files/:root/requeue` (watch roots only) in `putio-sync-api/src/routes/local/files.rs`
- [x] T018 [P] [US3] API tests in `putio-sync-api/tests/local_files.rs`: success clears the record, taken name 409, non-watch root 400, non-`.rejected` 400
- [x] T019 [US3] "Re-check" button on rejected entries in watch roots in `putio-sync-api/ui/pages/files.js`

## Phase 6: User Story 4 - Rename (P3)

- [x] T020 [US4] Implement `rename(path, name)` (name: "one segment, not `.`/`..`, no `/`, `\` or NUL, at most 255 bytes, and free") in `putio-sync-file-manager/src/local_files.rs`
- [x] T021 [US4] Add `POST /local/files/:root/rename` in `putio-sync-api/src/routes/local/files.rs`
- [x] T022 [P] [US4] API tests in `putio-sync-api/tests/local_files.rs`: rename ok, bad names 400, taken 409
- [x] T023 [US4] Rename button with an inline name field in `putio-sync-api/ui/pages/files.js`

## Phase 7: Polish

- [x] T024 Update `putio-sync-api/tests/ui_assets.rs` page list and `putio-sync-api/tests/secrets_sweep.rs` route paths; keep the UI under the 260 KB budget
- [x] T025 [P] CHANGELOG.md Unreleased line; README API list if it names `/local/files`
- [x] T026 `cargo fmt`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test -p putio-sync-api -p putio-sync-file-manager -p putio-sync-db -p putio-sync-download`

## Dependencies

- T001 → T002 → T003; T002 blocks every story. T004 and T005 block US1/US2 routes.
- US1 → US2 (delete uses the listing page). US3 and US4 depend only on Phase 2 and the page from T009.

## Parallel examples

- T004 with T002/T003 (different crates); T014 with T015; each story's test task with its UI task.

## Implementation strategy

MVP is US1 + US2 (see and delete). US3 and US4 follow in the same branch.
