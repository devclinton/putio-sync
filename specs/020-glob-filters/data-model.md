# Data Model: Glob Filters with AND and OR

## Pattern (core, `pattern::Pattern`)

- `text`: the trimmed source text; empty = not set.
- `groups`: AND-ed groups of OR-ed terms. Term: `not`, `path` (text has `/`), `alts`
  (brace-expanded token lists, each anchored; a non-glob term is wrapped in `*…*`).
- Equality and serialisation by `text`.
- `matches(name, path) -> bool`; empty matches everything.

## FolderRule (009) — new keys

| Key | Type | Meaning |
|-----|------|---------|
| `include` | string, optional | Only download: files must match. `""` clears the inherited value. |
| `exclude` | string, optional | Never download: matching files are refused. `""` clears. |

`is_empty()` counts them. `validate` checks both strictly.

## `[download]` — existing keys, now applied

| Key | UI label | Root value of |
|-----|----------|---------------|
| `whitelist_patterns` | Only download | `include` |
| `blacklist_patterns` | Never download | `exclude` |

## Effective (009) — new parts

- `include`, `exclude`: `Sourced<Pattern>` (`from` = rule path or `null` for global).
- `folder` (not serialised): the folder path the effective rule was resolved for.

## Reason — new variants

- `Excluded { pattern }` → "matches never-download pattern `P`"
- `NotIncluded { pattern }` → "doesn't match only-download pattern `P`"

## RulePreview (download) — new part

- `downloads`: up to 50 `{ file_id, name, path }` that would be downloaded.
