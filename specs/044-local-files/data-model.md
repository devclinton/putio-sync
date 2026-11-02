# Data Model: Local Files Page

No new tables. One new query: delete a `processed_torrents` row by `file_checksum`.

## Root

| Field | Type | Notes |
|-------|------|-------|
| key | string | `downloads`, `watch-0`, `watch-1`, … (index into `[watch] paths`) |
| label | string | "Downloads", or "Watch: <last path segment>" |
| path | string | The configured path as written in the config |
| exists | bool | The path is a folder that can be read |
| watch | bool | Re-queue is offered only in watch roots |

## Entry

| Field | Type | Notes |
|-------|------|-------|
| name | string | Last path segment (lossy UTF-8) |
| path | string | Relative to the root, `/`-separated |
| kind | `file` \| `dir` \| `link` | A link is never followed |
| size | u64 | Bytes; 0 for folders and links |
| modified | string? | RFC 3339 UTC |
| partial | string? | For `.<name>.downloading`: `<name>` |
| rejected | string? | For `<name>.rejected`: `<name>` |
| in_use | bool | A running download writes this partial, or this folder holds one |
| valid_name | bool | False when the name is not UTF-8; no action is offered |

## Summary (R5)

`{files, folders, bytes, in_use}` for one entry, counted without following links.

## Validation

- Relative path: see research R1. The root itself is listable but never an action target.
- New name: one segment, not `.`/`..`, no `/`, `\` or NUL, at most 255 bytes, and free.
- Re-queue: watch root, name ends `.rejected`, original ends `.torrent`/`.magnet`, original free.
