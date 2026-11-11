# Data Model: Lua Plugins, Sync Extension Points

## Hook (enum)

`filter` | `rename` | `folder`. Text form is the Lua function name and the manifest value.

## HookFile (what a plugin is told)

| Field | Type | Notes |
|-------|------|-------|
| `id` | integer | Put.io file id |
| `name` | text | Put.io name |
| `size` | integer | bytes |
| `extension` | text | lowercase, without the dot; `""` when none |
| `folder` | text | Put.io folder relative to the synced root, `""` for the root |
| `decision` | text | `download`, `stream`, `blocked` or `skipped` (filter: built-in; rename/folder: `download`) |
| `reason` | text | built-in reason in words, `""` for a download |

## FilterAnswer

`download` | `stream` | `skip`; `nil`/`null` = no answer. Lua `true` = download, `false` = skip.

## Rename answer

A text: one file name, not empty, not `.`/`..`, no `/` or `\`, at most 255 bytes; then made
safe like Put.io names. `nil` = no answer.

## Folder answer

A text: a relative folder; `/` separated; no `..` segment; leading `/` refused; empty segments
dropped; each segment made safe. `""` means the download folder itself. `nil` = no answer.

## PluginSettings (added)

| Field | Type | Default | Range |
|-------|------|---------|-------|
| `hook_timeout_ms` | integer | 500 | 50–5000 |

## Manifest (added)

| Field | Type | Notes |
|-------|------|-------|
| `hooks` | list of Hook | optional; listed once each; with hooks, `events` may be empty |

## Hook failure state (in memory, per plugin)

`failures_in_a_row: u32`, `paused_until: Option<Instant>`. 3 failures → paused 5 minutes.
Reset on success and when the plugin's slots are rebuilt.
