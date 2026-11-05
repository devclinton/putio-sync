# API Contract: Download Verification

All routes need the usual auth; in setup mode they return 503.

## `GET /verify`

```json
{
  "enabled": true,
  "status": {
    "state": "running",            // idle | running | paused | waiting | stopping
    "run_id": 4,
    "scope": "everything",          // everything | paths
    "report_only": false,
    "files_total": 1200, "files_done": 310,
    "bytes_total": 2400000000000, "bytes_done": 610000000000,
    "counts": { "verified": 305, "mismatch": 1, "missing": 3, "not_on_putio": 1,
                "busy": 0, "unreadable": 0, "not_checked": 0 },
    "current": "Movie (2020).mkv",
    "waiting_reason": null,         // "bandwidth paused" while waiting
    "read_limit_bytes_per_sec": 50000000
  },
  "last_run": { "id": 3, "scope": "everything", "report_only": false, "state": "finished",
                "error": null, "started_at": "…", "ended_at": "…",
                "files_total": 1190, "files_done": 1190, "bytes_total": 0, "bytes_done": 0,
                "counts": { "...": 0 } },
  "problems": [
    { "id": 9, "run_id": 3, "source": "run", "file_id": 123, "name": "a.mkv",
      "local_path": "/downloads/a.mkv", "result": "mismatch",
      "detail": "expected 1a2b3c4d, got 99999999", "queued": true,
      "at": "…", "fixed_at": null }
  ]
}
```

`problems`: unfixed problems of the last run plus unfixed `download` problems from the last
7 days, newest first, at most 200.

## `POST /verify`

Body `{ "paths": ["/downloads/a.mkv"]?, "report_only": false }`. No `paths` = everything.

- 202 with the `status` object.
- 409 `{"error":"a verify run is already going"}`.
- 400 when `paths` is given but none of them is a file putio-sync downloaded.

## `POST /verify/pause`, `POST /verify/resume`, `POST /verify/stop`

200 with the `status` object; 409 when there is no run (or, for resume, it is not paused).

## History (`GET /history`)

Each download row gains `verify` (`"crc32"`, `"size"`, `"none"` or `null`) and
`redownloads` (integer or `null`).

## Settings (`/settings/download`)

Gains `verify`, `verify_max_redownloads`, `verify_read_limit_mb_per_sec`,
`verify_follow_bandwidth_pause`; applied live.

## Queue / active (`GET /queue`, active downloads)

`phase` may be `"verifying"` (bytes and speed then measure the read).
