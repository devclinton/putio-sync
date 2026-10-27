# Implementation Plan: Web UI Redesign with Themes

**Branch**: `012-ui-themes` | **Date**: 2026-10-26 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/012-ui-themes/spec.md`

## Summary

Rewrite `putio-sync-api/ui/app.css` around a token layer: every colour is a CSS custom
property defined once with `light-dark()`, so the mode is just `color-scheme` on `<html>`
(`light dark` for system, or forced `light` / `dark`). Accents are `[data-accent]` rule sets
whose values come from a small generator ([themes.py](themes.py)) that tints the neutrals in
OKLab and checks WCAG AA. A tiny classic script, `theme.js`, runs in `<head>` before first
paint to apply the saved choice and exposes a small API for the nav toggle and the Settings
page. The top bar becomes a sidebar (desktop) and bottom tab bar (phone) with CSS mask icons;
pages keep their logic, with small markup changes for empty states and the Appearance section.
A Rust test parses the theme tokens out of `app.css` and fails `make ci` on any AA miss.
Decisions are in [research.md](research.md) (R1–R9).

## Technical Context

**Language/Version**: Rust 1.96 (embedding and tests); browser side ES2020 modules plus one
classic script, CSS with custom properties, `light-dark()` and `color-mix()`. No build step.

**Primary Dependencies**: none new.

**Storage**: browser `localStorage` key `putio-sync.appearance` (per browser, never sent to the
daemon).

**Testing**: `cargo test` (`ui_assets.rs`: asset reachability, size budget, new contrast test);
pages checked in headless Chrome against `examples/mock_ui.rs` at 1280 and 390 px, light and dark.

**Target Platform**: current Chrome, Safari (17.5+), Firefox (120+) on desktop and phone.

**Project Type**: embedded web UI in the `putio-sync-api` crate.

**Performance Goals**: theme applied before first paint; no layout shift on theme change.

**Constraints**: UI total under 150 KB (124 KB before; 144 KB after, so about 9 KB is left for
the features in flight; the planned 135 KB target was missed, mostly on the icons and accents); no external fonts or images; no inline scripts in `index.html`.

**Scale/Scope**: 8 pages, 7 accents × 3 modes.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status |
|-----------|--------|
| I. Crate boundaries | Pass: changes stay in `putio-sync-api/ui/` and its tests; `ui.rs` gains one asset line. |
| II. Local-first verification | Pass: the contrast check is a `cargo test`, so `make ci` enforces it. |
| III. Tests without Put.io | Pass: no Put.io calls; browser checks use `mock_ui`. |
| IV. Download integrity | Not affected. |
| V. Observable by default | Not affected. |
| VI. Secrets stay secret | Pass: the appearance choice is not secret; the token key is untouched. |
| VII. Simplicity | Pass: no dependency, no build step; the generator is a documented helper in the spec folder, not part of the build. |

Post-design re-check: unchanged, all pass.

## Project Structure

### Documentation (this feature)

```text
specs/012-ui-themes/
├── plan.md              # This file
├── research.md          # R1–R9
├── data-model.md        # Appearance choice, theme tokens
├── quickstart.md        # How to check it
├── themes.py            # Accent generator + AA check (run by hand when changing accents)
├── contracts/
│   └── ui-theme.md      # Token names, html attributes, storage key, nav icon hook
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
putio-sync-api/
├── src/ui.rs                 # + theme.js asset
├── ui/
│   ├── index.html            # theme.js in <head>, mode toggle in the nav
│   ├── theme.js              # NEW: apply saved choice before paint; get/set API
│   ├── app.css               # rewritten: tokens, accents, layout, components
│   ├── app.js                # mode toggle wiring, route transition
│   └── pages/*.js            # empty states; settings.js Appearance section
└── tests/ui_assets.rs        # + contrast test over the theme tokens
```

**Structure Decision**: everything stays in the existing UI folder; one new asset.

## Complexity Tracking

No violations.
