# Implementation Plan: Multi-language Web UI

**Branch**: `051-ui-i18n` | **Date**: 2026-11-08 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/051-ui-i18n/spec.md`

## Summary

Keep the UI's English text where it is and use it as the catalog key. A small static module picks the
language (saved choice, else the browser's, else English) and, for any language but English, loads
that catalog and a translator that translates text nodes and labels as they reach the page. A
Language card in Settings › General switches at once. Two node scripts keep `en.json` in step with
the code and fill the other catalogs from the translation service. Snap Store listings can't be
translated (research R7). Details: [research.md](research.md).

## Technical Context

**Language/Version**: Rust stable (asset embedding, tests); browser ES modules; Node 22 for scripts and UI tests
**Primary Dependencies**: none new. Translation runs offline of CI through `atranslate-cli`.
**Storage**: catalogs in `putio-sync-api/ui/i18n/`, embedded in the binary; the choice in `localStorage`
**Testing**: `tests/ui_i18n.rs` (served, valid, keys a subset of en.json, coverage ≥ 90%, file list
matches `LANGUAGES`); `tests/ui/i18n.test.mjs` (matching, translation, scanner, en.json current)
**Constraints**: the English page stays under the `ui_assets` cap; no host names or addresses of the
translation service in the repo
**Scale/Scope**: about 1,330 strings × 26 languages, about 80 KB per catalog

## Constitution Check

| Principle | Status | How |
|---|---|---|
| I. Crate boundaries | Pass | Only `putio-sync-api` (UI assets and their tests). |
| II. Local-first | Pass | `make ci` runs both tests; translation is a maintainer step (`make i18n-translate`), not CI. |
| III. Tests without Put.io | Pass | No network in tests. |
| IV. Download integrity | Pass | No change. |
| V. Observable | Pass | No change. |
| VI. Secrets | Pass | Nothing new reaches the daemon; the service address comes from the environment. |
| VII. Simplicity | Pass with note | No dependency, no build step. The size cap moves by 4 KiB (R4). |

## Project Structure

```text
putio-sync-api/
├── src/ui.rs                    # + i18n.js, translate.js, catalog!(code) × 26
├── ui/i18n.js                   # language choice, browser match, names, setLanguage (static)
├── ui/translate.js              # compile, tr, DOM walk + observer (dynamic import)
├── ui/i18n/en.json              # source catalog, generated
├── ui/i18n/<code>.json          # 26 translated catalogs
├── ui/app.js                    # boot: setLanguage before the first route; translated title
├── ui/settings/general.js       # Language card
├── ui/settings/sections.js      # card listed in General
├── tests/ui_i18n.rs
├── tests/ui/i18n.test.mjs
└── tests/ui_assets.rs           # cap 480 → 484 KiB
scripts/i18n-extract.mjs, scripts/i18n-translate.mjs
Makefile                         # make i18n, make i18n-translate
```

## Deviations

- Size cap raised to 484 KiB (R4).
- No plan for a translated Snap Store listing (R7).
