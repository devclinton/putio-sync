# Data Model: Media Server Refresh

## Manifest additions (`plugin.toml`)

| Item | Fields | Rules |
|------|--------|-------|
| `Action` (`[[actions]]`) | `id`, `label`, `help?` | id `[a-z0-9_]{1,40}`, unique; ≤ 8 actions; label 1–40 chars |
| `ConfigField.choices` | action id | must name a declared action; field type `string`, `url` or `string_list` |
| `FieldType::PathMap` | — | value is a list of `PathRule`; never `secret`; default must be valid |
| `tick_seconds` | integer | 5–3600; default 60 (028 behaviour) |

## Settings value: `PathRule`

`{ from: String, to: String }`, stored as an array of tables in `[[plugins]].config`.

- ≤ 16 rules; `from` starts with `/`; `to` starts with `/`, `X:\`, `X:/` or `\\`.
- Both trimmed of trailing separators (except a bare `/` or `X:\`).

## `ActionResult` (what `on_action` returns)

| Field | Type | Rules |
|-------|------|-------|
| `message` | string? | ≤ 500 chars, scrubbed |
| `code` | string? | ≤ 32 chars, scrubbed |
| `open_url` | string? | `http`/`https` only |
| `choices` | list of `{value, label?}`? | ≤ 200 items, strings |
| `next` | `{action, input?, after_seconds?}`? | action declared; after 1–30 s (default 2) |
| `done` | bool? | ends the page loop |
| `save` | table? | keys must be manifest fields; values checked by type |

Anything else in the table is ignored. A non-table return is an error.

## `HealthReport` (what `health()` returns)

A list of `{ status = "ok"|"warn"|"fail", message = string }` items (≤ 8); a single item table is accepted too.
Scrubbed. A Lua error counts as `fail` with the error text.

## Pending folder (Lua, in memory)

`pending[library_key .. "\0" .. mapped_folder] = { folder, library, first, last, count, tries }`

Lifecycle: created on `download.finished` → `count`/`last` updated by more files → refreshed
when due (deleted on success; `tries+1` and window restarted on failure; dropped after 2
tries) → all flushed on `sync.completed`. Lost on reload (a plugin restart).

## Counters

`putio_sync_plugin_counter_total{plugin, name}`: the media plugins use `refreshes`,
`refresh_failures`, `folders_grouped`, `skipped`. ≤ 16 names per plugin; extra names are
refused with a Lua error naming the limit.

## Plugin store keys

| Plugin | Key | Value |
|--------|-----|-------|
| plex | `client_id` | 32 hex chars, made once |
