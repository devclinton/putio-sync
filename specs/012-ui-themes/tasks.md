# Tasks: Web UI Redesign with Themes

**Input**: Design documents from `specs/012-ui-themes/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/ui-theme.md,
quickstart.md

**Tests**: the spec asks for an automated contrast check (SC-002) and the size budget
(SC-005); both are `cargo test`s. Page checks are by browser against `mock_ui`.

All paths are under `putio-sync-api/` unless they start with `specs/`.

## Phase 1: Setup

- [X] T001 Record before screenshots of all 8 pages (1280 / 390 px, light / dark) from `examples/mock_ui.rs` into the session scratchpad, and the current UI size (124,013 bytes)

## Phase 2: Foundational (blocks every story)

- [X] T002 Write the failing test `theme_contrast_meets_aa` in `tests/ui_assets.rs`: fetch `/assets/app.css`, collect `--name: light-dark(#rrggbb, #rrggbb)` declarations per rule (`:root`, each `[data-accent="…"]`), resolve each accent over `:root`, and assert ≥ 4.5:1 for `text`, `muted`, `accent`, `ok`, `warn`, `bad` on `bg`, `surface`, `surface-2`, and `on-accent` on `accent`, in light and dark; also assert at least 6 accents exist (contract: blue default, plus `indigo`, `teal`, `green`, `orange`, `pink`, `graphite`)
- [X] T003 Add `ui/theme.js` (classic script): read `localStorage["putio-sync.appearance"]` as `{mode, accent}`; "Unknown value → `system`" for mode and "Unknown value → `blue`" for accent; set or remove `data-mode` / `data-accent` on `<html>`; expose `window.appearance = { get, set, ACCENTS }`; `set` saves (try/catch) and dispatches `appearancechange` on `document`
- [X] T004 Register `theme.js` in `src/ui.rs` ASSETS and load it with `<script src="/assets/theme.js"></script>` in `<head>` of `ui/index.html`, before the stylesheet
- [X] T005 Rewrite the token layer at the top of `ui/app.css`: `:root` fallbacks, `color-scheme` handling for `[data-mode]`, the seven accent rules pasted from `specs/012-ui-themes/themes.py`, status colours, derived tokens (`--accent-soft`, charts, heat steps via `color-mix`), shape, depth, space and type tokens, and the legacy aliases listed in `contracts/ui-theme.md`

**Checkpoint**: T002 passes; the old pages render with new colours.

## Phase 3: User Story 1 - A calmer, more polished UI on every page (P1) 🎯 MVP

**Goal**: new visual language and navigation on every page.

**Independent Test**: quickstart step 1 on all 8 pages.

- [X] T006 [US1] Navigation in `ui/app.css`: fixed sidebar above 600 px (brand mark, icon + label links, current page in `--accent-soft` / `--accent`, foot with mode toggle, Metrics and Sign out); at ≤ 600 px a top row (brand, toggle, sign out) and a fixed bottom tab bar with 44 px+ items, safe-area padding and body padding so content clears it; CSS-mask icons per `[data-page]` with a generic default
- [X] T007 [US1] Base and components in `ui/app.css`: type scale, page head, cards (sentence-case titles), grid/stack, kv, counts (no overlapping labels), big numbers, dots with halo, pill badges, buttons, inputs, fields, checkboxes, progress bars, tables + phone cards, selection bar, banner, toasts (above the tab bar on phones), dialog, sign-in card, Stats page pieces
- [X] T008 [US1] Add `empty(title, hint)` to `ui/app.js` returning `div.empty`, and use it for the empty lists in `ui/pages/queue.js` (queue, active downloads), `ui/pages/status.js` (active downloads), `ui/pages/putio.js` (empty folder / no match), `ui/pages/transfers.js` (no transfers), `ui/pages/history.js` (no history / no match), `ui/pages/stats.js` (nothing in range)
- [X] T009 [US1] Motion in `ui/app.css` and `ui/app.js`: route-change entrance on `#view` (class toggled in `mount`), toast and dialog entrances, colour transitions; all inside `prefers-reduced-motion: no-preference`
- [X] T010 [US1] Browser pass over all pages at both widths and modes; fix layout faults found

## Phase 4: User Story 2 - Pick light, dark or system, and an accent (P1)

**Goal**: mode toggle in the nav, Appearance section in Settings, remembered per browser.

**Independent Test**: quickstart steps 2–3.

- [X] T011 [US2] Add the mode toggle button to `ui/index.html` (`#mode`, in `.topbar-end`) and wire it in `ui/app.js`: steps `system → light → dark → system` via `window.appearance.set`, label and `aria-label` "Appearance: System/Light/Dark", icon per mode, updates on `appearancechange`
- [X] T012 [US2] Appearance card at the top of `ui/pages/settings.js`: a radio group for mode (System / Light / Dark) and a radio group of accent swatches (visible name per swatch, current one checked), applying on change, with the note "Saved in this browser only."; swatch styles in `ui/app.css`
- [X] T013 [US2] Check that a saved choice applies on sign-in and on reload with no flash, that an unknown saved accent falls back to blue, and that blocked storage still applies the choice for the page view

## Phase 5: User Story 3 - Accessible in every theme (P2)

**Goal**: AA contrast, visible focus, keyboard order.

**Independent Test**: `theme_contrast_meets_aa` plus a keyboard pass.

- [X] T014 [US3] One `:focus-visible` ring rule for every interactive element in `ui/app.css` (accent outline, 2 px offset), including nav links, swatches, table checkboxes and chart rows
- [X] T015 [US3] Keyboard pass in the browser through nav, settings Appearance and a table page; confirm the tab order is nav then content and the radio groups use arrow keys

## Phase 6: Polish

- [X] T016 [P] Note the token contract for other UI branches in the shared session notes and in `putio-sync-api/ui/app.css` header comment
- [X] T017 Run `TMPDIR=<scratch> make ci`; confirm `assets_total_under_150kb` and record the new UI size
- [X] T018 After screenshots of all 8 pages (1280 / 390 px, light / dark, plus one accent) and a before/after sheet sent to the user

## Dependencies

- T002–T005 block everything. T005 needs the generator output (done in planning).
- US1 (T006–T010) and US2 (T011–T013) both touch `app.css` and `app.js`; do them in order.
- US3 relies on US1's components.
- T017–T018 last.

## Parallel opportunities

Few: almost every task edits `app.css`. T003/T004 (theme.js, ui.rs) can run alongside T002;
T016 alongside anything.

## Implementation strategy

MVP is Phase 2 + US1: new look on every page following the system mode. US2 adds the controls,
US3 hardens accessibility. Each phase ends with a browser check against `mock_ui`.
