# Contract: UI links to the guides (016)

All links open `https://devclinton.github.io/putio-sync/docs/<slug>.html` in a new tab via
the existing `docs(page, id, text)` helper.

## Install channel to setup guide

| `channel.kind` | Guide |
|----------------|-------|
| `unraid` | `guides/unraid` |
| `truenas` (new) | `guides/truenas` |
| `container`, `casaos`, `umbrel` | `guides/docker` |
| `snap`, `deb`, `flatpak` | `guides/linux` |
| `homebrew`, `pkg` | `guides/macos` |
| `winget`, `setup` | `guides/windows` |
| `archive`, `unknown`, missing, or `/update` failed | `guides/index` |

`archive` (a release tarball or zip) runs on any OS, and `/update` doesn't say which one. The
browser may be on another machine, so it can't tell either. The index lists every platform,
the Raspberry Pi included.

## Where links appear

| Place | Link text | Target |
|-------|-----------|--------|
| Setup wizard, first screen | "Setup guide" | from the table above |
| Settings › Radarr and Sonarr | "Guide" | `guides/radarr-sonarr` |
| Settings › folder rules | "Guide" | `guides/folder-rules` |
| Settings › watch folders | "Guide" | `guides/watch-folders` |
| Settings › media server | "Guide" | `guides/media-server` |
| Settings › metrics | "Guide" | `guides/metrics-grafana` |
| Doctor page | "Troubleshooting guide" | `guides/troubleshooting` |
| Logs page | "Troubleshooting guide" | `guides/troubleshooting` |
| Command palette | a "Guides" group, after Settings | every guide in `ui/guides.js` |

## Palette group

- Shown only when the query is not empty, ranked like the other groups, limit 8.
- Matched on: the title in the UI language, the English title, and `words`.
- The detail column shows the kind label. Choosing an entry opens the guide in a new tab and
  closes the palette.

## Channel API

`GET /update` already returns `channel: { kind, label, mode }`. The only change is the new
kind `truenas` (label "TrueNAS", mode `instructions`).
