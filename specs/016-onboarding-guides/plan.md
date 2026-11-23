# Implementation Plan: Onboarding guides

**Branch**: `016-onboarding-guides` | **Date**: 2026-11-18 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/016-onboarding-guides/spec.md`; agreed calls in
[design.md](design.md)

## Summary

The guides are new mdBook chapters under `site/book/src/guides/`, as a "Guides" part after
the Introduction:
- an index;
- seven setup guides;
- six task guides;
- four switching guides.

They are walkthroughs that link to the existing reference chapters. Each is machine-translated
by the 054 tooling and passes the site's lychee and axe checks.

The web UI gets one small module, `ui/guides.js`, holding the guide list and the map from
install channel to setup guide. The setup wizard, five settings sections, the Doctor and
Logs pages, and a new "Guides" group in the command palette all link from it. The wizard
reads the channel from the existing `GET /update`.

TrueNAS gets an install marker (`PUTIO_SYNC_INSTALL=truenas`) and a `Truenas` channel, so its
users get the TrueNAS guide. A node test keeps the UI's guide slugs and the book in step.

The work lands over four PRs: setup guides, task guides, switching guides, and the UI
links. Details: [research.md](research.md).

## Technical Context

**Language/Version**: Markdown (mdBook 0.5.4); plain ES modules for the UI; Rust stable for
the channel change

**Primary Dependencies**: none new

**Storage**: N/A

**Testing**:
- `node --test` through `tests/ui_js.rs`: the new `guides.test.mjs`, and the palette group
  added to the existing tests.
- `cargo test` in `putio-sync-update` for the `truenas` channel.
- `site/tools/build.sh`, plus the lychee and a11y checks, for the site.
- Hand walk-throughs per [quickstart.md](quickstart.md).

**Target Platform**: the published docs site, and the web UI in the browser

**Project Type**: documentation plus a small web-UI change

**Performance Goals**: no change to UI size worth noting. `guides.js` is a few KB, inside
the 512 KiB UI cap.

**Constraints**: guides must match the current release's UI wording and steps. No guide
repeats reference material. Links to the site must exist when the UI-links PR ships.

**Scale/Scope**: about 18 pages, 1 UI module, edits to about 8 UI files, and 1 enum variant

## Constitution Check

| Principle | Status |
|-----------|--------|
| I. Crate boundaries | Pass. Channel detection stays in `putio-sync-update`. The UI only reads `/update`. |
| II. Local-first verification | Pass. The new node test runs in `make ci`. `site/tools/build.sh` runs locally. |
| III. Tests without put.io | Pass. The tests read files only. The walk-throughs use a real account, but they are manual checks, not automated tests. |
| IV. Download integrity | Not affected. |
| V. Observable by default | Not affected. The troubleshooting guide explains the existing logs and metrics. |
| VI. Secrets stay secret | Pass. Guides use placeholders for tokens and passwords. Screenshots, if any, show none. |
| VII. Simplicity | Pass. No new endpoint, dependency or setting. A static guide list rather than generated. |

Post-design re-check: unchanged.

## Project Structure

### Documentation (this feature)

```text
specs/016-onboarding-guides/
├── design.md
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/ui-links.md
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
site/book/src/
├── SUMMARY.md                      # new "Guides" part after the Introduction
└── guides/
    ├── index.md
    ├── unraid.md  truenas.md  raspberry-pi.md  docker.md  macos.md  windows.md  linux.md
    ├── radarr-sonarr.md  folder-rules.md  watch-folders.md  media-server.md
    ├── metrics-grafana.md  troubleshooting.md
    └── from-qbittorrent.md  from-transmission.md  from-deluge.md  from-usenet.md
site/i18n/*.json                    # refreshed per PR
packaging/truenas/putio-sync/templates/docker-compose.yaml   # PUTIO_SYNC_INSTALL=truenas
putio-sync-update/src/channel.rs    # Channel::Truenas
putio-sync-api/
├── src/ui.rs                       # ASSETS gains guides.js
├── ui/guides.js                    # new: guide list + channel map
├── ui/palette.js                   # Guides group
├── ui/pages/setup.js               # wizard link
├── ui/pages/doctor.js, logs.js     # troubleshooting link
├── ui/settings/arr.js, filters.js, downloads.js (watch), media.js, advanced.js (metrics)
├── ui/i18n/*.json                  # new strings
└── tests/ui/guides.test.mjs        # new
```

Which settings module holds the watch folder and metrics fields is checked during
implementation. The list above is the expected place.

## Delivery

1. **PR A: setup guides (US1)**: index and the seven setup guides, the SUMMARY part, and
   translations. Claude walks through macOS, Unraid and Docker. The maintainer walks through
   Windows, Linux and TrueNAS. The Pi page carries the untested note.
2. **PR B: task guides (US2)**: six guides and translations. Walked through on Mnemosyne
   (Radarr/Sonarr, the media server, metrics) and on the Mac.
3. **PR C: switching guides (US3)**: four guides and translations.
4. **PR D: UI links and TrueNAS channel (US4)**: `guides.js`, the links, the palette group,
   the channel, the test, and UI translations. It opens once A to C are merged and the site
   is published.

Each PR adds a CHANGELOG `[Unreleased]` line. PR D also updates the README web-UI section and
ROADMAP.md.

## Complexity Tracking

None.
