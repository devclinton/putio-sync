# API Contract: Local Files

All routes need `Authorization: Bearer <token>` (401 otherwise). Errors use the API's usual
`{"error": "...", "details": "..."}` body. `path` is relative to the root, `/`-separated; empty
means the root.

| Status | When |
|--------|------|
| 400 | Bad path or name, unknown action for this entry (e.g. re-queue on a non-`.rejected` file) |
| 404 | Unknown root key, root missing on disk, or entry not found |
| 409 | Target name taken, or a running download writes the entry |

## GET /local/files

```json
{"roots": [{"key": "downloads", "label": "Downloads", "path": "/downloads", "exists": true, "watch": false},
           {"key": "watch-0", "label": "Watch: radarr", "path": "/watches/radarr", "exists": true, "watch": true}]}
```

## GET /local/files/{root}?path=sonarr

```json
{"root": "downloads", "path": "sonarr", "truncated": false,
 "entries": [{"name": ".Show.S01E01.mkv.downloading", "path": "sonarr/.Show.S01E01.mkv.downloading",
              "kind": "file", "size": 1048576, "modified": "2026-11-01T10:00:00Z",
              "partial": "Show.S01E01.mkv", "rejected": null, "in_use": true, "valid_name": true}]}
```

The path must name a folder (400 for a file).

## GET /local/files/{root}/summary?path=sonarr/Show

`{"files": 3, "folders": 1, "bytes": 123456, "in_use": false}`; 400 for the root itself.

## DELETE /local/files/{root}?path=sonarr/Show

Removes a file, link or folder. `{"success": true, "message": "Deleted sonarr/Show"}`.
400 for the root itself; 409 when in use.

## POST /local/files/{root}/rename

Body `{"path": "radarr/Movie.mkv", "name": "Movie (2024).mkv"}` →
`{"success": true, "message": "...", "path": "radarr/Movie (2024).mkv"}`.

## POST /local/files/{root}/requeue

Body `{"path": "x.torrent.rejected"}` → `{"success": true, "message": "...", "path": "x.torrent"}`.
Watch roots only (400 elsewhere).

## GET and POST /local/files/{root}/subtitles

Added by feature 025 ("Fetch subtitles" on a video row). `GET ?path=` answers whether Put.io or
transcription can supply subtitles; `POST {"path"}` starts a `manual:<n>` pipeline item
(`202 {"key"}`, `409` with the running item's key). The path guard is the same as above. See
`specs/025-subtitles/contracts/api.md`.

## GET and POST /local/files/{root}/translate

Added by feature 026 ("Translate" on a video row). `GET ?path=` answers whether a translator is
set up and a subtitle exists to translate from, with the wanted languages that are still
missing; `POST {"path", "targets": [...]}` (1 to 10 languages) starts a `manual:<n>` pipeline
item with one translation per language (`202 {"key"}`, `409` with the running item's key,
`400` for a bad target or no translator). The path guard is the same as above. See
`specs/026-subtitle-translation/contracts/api.md`.
