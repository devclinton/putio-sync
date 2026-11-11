# Data Model: Stream Mode with DLNA

## SyncMode (config value)

`"download"` (default) | `"stream"`. On `[download] mode` and `[[download.rules]] mode`
(optional, inherited). Resolved into `Effective.mode: Sourced<SyncMode>`.

## Decision (rules)

Adds `Decision::Stream { rule: Option<String> }`: the file is accepted but kept on Put.io.
`rule` is the deepest rule that set `mode`, `None` for `[download]`. `reason()` is
`"kept on Put.io (stream mode)"`.

## TreeDecision (sync pass report)

Adds `Streamed`. `accepted()` stays `Queued | Known` (a streamed file is not downloaded).
`SyncStats.files_streamed` counts them.

## stream_files (SQLite)

| Column | Type | Notes |
|--------|------|-------|
| `file_id` | INTEGER PK | Put.io file id |
| `filename` | TEXT NOT NULL | |
| `folder_path` | TEXT NOT NULL | relative to the Put.io root |
| `size` | INTEGER NOT NULL | bytes |
| `pending` | TEXT NOT NULL | space-separated step names still to run; `''` when done |
| `attempts` | INTEGER NOT NULL DEFAULT 0 | failed step runs |
| `last_error` | TEXT | last `Retry` reason |
| `first_seen` | TIMESTAMP DEFAULT CURRENT_TIMESTAMP | |

Transitions: inserted by the pass with `pending = all step names` → each `Done` removes a name
→ `''` (done). A row is deleted when a full pass no longer decides `Stream` for its file.

## ArrivedFile (in memory)

`file_id`, `name`, `folder`, `size` — what a step gets.

## DlnaConfig

`putio_folder: Option<i64>` unchanged; `putio_root()` returns it or `0` (the synced folder).
