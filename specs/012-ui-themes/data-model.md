# Data Model: Web UI Redesign with Themes

No server-side data changes.

## Appearance choice (browser only)

Stored as JSON in `localStorage["putio-sync.appearance"]`.

| Field | Values | Default | Rule |
|-------|--------|---------|------|
| `mode` | `system`, `light`, `dark` | `system` | Unknown value → `system` |
| `accent` | `blue`, `indigo`, `teal`, `green`, `orange`, `pink`, `graphite` | `blue` | Unknown value → `blue` |

Transitions: the nav toggle steps `system → light → dark → system`. The Settings page sets
either field directly. A missing or unreadable key means the defaults; a failed write keeps the
choice for the current page view only.

## Theme tokens

One set per accent, each token holding a light and a dark value. See
[contracts/ui-theme.md](contracts/ui-theme.md) for names.
