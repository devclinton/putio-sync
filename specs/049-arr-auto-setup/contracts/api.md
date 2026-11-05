# putio-sync routes (049)

All protected (API token). The target is as in 018's SetupRequest:
`{"application": <index>}` (saved key used when `api_key` is empty) or `{"url": "<address>"}`
(with `api_key`). A body error (no key, no such application) is a 4xx `ApiError`; a problem
with the app is a 200 with `ok: false`.

## POST /arr/setup/check

Body: `{target, api_key?, host?}`. Changes nothing in the app.

```json
{
  "ok": true,
  "kind": "sonarr", "app": "Sonarr", "url": "http://sonarr:8989", "host": "putio-sync",
  "folder": {
    "path_check": {"status": "not_visible", "folder": "/downloads", "arr_path": "/downloads", "mappings": [], "detail": null},
    "proposal": {"action": "create", "host": "putio-sync", "remote_path": "/downloads/", "local_path": "/data/downloads/", "id": null},
    "found": "/data/downloads",
    "looked": ["/data/media", "/data", "/data/downloads"]
  },
  "folder_error": null,
  "recommendations": [{"name": "import_extra_files", "on": false}],
  "recommendations_error": null
}
```

`folder_error` is `{reason, detail}` (`no_client`, `marker_failed`, or an app error) when the
folder step couldn't run; `recommendations_error` likewise. Failure to reach the app at all:
`{"ok": false, "reason": "...", "detail": "..."}`.

## POST /arr/setup/mapping

Body: `{target, api_key?, host?, proposal}`. Re-runs the check (R4); writes only when the new
proposal equals `proposal` and is `create`, `update` or `remove`.

- Written: `{"ok": true, "done": "create", "path_check": {...}}`. For a saved application the
  check replaces the one in `GET /arr`.
- Nothing to do: `{"ok": true, "done": "none", "path_check": {...}}`.
- Different: `{"ok": false, "reason": "changed", "proposal": {...}}`.
- Refused: `{"ok": false, "reason": "arr_refused", "detail": "<app's message>"}`.

## POST /arr/setup/recommendation

Body: `{target, api_key?, name}`, `name` one of `completed_download_handling`,
`import_extra_files`. Turns that one on; returns `{"ok": true, "recommendations": [...]}` read
again afterwards, or `{"ok": false, reason, detail}`. An unknown name is a 400.
