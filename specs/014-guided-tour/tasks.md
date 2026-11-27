# Tasks: Guided tour and in-app help

**Input**: [spec.md](spec.md), [plan.md](plan.md). Paths are relative to the repo root; `ui/`
means `putio-sync-api/ui/`.

## Phase 1: Foundational

- [x] T001 Write `ui/tour.js`: `store`, `readSeen`, `markSeen`, `resetSeen`, `due`, `move`,
  `place` (FR-002, FR-003, FR-007)
- [x] T002 Write `ui/tour_content.js`: `TOURS` for the 11 navigation pages and `HELP` for every
  card in `SECTIONS`, each with a guide slug (FR-001, FR-009, FR-010)
- [x] T003 Write `putio-sync-api/tests/ui/tour.test.mjs`: every navigation page has a tour;
  every step has a title, text and a target the UI uses; every card has help; every guide is
  in `guides.js`; seen/reset with good, throwing and corrupt storage; `move`; `place` on
  desktop and phone (SC-004)

## Phase 2: User Story 1 - Tour on first visit (P1)

- [x] T004 [US1] Write `ui/tour_ui.js`: the card, Skip/Back/Next/Done, "Don't show tours",
  Esc, target outline, positioning, focus (FR-004–FR-007)
- [x] T005 [US1] Call `startTour(name)` from `show()` in `ui/app.js`; lazy-import `tour_ui.js`
  only when a tour is due (FR-003, FR-012)
- [x] T006 [US1] Add tour styles to `ui/app.css`, with the animation under reduced-motion
  no-preference and a sheet at phone width (FR-007)

## Phase 3: User Story 2 - Settings help (P1)

- [x] T007 [US2] Write `ui/settings/help.js`: `attachHelp(root)` with the ? button, the panel
  and the guide link (FR-009)
- [x] T008 [US2] Call `attachHelp` from `ui/pages/settings.js` and stop it on unmount
- [x] T009 [US2] Add `.help-btn` and `.help-panel` styles to `ui/app.css`

## Phase 4: User Story 3 - Show the tours again (P2)

- [x] T010 [US3] Add the "Tours and help" card to `ui/settings/general.js` and to `SECTIONS`
  in `ui/settings/sections.js` (FR-008)

## Phase 5: Polish

- [x] T011 Register `tour.js`, `tour_ui.js`, `tour_content.js` and `settings/help.js` in
  `putio-sync-api/src/ui.rs`
- [x] T012 Run `make i18n` and translate the new strings into all 26 languages (FR-011)
- [x] T013 README "Tours and help" section, CHANGELOG line, ROADMAP 014 marked done
- [x] T014 Run fmt, clippy (full and lite), the workspace tests, the node tests and the i18n
  check; look at one tour and one help panel in a browser
