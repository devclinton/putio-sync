# Tasks: 049 Radarr/Sonarr automatic setup including path mapping

**Input**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/](contracts/)

Tests are written with each piece (constitution III) against mockito Radarr/Sonarr servers.

## Phase 1: Setup

- [X] T001 Add `putio_sync_arr_path_mapping_total{result}` and the `ArrMappingResult` label enum in `putio-sync-telemetry/src/{labels.rs,lib.rs,arr.rs}`, pre-registered for every label

## Phase 2: Foundational (blocks every story)

- [X] T002 `RemotePathMapping.id` (optional), `create/update/delete_remote_path_mapping`, `root_folders()`, `folders(path) -> Vec<Folder{name, path}>` in `putio-sync-arr/src/arr_api.rs`, with mock tests
- [X] T003 `setup::connect(address, key, http)`: detect the app over `base_urls` (public wrapper of `detect_first`) in `putio-sync-arr/src/setup.rs`
- [X] T004 Refactor `target_of` in `putio-sync-api/src/routes/arr.rs` to take a target and key, so the new routes share it

## Phase 3: US1 Folder mapping worked out and created (P1) — MVP

- [X] T005 [US1] `path_mapping.rs`: marker guard (create `putio-sync-check-<hex>`, removed on drop) with a unit test that it is removed
- [X] T006 [US1] `path_mapping::search`: seeds from root folders and `/`, boosted BFS, skip list, limits (R3); tests: Docker layout found in few listings, not found reports `looked`, limits respected, Windows paths keep `\`
- [X] T007 [US1] `path_mapping::analyse`: path check, marker verification (R2), search, proposal (R4); tests for none / create / not_found
- [X] T008 [US1] `path_mapping::apply`: create/update/remove, logs and metric; test for create
- [X] T009 [US1] Routes `POST /arr/setup/check` and `POST /arr/setup/mapping` (client host from `host` or the app's putio-sync client, `no_client`, `marker_failed`, `changed`; saved application keeps the new check) in `putio-sync-api/src/routes/arr.rs`
- [X] T010 [US1] Route tests in `putio-sync-api/tests/arr_settings.rs`: check proposes create; mapping creates then check passes; no client → `no_client`; no marker left behind

## Phase 4: US2 One guided setup (P1)

- [X] T011 [US2] `checkArrSetup`, `applyArrMapping`, `applyArrRecommendation` in `putio-sync-api/ui/api.js`
- [X] T012 [US2] Guided steps renderer in `putio-sync-api/ui/settings/arr.js`: download client, category, folder (proposal + confirm + recheck), recommendations; stops after a failed client step
- [X] T013 [US2] Rows show **Set up** whenever they have a key; "Set up automatically" continues into the steps after a successful run

## Phase 5: US3 Safe to run again (P2)

- [X] T014 [US3] Proposal tests: wrong existing mapping → `update` by id; same path hidden by a mapping → `remove`; already visible → `none` and no write
- [X] T015 [US3] Route test: mapping apply with a stale proposal returns `changed` and writes nothing; applying twice writes once

## Phase 6: US4 Recommended settings (P3)

- [X] T016 [US4] `recommend.rs`: read both settings (missing → skipped), turn one on with a whole-body PUT (`srt` appended); mock tests
- [X] T017 [US4] Route `POST /arr/setup/recommendation` and recommendations in `/arr/setup/check`; route test

## Phase 7: Polish

- [X] T018 Secrets sweep covers the three new routes in `putio-sync-api/tests/secrets_sweep.rs`
- [X] T019 README: Path mapping section and route table; CHANGELOG Unreleased entry; ROADMAP has no 049 entry (checked)
- [X] T020 UI bundle stays under the cap (`tests/ui_assets.rs`); `node --check` on changed UI files
- [X] T021 `make ci` (or fmt, clippy, tests, release build) passes

## Dependencies

T001–T004 → US1 (T005–T010) → US3 tests build on US1. US4 (T016–T017) needs only T002/T004.
UI (T011–T013) needs the routes (T009, T017).

## Analysis (2026-11-05)

Cross-check of spec, plan and tasks: every FR maps to a task (FR-001 T013; FR-002/003 T012;
FR-004 T007; FR-005 T006; FR-006 T005; FR-007 T007, T014; FR-008 T009, T012; FR-009 T009;
FR-010 T015; FR-011/012 T016; FR-013 T009; FR-014 T018; FR-015 T001, T008). SC-003's 30 s is
met by the 20 s search limit (R3). No conflicts with the constitution; no ambiguous terms left.
