# Tasks: Multi-language Web UI (051)

**Input**: `specs/051-ui-i18n/` (spec, plan, research, quickstart).
**Tests**: included; no network in tests.
**Format**: `- [ ] T### [P] [US#] description with file path`.

## Phase 1: Setup

- [X] T001 [P] Write the extractor `scripts/i18n-extract.mjs` (scanner, text rules, `--check`) and generate `putio-sync-api/ui/i18n/en.json`
- [X] T002 [P] Write `scripts/i18n-translate.mjs` (only missing strings, drops stale ones) and `make i18n` / `make i18n-translate` in `Makefile`

## Phase 2: Foundational

- [X] T003 Write failing node tests in `putio-sync-api/tests/ui/i18n.test.mjs`: browser tag matching, exact and pattern translation, values reordered and translated, dropped placeholders fall back, scanner skips comments and regexes, en.json current
- [X] T004 `putio-sync-api/ui/i18n.js`: `LANGUAGES`, `match`, `choice`, `nativeName`, `tr`, `setLanguage` (fetch catalog, dynamic import of the translator, `lang`/`dir`, `languagechange`)
- [X] T005 `putio-sync-api/ui/translate.js`: `compile`, `tr`, DOM walk with skipped data, `MutationObserver`, `apply`
- [X] T006 Serve `i18n.js`, `translate.js` and `catalog!(code)` for each language in `putio-sync-api/src/ui.rs`

## Phase 3: User Story 1 + 2 - Browser language and the picker (P1)

- [X] T007 [US1] Boot with `setLanguage(choice(), false)` before the first route and translate the page title in `putio-sync-api/ui/app.js`
- [X] T008 [US2] Language card in `putio-sync-api/ui/settings/general.js`, listed in `putio-sync-api/ui/settings/sections.js`
- [X] T009 [US1] Rust tests `putio-sync-api/tests/ui_i18n.rs`: each language served as JSON, keys a subset of en.json, ≥ 90% usable, files match `LANGUAGES`, translator served, no en.json route
- [X] T010 [US1] Size cap 480 → 484 KiB in `putio-sync-api/tests/ui_assets.rs` (research R4)
- [X] T011 [US2] Check in the mock UI: German boot, picker lists native names, live switch and back, added nodes translated, `translate="no"` kept, Arabic right to left

## Phase 4: User Story 3 - Catalogs (P2)

- [x] T012 [US3] `make i18n-translate` for all 26 languages through atranslate; commit `putio-sync-api/ui/i18n/*.json`
- [x] T013 [US3] Spot-check a few catalogs for placeholders kept and nothing untranslated

## Phase 5: User Story 4 - Snap Store listing (P3)

- [X] T014 [US4] Research: not supported (research R7); nothing wired into `snap/`

## Phase 6: Polish

- [X] T015 [P] README "Languages", CHANGELOG, ROADMAP entry
- [x] T016 `make ci` passes
