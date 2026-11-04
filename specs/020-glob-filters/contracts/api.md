# Contract: API changes (020)

Changes to the 009 folder-rule routes (`specs/009-folder-rules/contracts/api.md`). All
require the API token; in setup mode they answer 503 like every non-wizard route.

## RuleBody

Gains `include` and `exclude` (string or null). `null` = inherit; `""` = clear the
inherited pattern; anything else is checked strictly (400 `{ "error": "Only download: unclosed quote" }`).

## Effective

Gains:

```json
"include": { "value": "*.mkv OR *.mp4", "from": "Movies" },
"exclude": { "value": "", "from": null }
```

`value` is `""` when no pattern is in force.

## `GET /rules` → `global`

Gains `include` and `exclude` (the `[download] whitelist_patterns` / `blacklist_patterns`
text, `""` when unset).

## `POST /rules/preview`

```json
{ "folder_id": 0, "global": { "include": "*.mkv", "exclude": "*sample*" } }
```

- `global` (optional) replaces the account-wide Only / Never download values in the
  candidate config (null or `""` = unset). It is allowed with `folder_id: 0`; `rule` on
  the root is still refused.
- Response gains `downloads`: up to 50 `{ "file_id", "name", "path" }` that would be
  downloaded. Items refused by a pattern have `decision: "blocked"` and a reason naming
  the pattern.

## `PUT /settings/download`

Unchanged shape. `whitelist_patterns` / `blacklist_patterns` are now checked strictly;
400 names the setting.
