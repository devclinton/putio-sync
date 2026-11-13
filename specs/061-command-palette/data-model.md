# Data model: Command palette

Nothing is stored. Everything here lives in the browser for as long as the palette is open,
or is part of an API response.

## Palette result (UI)

| Field | Meaning |
|---|---|
| `group` | `actions`, `pages`, `settings`, `queue`, `putio`, `local` |
| `label` | the name shown and matched |
| `detail` | optional: a settings section, a sync folder name, a folder path, a size |
| `run()` | goes to a place (`navigate(hash)`) or runs an action |
| `score` | from `fuzzy.js`; sorts results within a group |

Groups are always shown in the order above. Empty groups are hidden. Each group shows up to 8
results.

## Settings entry (UI, `settings/catalog.js`)

| Field | Meaning |
|---|---|
| `section` | the section id from `SECTIONS` |
| `card` | the card id (`card-<id>`) |
| `address` | the `?field=` value: `card-<id>` or `<key>.<name>` |
| `label` | the card title, or the field label (with the card title as the detail) |
| `hint` | the field's help text, also matched |

## Put.io file match (API)

`GET /putio/files/search?q=` → `{ files: [PutioHit] }`

| Field | Type | Meaning |
|---|---|---|
| `id` | i64 | put.io file id |
| `name` | string | file or folder name |
| `parent_id` | i64 | the folder to open |
| `is_folder` | bool | from `file_type == "FOLDER"` |
| `size` | u64 | bytes |

## Local file match (API, `putio-sync-file-manager`)

`GET /local/files/search?q=` → `{ hits: [LocalHit], truncated: bool }`

| Field | Type | Meaning |
|---|---|---|
| `root` | string | root key (`download`, `watch-0`…) |
| `root_label` | string | the root's label |
| `path` | string | relative to the root, `/`-separated |
| `name` | string | the last path segment |
| `kind` | `file` \| `dir` | shortcuts are never returned |
| `size` | u64 | bytes; 0 for folders |

`truncated` is true when the search stopped at 50 hits or 2 seconds.

## Validation

- `q` is trimmed. Fewer than 2 characters → 400. Longer than 200 → 400.
- Names that are not valid UTF-8 are skipped.
