# Data Model: Select All, Sort and Filter

Nothing is stored. These are the in-browser shapes.

## Filter

- `text`: what was typed (kept in the address as `q`).
- `groups`: `Term[][]`. The outer list is AND, each inner list is OR.
- `Term`: `{ re, not }`. `re` is a case-insensitive `RegExp`, anchored (`^…$`) when the pattern
  holds `*` or `?`.
- Empty `groups` matches everything.

## Sort

- `key`: one of the page's column keys.
- `desc`: boolean. Kept in the address as `sort=<key>-asc|desc`; absent means server order.

| Page | Keys (heading) | Folder-first |
|------|----------------|--------------|
| Put.io | `name` (Name), `size` (Size), `date` (Date, `created_at`) | yes |
| Transfers | `name`, `status`, `progress` (`percent_done`), `size`, `added` (`created_at`) | no |
| Local files | `name`, `size` (folders: none), `modified` | yes |

## Selection (existing, feature 008)

- `reset(ids)` takes the ids of the rows shown after filtering and not disabled; selected ids
  outside it are dropped. Local files uses the entry `path` as the id.
