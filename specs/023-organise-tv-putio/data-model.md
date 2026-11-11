# Data Model: Organise TV Shows on Put.io

## OrganiseConfig (`[organise]`)

| Field | Type | Default | Rule |
|---|---|---|---|
| `tv` | bool | `false` | organise streamed TV episodes |
| `tv_folder` | string | `"TV Shows"` | Put.io path from the synced root; no `..`; `""` is the root |
| `apply` | `"preview"` \| `"auto"` | `"preview"` | auto applies in the pass that plans |
| `use_plugins` | bool | `true` | ask 028 folder/rename hooks |

## organise_moves (table)

| Column | Type | Notes |
|---|---|---|
| `file_id` | INTEGER PK | Put.io file id; one move per file |
| `kind` | TEXT | `tv` (024: `movie`) |
| `from_folder` | TEXT | path from the synced root when planned |
| `from_name` | TEXT | name when planned; replaced by the name read on apply |
| `to_folder` | TEXT | target path from the synced root |
| `to_name` | TEXT | target name |
| `by_plugin` | INTEGER | 1 when a plugin set the folder or name |
| `status` | TEXT | `planned`, `applied`, `undone`, `failed` |
| `from_parent_id` | INTEGER NULL | parent folder id read on apply, for undo |
| `error` | TEXT NULL | last failure |
| `created_at`, `updated_at` | TIMESTAMP | |

## State transitions

```text
          plan                 apply ok
 (none) ───────▶ planned ──────────────▶ applied ──undo ok──▶ undone
                  │  ▲                     │                    │
       apply fail │  │ apply again          │ undo fail          │ apply again
                  ▼  │                     ▼ (stays applied,     ▼
                 failed ──────────────────  error set)        applied
 dismiss: planned | failed | undone ──▶ (row removed)
```

A move whose file is already at its target is never stored.
