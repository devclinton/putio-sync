# Contract: UI theme hooks

For code in `putio-sync-api/ui/`, including pages added by other features.

## `<html>` attributes (set by `theme.js`)

| Attribute | Values | Absent means |
|-----------|--------|--------------|
| `data-mode` | `light`, `dark` | follow the system |
| `data-accent` | `indigo`, `teal`, `green`, `orange`, `pink`, `graphite` | blue |

## `window.appearance`

- `get()` → `{ mode, accent }`
- `set({ mode?, accent? })` → applies at once, saves to `localStorage["putio-sync.appearance"]`,
  dispatches `appearancechange` on `document`.
- `ACCENTS` → the list of accent names in display order.

## Design tokens (CSS custom properties)

Use these instead of literal colours. All colours switch with mode and accent.

| Group | Tokens |
|-------|--------|
| Surfaces | `--bg`, `--surface`, `--surface-2`, `--line` |
| Text | `--text`, `--muted` |
| Accent | `--accent`, `--on-accent`, `--accent-soft` (tinted background) |
| Status | `--ok`, `--warn`, `--bad` (text-safe on every surface) |
| Charts | `--chart-1`, `--chart-1-on`, `--heat-0` … `--heat-5` |
| Shape | `--r-sm` 8px, `--r-md` 12px, `--r-lg` 20px, `--r-pill` |
| Depth | `--shadow-1` (cards), `--shadow-2` (popovers, toasts, dialog) |
| Space | `--s-1` 4px … `--s-6` 32px, `--gap` |
| Type | `--font`, `--mono`, `--fs-sm` 13px, `--fs-md` 15px, `--fs-lg` 17px, `--fs-xl` 22px, `--fs-title` 34px |

Old names kept as aliases for in-flight branches: `--border` (= `--line`), `--accent-text`
(= `--on-accent`), `--radius` (= `--r-lg`), `--shadow` (= `--shadow-1`), `--bar` (= `--accent`).

## Shared classes

`card`, `grid`, `stack`, `page-head`, `kv`, `counts`, `big`, `badge` (`ok`/`warn`/`bad`/`active`),
`dot`, `btn` (`btn-primary`/`btn-danger`/`btn-quiet`/`btn-small`/`btn-icon`), `input`, `field`,
`fields`, `table` + `table-wrap` (rows become cards ≤ 600 px; give each `td` a `data-label`),
`selection-bar`, `empty` (empty state: a `strong` line then a `p`).

## Nav icons

A page link `<a href="#/x" data-page="x">` gets a generic icon. To give it its own, add one CSS
rule: `#nav a[data-page="x"] { --icon: url("data:image/svg+xml,…"); }` with a 24×24 stroked
SVG (stroke `black`; it is used as a mask, so the colour comes from the theme).
