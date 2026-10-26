# Contract: folder rule routes (feature 009)

All routes require the API token (Constitution VI). Bodies are JSON; a body that doesn't parse is 400 with `{ "error": ... }`. Every save validates the whole config and writes it with `save_keeping_file_credentials`; a refused save (400) changes nothing in memory or on disk. A save hands the new rules to the download manager, so they apply from the next sync and download.

## Shared shapes

**RuleBody** (every field optional; an empty body on PUT means "clear the rule"):

```json
{ "preset": "video", "allow": ["nfo"], "block": [], "skip": null,
  "delete_blocked": true, "delete_after_download": null, "skip_existing": null }
```

**Effective**: each part with `from`, the rule path it came from, or `null` for the global `[download]` settings.

```json
{
  "filter": { "only": ["mkv", "mp4", "nfo"], "blocked": ["exe"], "from": "Movies" },
  "preset": { "value": "video", "from": "Movies" },
  "skip": { "value": false, "from": null },
  "delete_blocked": { "value": true, "from": "Movies" },
  "delete_after_download": { "value": false, "from": null },
  "skip_existing": { "value": true, "from": null }
}
```

`filter.only` is `null` for a block-list filter. `preset` is `{ "value": null, "from": null }` when no rule on the path sets one.

## `GET /rules`

```json
{
  "rules": [{ "path": "Movies", "preset": "video", "allow": [], "block": [], "skip": null, "delete_blocked": true, "delete_after_download": null, "skip_existing": null }],
  "presets": [{ "name": "video", "label": "Video only", "builtin": true, "overridden": false, "only": ["mkv", "..."], "block": [], "used_by": ["Movies"] }],
  "global": { "blocked_extensions": ["exe", "..."], "delete_after_download": false, "skip_existing": true }
}
```

Rules in config order; presets built-ins first (fixed order), then user presets by name.

## `GET /rules/folder/{id}`

Put.io folder `id` (0 = root). 200:

```json
{ "folder_id": 100, "path": "Movies", "rule": { RuleBody } | null, "effective": { Effective } }
```

404 when Put.io says the folder doesn't exist; 400 when `id` is a file. Root: `path` is `""`, `rule` is always `null`.

## `PUT /rules/folder/{id}`

Body RuleBody. Sets (replaces) the rule for the folder's path; an all-empty body removes it. 400 for the root (`"The root uses the Download settings"`), an unknown preset, a bad extension. 200 `{ "message": "Rule for Movies saved. Applies from the next sync." }`.

## `DELETE /rules/folder/{id}`

Removes the folder's rule. 200 `{ "message": "Rule for Movies removed." }` (also when there was none: `"No rule for Movies; nothing changed."`).

## `POST /rules/delete`

`{ "path": "Movies" }`: remove a rule by path (Settings, works for folders that no longer exist). 404 when no rule has that path.

## `PUT /rules/presets/{name}`

`{ "only": ["cbz", "cbr"] }` or `{ "block": ["iso"] }`. Creates a user preset or overrides a built-in. 400 for a bad name or extension, or both `only` and `block`.

## `DELETE /rules/presets/{name}`

Built-in: removes the override (reset); 200 even when not overridden. User preset: removes it; 409 `{ "error": "Preset comics is used by 2 rules", "details": "Comics, Kids/Comics" }` while a rule uses it; 404 when it doesn't exist.

## `POST /rules/preview`

```json
{ "folder_id": 100, "rule": { RuleBody } }
```

`rule` omitted: the saved rules. Present: the folder's rule replaced by it (an empty one = cleared). Lists the folder's tree and decides each file; nothing is changed.

```json
{
  "path": "Movies",
  "files": 42, "download": 30, "blocked": 10, "skipped": 2, "would_delete": 9,
  "items": [{ "file_id": 102, "name": "setup.exe", "path": "Movies", "decision": "blocked",
              "reason": "extension exe is blocked", "rule": "Movies", "would_delete": true }],
  "truncated": false
}
```

`items` holds blocked and skipped files only, first 200; `would_delete` is false for queued files.

## `GET /rules/decisions?limit=N`

Newest first, `limit` 1..=500 (default 100):

```json
{ "items": [{ "file_id": 102, "name": "setup.exe", "path": "Movies", "decision": "blocked",
              "reason": "extension exe is blocked", "rule": null, "decided_at": "2026-10-26T10:00:00Z" }] }
```

503 without a database.

## `POST /rules/decisions/clear`

200 `{ "message": "Cleared 12 decisions" }`.

## Changed: `POST /local/queue`

Response gains `deleted` (files deleted from Put.io by `delete_blocked`). `blocked` now counts files blocked or skipped by any rule.
