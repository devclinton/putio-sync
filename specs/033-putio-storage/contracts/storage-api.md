# Contract: Storage API (`/putio/storage`)

All routes require the API token (Bearer or session cookie) and answer 503 in setup mode.
Errors use the API's usual `{"error": "..."}` body.

## GET /putio/storage

Account numbers and what fills it. 502-style client error when Put.io fails.

```json
{
  "size": 107374182400, "used": 96636764160, "free": 10737418240,
  "used_percent": 90.0, "low": true,
  "warn_free_percent": 10, "target_free_percent": 20,
  "auto_free": false, "run_at": "02:30",
  "top": [{ "id": 10, "name": "Movies", "size": 64424509440, "is_folder": true, "share_percent": 66.7 }]
}
```

`top` is up to 10 root items, largest first. With `size ≤ 0`, `low` is false.

## POST /putio/storage/plan

No body. Builds a preview; deletes nothing. 409 while a free-up is running; 503 without a
database.

```json
{
  "id": "4b7e…", "free_before": 5368709120, "target_bytes": 21474836480,
  "needed_bytes": 16106127360, "total_bytes": 16500000000, "free_after": 21868709120,
  "shortfall_bytes": 0, "complete": true,
  "files": [{ "id": 31, "path": "/Movies/Old.mkv", "size": 8000000000,
              "created_at": "2026-01-02T03:04:05", "local_path": "/data/Movies/Old.mkv" }]
}
```

`needed_bytes = 0` and `files = []` when free space already meets the target.

## POST /putio/storage/free

```json
{ "plan_id": "4b7e…", "file_ids": [31, 32] }
```

Deletes the listed files of that plan after re-checking each. 400 on a bad body or empty
`file_ids`; 409 when the plan is unknown or expired ("Preview again") or a run is going.

```json
{
  "run_id": 7, "bytes_deleted": 8000000000, "free_after": 13368709120,
  "deleted": [{ "id": 31, "path": "/Movies/Old.mkv", "size": 8000000000, "created_at": "…", "local_path": "…" }],
  "skipped": [{ "id": 32, "path": "/Movies/Gone.mkv", "reason": "no longer on Put.io" }],
  "failed": []
}
```

`run_id` is null when nothing was deleted.

## GET /putio/storage/runs

Latest 20 runs, newest first. `[]` without a database.

```json
[{ "id": 7, "finished_at": "2026-11-03 10:00:00", "trigger": "manual", "files": 1,
   "bytes": 8000000000, "failed": 0, "free_after": 13368709120,
   "deleted": [{ "file_id": 31, "path": "/Movies/Old.mkv", "size": 8000000000,
                 "created_at": "…", "local_path": "…" }] }]
```

## Settings: GET/PUT /settings/storage, POST /settings/storage/preview

Body: `{ "warn_free_percent": 10, "target_free_percent": 20, "auto_free": false,
"run_at": "02:30" }`. Same save/preview semantics as the other groups (015). Invalid
levels → 400.

## CLI

`putio-sync files free-space [--dry-run] [-y|--yes]`: prints the plan table (path, size,
added, local path) and totals; `--dry-run` stops there; otherwise asks "Delete N files
(X) from Put.io? [y/N]" unless `-y`, then prints deleted/skipped/failed. Exit code 1 when
any file failed.

## Metrics

- `putio_sync_putio_storage_size_bytes`, `putio_sync_putio_storage_used_bytes` (gauges)
- `putio_sync_storage_free_runs_total{trigger="manual|schedule",result="success|failure"}`
- `putio_sync_storage_freed_files_total`, `putio_sync_storage_freed_bytes_total`
