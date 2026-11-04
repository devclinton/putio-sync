# Data Model: Configuration Assistant

Nothing is stored; these are request/response and core types.

## Risk (`putio_sync_core::assistant::Risk`)

| Field | Type | Notes |
|-------|------|-------|
| `id` | string | stable id, e.g. `delete_temp_folder` |
| `level` | `danger` \| `warning` | danger = can lose data |
| `keys` | string[] | dotted keys involved, e.g. `download.delete_after_download` |
| `message` | string | plain words; names paths, never secrets |

Ordered danger first, then by id. The default config yields none.

## Change (`putio_sync_core::assistant::Change`)

| Field | Type | Notes |
|-------|------|-------|
| `key` | string | group field name (group previews) or dotted key (imports) |
| `before` | JSON value \| null | null when unset |
| `after` | JSON value \| null | null when removed |
| `live` | bool | applies without a restart under the running engine |

Group previews diff the group's GET view; imports diff the flattened `settings_table`
(tables flatten to dotted keys; arrays and arrays of tables are compared whole).

## Preview (group preview and import preview response)

| Field | Type | Notes |
|-------|------|-------|
| `valid` | bool | a save / apply would be accepted |
| `error` | string? | the refusal reason when not valid |
| `changes` | Change[] | empty when nothing changes or not valid |
| `when` | string | when live changes take effect, e.g. "Applies from the next download" |
| `risks` | Risk[] | risks of the config the save would produce |
| `unknown` | ConfigWarning[] | import only: keys the config does not know |
| `ignored` | string[] | import only: `auth`, `arr`, `api.auth_token` when present |
| `message` | string? | import apply only: the result message |

## Assistant info (`GET /settings/assistant`)

| Field | Type | Notes |
|-------|------|-------|
| `groups.<group>.affects.<field>` | string | what the field affects |
| `groups.<group>.live` | string[] | fields applied without a restart now |
| `groups.<group>.when` | string | when they take effect |
| `groups.<group>.defaults` | object | the group view of the default config |
| `risks` | Risk[] | risks of the running config |
