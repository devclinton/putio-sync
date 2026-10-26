# API Contract: Empty-Folder Cleanup

All routes require `Authorization: Bearer <api token>` (401 otherwise), like every other
protected route.

## POST /putio/cleanup/empty-folders

Request body (JSON, `dry_run` required):

```json
{ "dry_run": true }
```

200 response:

```json
{
  "dry_run": true,
  "scanned": 42,
  "removed_count": 3,
  "removed": [
    { "id": 11, "path": "/A", "age_hours": 400 },
    { "id": 13, "path": "/B/C", "age_hours": 72 },
    { "id": 12, "path": "/B", "age_hours": 72 }
  ],
  "kept_protected": 1,
  "kept_too_new": 2,
  "failures": [ { "id": 20, "step": "delete", "path": "/D", "error": "..." } ],
  "duration_ms": 812
}
```

- 400: missing or non-boolean `dry_run` (`{"error": ...}`).
- 409: a cleanup is already running in this process (`{"error": "Empty-folder cleanup is already running"}`).
- 502/500: the run could not start (Put.io error listing the root or transfers).

## GET /settings/cleanup

```json
{ "enabled": false, "run_at": "03:00", "min_age_hours": 24, "nested": true }
```

## PUT /settings/cleanup

Same body; a missing `nested` means `true`. Validated (`run_at` `HH:MM` 24h, `min_age_hours` ≤ 8760); 400 on error with
nothing saved. Saved with `Config::save_keeping_file_credentials`. Response
`{"message": ...}`: under `daemon` all fields apply from the next scheduled check;
under `server` the message notes the daily run only happens under `daemon`.

`PUT /settings/watcher` also refreshes the protected set (new `parent_folder_id`).
