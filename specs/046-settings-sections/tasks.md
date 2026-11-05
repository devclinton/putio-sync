# Tasks: Settings Split into Sections

**Input**: Design documents from `/specs/046-settings-sections/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/settings-links.md

**Tests**: node tests for the pure `sections.js` (addresses, key → section, text links); the
existing asset tests check imports and the bundle cap. The page itself is checked in a browser
per quickstart.md.

## Phase 1: Setup

- [X] T001 Record the bundle size before the change (378,001 bytes) in specs/046-settings-sections/plan.md and confirm `node` runs `putio-sync-api/tests/ui/*.test.mjs`

## Phase 2: Foundational

- [X] T002 Write `putio-sync-api/tests/ui/settings_sections.test.mjs` for `settingsHref`, `fieldId`, `sectionOfKey` and `linkParts` per contracts/settings-links.md (red first)
- [X] T003 Create `putio-sync-api/ui/settings/sections.js`: SECTIONS (id, title, help, cards per data-model.md), `settingsHref`, `fieldId` (`watch.` → `watcher`), `sectionOfKey`, `linkParts`; T002 passes
- [X] T004 Create `putio-sync-api/ui/settings/form.js` from the group/form/preview/risk helpers of `ui/pages/settings.js`: `groupCard(def, ctx)` with `part` cards saving on the latest saved group values, Defaults limited to the card's own fields, `advanced` fields marked `adv`, card id `card-<id>`, and dirty/problem reports through `ctx.report`
- [X] T005 In `putio-sync-api/ui/app.js` route(): when the new address is on the current page and the page exports `update`, call `update({path, query})` instead of mounting again
- [X] T006 Register every new module in `putio-sync-api/src/ui.rs`

## Phase 3: User Story 1 - Find settings by topic (P1) 🎯 MVP

**Goal**: seven sections, one shown at a time, each with help; nothing lost when switching.
**Independent test**: quickstart steps 3 and 7.

- [X] T007 [P] [US1] `putio-sync-api/ui/settings/general.js`: Appearance, Credentials, Notifications
- [X] T008 [P] [US1] `putio-sync-api/ui/settings/putio.js`: Put.io app ID, Put.io storage, Put.io cleanup, Torrent watcher
- [X] T009 [P] [US1] `putio-sync-api/ui/settings/downloads.js`: Downloads, Bandwidth (with the schedule editor), Verification as `part` cards of the download group
- [X] T010 [P] [US1] `putio-sync-api/ui/settings/filters.js`: File filters (`part` card with the pattern preview), the folder rules cards from `ui/rules.js`, Release check with its override tables and effective table
- [X] T011 [P] [US1] `putio-sync-api/ui/settings/integrations.js` (Plugins, Event hooks) and `putio-sync-api/ui/settings/arr.js` (Radarr / Sonarr)
- [X] T012 [P] [US1] `putio-sync-api/ui/settings/advanced.js`: Network, Tailscale, Logging, Backup and restore
- [X] T013 [US1] Rewrite `putio-sync-api/ui/pages/settings.js` as the shell: warnings banner, section list, one panel per section with its help, `update()` switching panels, `#/settings` → General
- [X] T014 [US1] Section layout in `putio-sync-api/ui/app.css`: side list from 900 px, pill row that scrolls within itself below, no page overflow at 375 px

## Phase 4: User Story 2 - Link to a section and setting (P1)

**Independent test**: quickstart steps 4 and 6.

- [X] T015 [US2] Field jump in `putio-sync-api/ui/pages/settings.js`: wait up to 5 s for the element, turn on Show advanced when needed, scroll, focus, highlight 2 s; warnings banner links each risk via `sectionOfKey`
- [X] T016 [P] [US2] Doctor suggestions and the history verify note through `linkParts` in `putio-sync-api/ui/pages/doctor.js` and `putio-sync-api/ui/pages/history.js`
- [X] T017 [P] [US2] Section links in `putio-sync-api/ui/pages/status.js` (arr client off → `arr-enabled`) and `putio-sync-api/ui/rules.js` (root rule → File filters); `#/` links in What's new notes open in the same tab in `putio-sync-api/ui/pages/news.js`
- [X] T018 [P] [US2] Rename settings in text: `putio-sync-doctor/src/checks/{plugins,verify,hooks}.rs`, `putio-sync-core/src/setup.rs`, `putio-sync-core/src/assistant.rs`, `putio-sync-cli/src/commands/setup.rs`, and any tests that match the text
- [X] T019 [P] [US2] Rename settings in docs: README.md, PLUGINS.md, EVENTS.md, config.example.toml, example-config.toml

## Phase 5: User Story 3 - Search (P2)

**Independent test**: quickstart step 5.

- [X] T020 [US3] Search box in `putio-sync-api/ui/pages/settings.js`: scan `label.field` and card titles across panels, rank per research R7, 10 results, Enter picks the first, Escape clears, a pick jumps as in T015; styles in `putio-sync-api/ui/app.css`

## Phase 6: User Story 4 - Unsaved and problem marks (P2)

**Independent test**: spec US4 acceptance 1-3.

- [X] T021 [US4] Section marks in `putio-sync-api/ui/pages/settings.js` from `ctx.report` and the assistant risks; "Unsaved changes" note on the card (in `form.js`); `beforeunload` while anything is dirty

## Phase 7: User Story 5 - Advanced toggle (P3)

**Independent test**: spec US5 acceptance 1-3.

- [X] T022 [US5] "Show advanced (N)" per section in `putio-sync-api/ui/pages/settings.js`, stored in `localStorage` key `putio-sync.settings.advanced`; CSS hides `.adv` in panels without `show-adv`

## Phase 8: Polish

- [X] T023 CHANGELOG.md entry under Unreleased (Changed)
- [X] T024 `cargo test -p putio-sync-api --test ui_js --test ui_assets`; record bundle size and headroom in plan.md
- [X] T025 Browser check against a local daemon at desktop and 375 px per quickstart.md
- [X] T026 Merge `origin/main`, place any new Settings field (047 log level) in its section, `TMPDIR=<scratch> make ci`, push, hand-over note in the shared notes folder

## Dependencies

- T002 → T003; T003, T004 → T007-T013; T005 needed by T013.
- US2-US5 build on the shell (T013). T016-T019 are independent of each other.

## Parallel examples

- After T004: T007-T012 touch separate new files.
- US2: T016, T017, T018, T019 touch separate files.

## Implementation strategy

MVP is US1 (T001-T014). US2 makes links useful; US3-US5 are small additions to the shell.
