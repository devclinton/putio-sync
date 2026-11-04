# Contract: HTTP API

All routes need the API token and answer 503 in setup mode.

## `GET /hooks`

```json
{
  "hooks": [
    {
      "name": "home-assistant",
      "enabled": true,
      "kind": "url",
      "target": "https://ha.local:8123/api/…",
      "events": ["download.finished", "storage.low"],
      "timeout_secs": 10,
      "queued": 0,
      "runs": [
        {
          "event_type": "download.finished",
          "event_id": "4b0c…",
          "started_at": "2026-11-04T09:12:33.120Z",
          "duration_ms": 41,
          "result": "success",
          "attempts": 1,
          "exit_code": null,
          "http_status": 200,
          "error": null,
          "stdout": "",
          "stderr": ""
        }
      ]
    }
  ]
}
```

`target` is the redacted URL or the program path (no arguments, which may hold secrets).
`runs` holds the last 20, newest first. `result` is `success`, `failure` or `timeout`.

## `POST /hooks/test`

Body `{ "name": "home-assistant" }`. Runs one `hook.test` event against that hook now
(enabled or not, without retries) and returns the run object as above, with `200` even
when the hook failed. `404` for an unknown name. `409` while the same hook is already
running a test.

## `GET /events/schema`

The JSON Schema document, `Content-Type: application/schema+json`.

## Settings group `hooks`

- `GET /settings/hooks` → `{ "hooks": [HookConfig…] }`, with every header and env value
  replaced by `"********"` and each URL cut down as in logs (`https://h.lan/api/…`).
- `PUT /settings/hooks` with the same shape. A `"********"` value keeps the stored value
  for that hook name and key; the cut-down form of the stored URL keeps the stored URL. Validated with `Config::validate`; a 400 names the hook and
  field. Applies live: the running service is reconfigured.
- `POST /settings/hooks/preview` → the usual preview (changes, risks), with values masked.
- `GET /settings/export` masks hook header and env values; `POST /settings/import`
  refuses a file that still has a mask the current config cannot resolve.
