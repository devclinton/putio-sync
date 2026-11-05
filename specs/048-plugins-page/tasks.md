# Tasks: Plugins Settings Page

**Input**: Design documents from `specs/048-plugins-page/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/plugins-page.md

**Tests**: node tests for the new pure helpers and the section addresses (project habit for
`ui/plugins.js` and `ui/settings/sections.js`); the bundle cap test stays.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Confirm `GET /plugins` gives `description`, `version`, `state` and `permissions` and that `PUT /settings/plugins` approves hosts of an enabled entry (`check_at`) in putio-sync-plugins/src/service.rs; no API change (R1)

## Phase 2: Foundational

- [X] T002 [P] Add `split(found, stored)`, `summary(description)`, `approvalText(status, row)` and `withEnabled(stored, status, on)` to putio-sync-api/ui/plugins.js as data-model.md describes; `split` keeps name order and puts a plugin in `tabs` only when its saved entry has `enabled: true`
- [X] T003 [P] Node tests for the four helpers in putio-sync-api/tests/ui/plugins.test.mjs (no entry → available; summary cuts at the first sentence or line; approval text with fixed hosts, no hosts, and an unset URL field; withEnabled keeps other entries and masked secrets and adds a default entry)
- [X] T004 Pass `extra(data, { clean, rebase, onSaved })` from `form()` in putio-sync-api/ui/settings/form.js (R3)

## Phase 3: User Story 1 – Plugins have their own page (P1) 🎯 MVP

**Independent test**: `#/settings/plugins` opens a Plugins section; Integrations has no plugins; old links land on Plugins.

- [X] T005 [US1] Add the `plugins` section after `integrations` in putio-sync-api/ui/settings/sections.js; drop plugins from Integrations' cards and help; map `plugins` in `KEY_SECTION` to `plugins`; link the old text "Settings › Integrations › Plugins" to the new card
- [X] T006 [US1] Move `PLUGINS` and `pluginsExtra` from putio-sync-api/ui/settings/integrations.js to a new putio-sync-api/ui/settings/plugins.js exporting `cards`; update the header comments of both files
- [X] T007 [US1] In putio-sync-api/ui/pages/settings.js add `BUILD.plugins`, and make a `card-<id>` address open the section that holds that card (R5)
- [X] T008 [P] [US1] Point the plugin link in putio-sync-api/ui/pages/logs.js at `#/settings/plugins?field=card-plugins`
- [X] T009 [P] [US1] Update putio-sync-api/tests/ui/settings_sections.test.mjs for the new section, the key, and the old text

## Phase 4: User Story 2 – One tab per enabled plugin (P1)

**Independent test**: two enabled plugins → two tabs, each with that plugin's full settings; edits survive switching.

- [X] T010 [US2] In putio-sync-api/ui/settings/plugins.js draw a `role=tablist` bar and one `role=tabpanel` per plugin in `split().tabs`; the panel holds the old card body without the Enabled box; the selected tab survives redraws and reloads
- [X] T011 [US2] Mark a tab with a `badge bad` when its state is broken, needs_approval or missing_config, in putio-sync-api/ui/settings/plugins.js
- [X] T012 [US2] Keep rows (editor state) by name across Reload when the fields are unchanged, in putio-sync-api/ui/settings/plugins.js (R4)
- [X] T013 [US2] When a jump target is inside a hidden `[role=tabpanel]`, select its tab first, in putio-sync-api/ui/pages/settings.js (R5)
- [X] T014 [P] [US2] Tab bar CSS like the 046 pills (row, sideways scroll) in putio-sync-api/ui/app.css

## Phase 5: User Story 3 – Turn a plugin on from a short list (P1)

**Independent test**: a plugin that is off shows one row with Enable; Enable asks with its hosts, then the plugin gets a tab.

- [X] T015 [US3] Draw the "Available plugins" list (name, version, `summary`, Enable; a load error instead of Enable when the manifest did not load) in putio-sync-api/ui/settings/plugins.js
- [X] T016 [US3] Enable: `confirm` with `approvalText`, then PUT `/settings/plugins` with `withEnabled(..., true)`, re-read `/settings/plugins` and `/plugins`, select the new tab, keep other edits, and settle the unsaved mark with `clean`/`rebase`; show a refused save as an error, in putio-sync-api/ui/settings/plugins.js
- [X] T017 [P] [US3] List row CSS (one line, ellipsis) in putio-sync-api/ui/app.css

## Phase 6: User Story 4 – Turn a plugin off from its tab (P2)

**Independent test**: Disable moves the plugin to the list; Enable again shows its saved settings.

- [X] T018 [US4] Disable button on each tab: ask first only when that row differs from its saved entry, then save `withEnabled(..., false)` the same way as Enable, in putio-sync-api/ui/settings/plugins.js

## Phase 6b: User Story 5 – Copy a sign-in code and open its page (P2)

**Independent test**: Sign in with Plex shows the code with Copy and "Copy and open plex.tv/link"; the link stays while it waits.

- [X] T024 [P] [US5] Add `carryFlow(prev, answer, busy)` and `linkLabel(url)` to putio-sync-api/ui/plugins.js with node tests in putio-sync-api/tests/ui/plugins.test.mjs
- [X] T025 [US5] In putio-sync-api/ui/settings/plugins.js show the code with Copy and, with a link, "Copy and open <link>" (open in the click; plain link kept); without the Clipboard API select the code and say to copy by hand
- [X] T026 [P] [US5] `.code` (one click selects it) and the code row in putio-sync-api/ui/app.css
- [X] T027 [P] [US5] Plex `sign_in` and `sign_in_poll` return `open_url = https://plex.tv/link` in putio-sync-plugins/bundled/plex/main.lua (and the action help in plugin.toml); update putio-sync-plugins/tests/mediaserver.rs
- [X] T028 [P] [US5] PLUGINS.md (plex row, `code`/`open_url` notes) and CHANGELOG.md

## Phase 7: Polish

- [X] T019 [P] "Settings › Plugins" in putio-sync-doctor/src/checks/plugins.rs, PLUGINS.md, README.md, EVENTS.md, config.example.toml and example-config.toml; README's Settings section list
- [X] T020 [P] CHANGELOG.md entry; remove 048 from ROADMAP.md if listed
- [X] T021 `cargo test -p putio-sync-api` (node tests and `assets_total_under_416kb`) and the doctor tests; check the bundle grew by at most 11 KiB
- [X] T022 Run quickstart.md in the browser against a local daemon, desktop and 375 px
- [X] T023 `make ci`, merge origin/main, push, and check the CI, Snap and Docker runs

## Dependencies

- T002, T004 before T010–T018. T005–T007 before T010.
- US2 before US3 and US4 (they move plugins between tabs and the list).
- T019–T020 any time; T021–T023 last.

## Parallel examples

- T002 with T004; T003 with T008, T009 and T014.

## Implementation strategy

US1 alone is shippable (a page with the old cards). US2 and US3 make the request; US4 closes
the loop.
