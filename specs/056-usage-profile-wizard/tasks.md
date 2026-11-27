# Tasks: Usage Profile in the Setup Wizard

**Input**: Design documents from `specs/056-usage-profile-wizard/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/profile-api.md

**Tests**: included (constitution III and the definition of done ask for them).

## Phase 1: Setup

- [x] T001 Add `pub mod profile;` to putio-sync-core/src/lib.rs with an empty module in putio-sync-core/src/profile.rs

## Phase 2: Foundational

- [x] T002 Define `Kind` (movies, tv, music, books, software, other; serde lowercase), per-kind preset, pre-check size (none, none, 5, 1, off, none), video flag, folder names and default folder name in putio-sync-core/src/profile.rs
- [x] T003 Define `Answers {kinds, arr, by_hand, watch, language}` and `Override {off, value}` in putio-sync-core/src/profile.rs
- [x] T004 Define `Key` (rule preset per folder, pre-check watch min/enabled per path, delete after download, cleanup enabled, subtitle languages/mode/excluded, watch enabled/paths) with `get`, `set` (`Null` = default), `Display` and `FromStr` in putio-sync-core/src/profile.rs
- [x] T005 Add `ProfileConfig {kinds, arr, by_hand, watch, language, folders, applied}` and `Config.profile: Option<ProfileConfig>` (skipped when None) in putio-sync-core/src/profile.rs and putio-sync-core/src/config.rs
- [x] T006 Unit tests: every key round-trips through `Display`/`FromStr` and `get` after `set`; an old config without `[profile]` round-trips unchanged, in putio-sync-core/src/profile.rs

## Phase 3: User Story 3 — settings that fit (P1)

**Goal**: answers + detected folders → recommendations.  **Test**: core unit tests per kind.

- [x] T007 [US3] `detect(folders) -> found` (case-insensitive names, R7; "audio" books get the audio preset) in putio-sync-core/src/profile.rs
- [x] T008 [US3] `plan(answers, found, overrides) -> Vec<Recommendation>`: rules (FR-010), pre-check watch (FR-012), delete + cleanup (FR-013), subtitles (FR-014), watch (FR-015), *arr category folders; notes dlna/tailscale in putio-sync-core/src/profile.rs
- [x] T009 [US3] `apply(config, recs)` and `diff(config, profile_cfg, recs) -> {changes, kept}` keeping hand-made changes (R3), with `restart` from `restart::needs_restart`, in putio-sync-core/src/profile.rs
- [x] T010 [US3] Unit tests: movies+tv+radarr+sonarr, music+books on a watch folder (1 MB), software turns pre-check off and lifts blocked extensions, subtitles languages de+en, overrides switch off/change values, kept values on re-run, in putio-sync-core/src/profile.rs

## Phase 4: User Story 1 — four answers (P1) 🎯 MVP

**Goal**: link → language → Put.io → usage → review → Start syncing.  **Test**: API tests + manual quickstart 4.

- [x] T011 [US1] Serve `GET /setup` as the UI index and keep a link base in SetupState; banner prints `…/setup#code=…`, also when the code is replaced, in putio-sync-api/src/setup.rs and putio-sync-cli/src/commands/setup_mode.rs
- [x] T012 [US1] `GET /setup/profile/detect` and `POST /setup/profile` (store draft in session, return review with 015 risks) in putio-sync-api/src/routes/setup.rs
- [x] T013 [US1] `/setup/finish` applies the session profile and writes `[profile]`, in putio-sync-api/src/routes/setup.rs
- [x] T014 [US1] API tests (mocked Put.io): detect, review, finish writes rules and `[profile]`, no-kinds 400, in putio-sync-api/tests/setup_profile.rs
- [x] T015 [P] [US1] Pure helpers (code from fragment, reason text per code, setting labels) in putio-sync-api/ui/profile.js with tests in putio-sync-api/tests/ui/profile.test.mjs
- [x] T016 [US1] Wizard: language screen and switcher, fragment unlock with `history.replaceState`, usage screen, folder step only when the check fails, review with switches/values and dry-run, Start syncing, in putio-sync-api/ui/pages/setup.js and putio-sync-api/ui/profile_view.js
- [x] T017 [US1] Register new UI assets in putio-sync-api/src/ui.rs

## Phase 5: User Story 2 — Radarr and Sonarr (P1)

- [x] T018 [US2] Usage screen runs `/setup/arr/discover`; review checks each ticked app with `/setup/arr/preview`; Start syncing runs `/setup/arr/run` (remember) before finish, in putio-sync-api/ui/pages/setup.js
- [ ] T019 [US2] (Deferred: the Radarr / Sonarr settings card already runs these checks after start) After start, for each saved application call `/arr/setup/check` and offer the 049 mapping and recommendation buttons, in putio-sync-api/ui/pages/setup.js

## Phase 6: User Story 4 — set it up myself (P2)

- [x] T020 [US4] "I'll set it up myself" clears the profile (`answers: null`) and continues with today's steps unchanged, in putio-sync-api/ui/pages/setup.js; API test that finish without a profile writes no `[profile]` in putio-sync-api/tests/setup_profile.rs

## Phase 7: User Story 5 — run again from Settings (P3)

- [x] T021 [US5] `GET /settings/profile`, `POST /settings/profile/preview`, `PUT /settings/profile` (through settings `update`) in putio-sync-api/src/routes/system/profile.rs and the router
- [x] T022 [US5] API test: hand-changed value listed as kept and not overwritten, in putio-sync-api/tests/setup_profile.rs
- [x] T023 [US5] Settings › Usage profile page in putio-sync-api/ui/settings/profile.js, linked from the settings menu, registered in putio-sync-api/src/ui.rs

## Phase 8: Polish

- [x] T024 Document `[profile]` in config.example.toml
- [x] T025 `make i18n`, fill all 26 locales in putio-sync-api/ui/i18n/
- [x] T026 CHANGELOG.md line under [Unreleased]
- [x] T027 Run clippy, tests, `scripts/ci-local.sh` lite build, node UI tests, `node scripts/i18n-extract.mjs --check`

## Dependencies

Phase 2 → US3 → US1 → US2, US4, US5 (US4/US5 independent of US2). T015 can run alongside T011–T014.

## Implementation Strategy

MVP = US3 + US1 (profile path without *arr). Then US2, US4, US5, polish, one PR.
