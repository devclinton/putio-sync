# Implementation Plan: Guided tour and in-app help

**Branch**: `feat/014-guided-tour` | **Date**: 2026-11-27 | **Spec**: [spec.md](spec.md)

## Summary

UI only; no Rust behaviour changes beyond registering the new assets.

- **`ui/tour_content.js`** (data): `TOURS` (page → steps `{ target, title, text }`) and `HELP`
  (settings card id → `{ text, guide }`). Plain string literals, so the i18n extractor (051)
  picks them up and the translator translates them as they reach the page.
- **`ui/tour.js`** (pure logic, node-tested): the seen set in `localStorage` with an in-memory
  fallback (`readSeen`, `markSeen`, `resetSeen`), `due(page, seen, { setup })`,
  `move(index, count, by)` and `place(target, card, view)` for the card's position.
- **`ui/tour_ui.js`** (DOM, lazy): `start(page, { force })` and `stop()`. A fixed, non-modal
  `role="dialog"` card with Skip, Back, Next/Done and "Don't show tours"; outlines the step's
  target with `.tour-target`; repositions on resize and scroll; Esc closes it.
- **`ui/app.js`**: `show()` calls `startTour(name)`, which stops any tour and, when one is due,
  imports `tour_ui.js` and starts it. `tour.js` is the only module loaded up front.
- **`ui/settings/help.js`** (DOM): `attachHelp(root)` adds a ? button to each card heading and
  keeps doing so with a `MutationObserver`, because cards rebuild themselves when they load,
  save or retry. The panel links to the guide through `docs()` and `GUIDES`.
- **`ui/settings/general.js`, `sections.js`**: a "Tours and help" card in General with
  **Show the tours again**; it is in `SECTIONS`, so the palette and Settings search find it.
- **`ui/app.css`**: `.tour`, `.tour.sheet` (phone), `.tour-target`, `.help-btn`, `.help-panel`;
  the entrance animation is inside the existing `prefers-reduced-motion: no-preference` block.

## Technical Context

**Language/Version**: plain ES modules for the UI; Rust stable only for the asset table
**Primary Dependencies**: none new
**Storage**: `localStorage["putio-sync.tours"]` = `{ "<page>": 1 }`, per browser
**Testing**: `node --test putio-sync-api/tests/ui/tour.test.mjs` (also run by `ui_js.rs`);
`ui_assets.rs` checks every import resolves to a registered asset
**Constraints**: no dependency; nothing blocks the page; all text translatable; no server state

## Constitution Check

| Principle | Check |
|---|---|
| I Crate boundaries | Only `putio-sync-api`'s embedded UI changes. ✅ |
| II Local-first | Everything runs under `make ci`. ✅ |
| III Tests without Put.io | Node tests over pure modules and data. ✅ |
| IV Download integrity | Nothing is downloaded or deleted. ✅ |
| V Observable | No daemon work, so no logs or metrics. ✅ |
| VI Secrets | The seen set holds page names only. ✅ |
| VII Simplicity | No dependency, no config key, no API route. ✅ |

## Decisions

- **Seen on show, not on finish.** Otherwise a user who clicks away mid-tour gets it again on
  every visit.
- **No backdrop and no focus trap.** The brief says the tour never blocks the page. The card
  takes focus once, so screen readers announce it and Tab reaches its buttons, unless focus is
  already in a field.
- **A missing target is not an error.** Lists and toolbars depend on data; the step shows
  without an outline. A test checks each selector's class or id exists in the UI source.
- **One ? per card, not per field.** Fields already have hints and "Affects" lines; 200 more
  buttons would be noise. Recorded under Assumptions in the spec.
- **Help buttons by observer.** Ten modules build cards in their own ways and rebuild them on
  load; one observer in `pages/settings.js` covers them all without touching each module.

## Project Structure

```text
putio-sync-api/ui/tour.js, tour_ui.js, tour_content.js   # new
putio-sync-api/ui/settings/help.js                        # new
putio-sync-api/ui/app.js                                  # startTour() from show()
putio-sync-api/ui/pages/settings.js                       # attachHelp()
putio-sync-api/ui/settings/general.js, sections.js        # "Tours and help" card
putio-sync-api/ui/app.css                                 # tour and help styles
putio-sync-api/ui/i18n/*.json                             # new strings, 26 languages
putio-sync-api/src/ui.rs                                  # register the four new assets
putio-sync-api/tests/ui/tour.test.mjs                     # new
README.md, CHANGELOG.md, ROADMAP.md                       # docs
```
