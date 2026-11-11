# API contract: organise (feature 023)

All routes are under `/api/v1` and need the API token.

## GET /organise

```json
{
  "available": true,
  "moves": [
    {
      "file_id": 7, "kind": "tv", "status": "planned", "by_plugin": false,
      "from_folder": "Show.Name.S02E05.1080p.WEB-DL.x264-GRP",
      "from_name": "Show.Name.S02E05.Title.1080p.WEB-DL.x264-GRP.mkv",
      "to_folder": "TV Shows/Show Name/Season 02",
      "to_name": "Show Name - S02E05 - Title.mkv",
      "error": null, "updated_at": "2026-11-10 18:00:00"
    }
  ]
}
```

`available` is false under `server` (no organiser runs); `moves` is then empty. Newest first, at
most 500.

## POST /organise/apply · /organise/undo · /organise/dismiss

Body `{"file_ids": [7, 8]}` (1–200 ids). Reply:

```json
{ "results": [ { "file_id": 7, "status": "applied", "error": null },
               { "file_id": 8, "status": "failed", "error": "already exists in TV Shows/…" } ] }
```

Unknown ids give `{"status": "missing"}`. Apply takes `planned`, `failed` and `undone` moves;
undo takes `applied`; dismiss takes anything but `applied`. 503 when no organiser runs.

## POST /organise/scan

Plans moves for streamed files with no move yet. Reply `{"planned": 3}`.

## GET /settings/organise · PUT /settings/organise · POST /settings/organise/preview

Body as in [config.md](config.md): `{"tv": true, "tv_folder": "TV Shows", "apply": "preview",
"use_plugins": true}`. The PUT answers `{"message": …}`; every field applies at once.
