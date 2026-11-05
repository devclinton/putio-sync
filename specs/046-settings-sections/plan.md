# Implementation Plan: Settings Split into Sections

**Branch**: `046-settings-sections` | **Date**: 2026-11-05 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/046-settings-sections/spec.md`

## Summary

The Settings page becomes a shell (`ui/pages/settings.js`) with a section list, a search box
and one panel per section; every panel is built when the page opens and only one is shown.
The cards move unchanged into per-section modules under `ui/settings/`. A pure module,
`ui/settings/sections.js`, holds the sections, their cards and help, the field-address rules
and a function that turns "Settings › Section › Card" in any text into a link; the doctor,
history, status, folder rules and What's new use it. The router gains one hook: when the
address stays on the same page and the page exports `update(params)`, the page is updated
instead of mounted again, so switching sections keeps unsaved input. The Download group is
shown as four cards that each save only their own fields on top of the latest saved values.
No server API change; doctor, CLI and docs text change to the new names
([research.md](research.md)).

## Technical Context

**Language/Version**: plain ES modules, no build step; Rust stable for the asset and text tests

**Primary Dependencies**: none new

**Storage**: the "Show advanced" choice per section in `localStorage` (this browser only)

**Testing**: `node --test` over `tests/ui/settings_sections.test.mjs` for `sections.js` (run
by `tests/ui_js.rs` in `cargo test`); `tests/ui_assets.rs` checks every import resolves and
the bundle cap; manual browser check per [quickstart.md](quickstart.md)

**Target Platform**: the embedded web UI in desktop and phone browsers

**Project Type**: workspace of crates; daemon with HTTP API and embedded web UI

**Performance Goals**: search answers each keystroke over a few hundred fields with no
visible lag (a linear scan of the page's labels)

**Constraints**: the whole UI stayed under 384 KB; 378.0 KB (378,001 bytes) was used before
this feature. The split, section shell, search and marks came to about 20 KB, so the cap is
raised to 416 KB (see Complexity Tracking); about 398 KB is used after, leaving about 27 KB,
part of which the parallel 047 log viewer needs.

**Scale/Scope**: 7 sections, 22 cards, about 110 fixed fields plus plugin and hook fields

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | UI change inside `putio-sync-api/ui`; doctor and CLI only change message text |
| II. Local-first verification | New JS tests run inside `cargo test`, so `make ci` covers them |
| III. Tests without Put.io | `sections.js` is pure and tested without a browser or network |
| IV. Download integrity | Not touched; settings save through the same routes |
| V. Observable by default | No server behaviour change |
| VI. Secrets | Unchanged: secrets stay shown as set / not set; addresses carry only section and field names |
| VII. Simplicity | No dependency, no server route; the cards are moved, not rewritten |

Gate: pass. Re-checked after design: pass.

## Project Structure

### Documentation (this feature)

```text
specs/046-settings-sections/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/settings-links.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-api/
├── src/ui.rs                      # register the new modules
├── ui/
│   ├── app.js                     # router: update(params) on the same page
│   ├── app.css                    # section layout, nav, search, badges, advanced
│   ├── pages/settings.js          # shell: nav, search, marks, advanced, jumps
│   ├── pages/{doctor,history,news,status}.js, rules.js   # links into sections
│   └── settings/
│       ├── sections.js            # pure: sections, cards, help, addresses, linkParts
│       ├── form.js                # one group card: form, preview, Defaults, save, marks
│       ├── general.js             # Appearance, Credentials, Notifications
│       ├── putio.js               # Put.io app ID, storage, cleanup, watcher
│       ├── downloads.js           # Downloads, Bandwidth (schedule), Verification
│       ├── filters.js             # File filters, folder rules cards, Release check
│       ├── integrations.js        # Plugins, Event hooks
│       ├── arr.js                 # Radarr / Sonarr
│       └── advanced.js            # Network, Tailscale, Logging, Backup and restore
└── tests/ui/settings_sections.test.mjs
putio-sync-doctor/src/checks/*.rs, putio-sync-core/src/{setup,assistant}.rs,
putio-sync-cli/src/commands/setup.rs        # message text
README.md, PLUGINS.md, EVENTS.md, config.example.toml, example-config.toml, CHANGELOG.md
```

**Structure Decision**: section modules live in `ui/settings/` (not `ui/pages/`, since they
are not routes); the network and access cards share `advanced.js` with the Advanced cards to
keep the module count down. Each module exports `cards(ctx)`.

## Complexity Tracking

| Change | Why | Simpler option rejected because |
|--------|-----|----------------------------------|
| Bundle cap 384 KB → 416 KB in `tests/ui_assets.rs` | Moving cards into modules repeats a little import and helper code, and the shell (sections, search, marks, advanced toggle, field jumps) is about 12 KB of new code; the total came to about 398 KB | Trimming 14 KB elsewhere would mean cutting unrelated features; the cap has been raised the same way before (150, 225, … 384 KB) |
