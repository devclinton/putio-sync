# Tasks: Organise Movies on Put.io

## Phase 1: Core naming (US1, US2)
- [x] T001 [US1] `Movie` reader (title, year, part, extra, sample) in putio-sync-core/src/release.rs
- [x] T002 [US1] `OrganiseKind::Movie`, `movies`/`movies_folder`, validation and `any()` in putio-sync-core/src/organise.rs
- [x] T003 [US1] `movie_target` with folder fallback, parts and subtitle language in putio-sync-core/src/organise.rs
- [x] T004 [US2] Extras folders and sample skip in `movie_target`
- [x] T005 [US1] Naming contract tests (contracts/naming.md) and real Radarr/Sonarr names in putio-sync-core/src/organise.rs

## Phase 2: Shared paths (US3)
- [x] T006 [US3] `OrganiseMoveKind::Movie` in putio-sync-telemetry/src/labels.rs; map it in putio-sync-download/src/organise.rs
- [x] T007 [US3] Doctor lint for `organise.movies` in putio-sync-core/src/config_lint.rs
- [x] T008 [US3] Live fields `movies`, `movies_folder` in putio-sync-api/src/routes/system/organise.rs
- [x] T009 [US3] Download integration tests: plan/apply/undo a film, sample skipped (putio-sync-download/tests/organise.rs)
- [x] T010 [US3] API test round-trips the new fields (putio-sync-api/tests/organise.rs)

## Phase 3: UI and docs
- [x] T011 [US3] Organise panel: movies toggle and folder, kind column, wording (putio-sync-api/ui/organise.js)
- [x] T012 `make i18n`; UI size check
- [x] T013 config.example.toml, README, CHANGELOG Unreleased, ROADMAP 024 done

## Dependencies
T001 → T003 → T004 → T005; T002 before T003; T006–T010 after T002; T011 after T008.
