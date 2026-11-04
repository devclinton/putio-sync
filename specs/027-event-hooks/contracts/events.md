# Contract: Events (schema version 1)

Source of truth: `putio-sync-core/src/events/schema.json`. Reference for users:
`EVENTS.md`. Served at `GET /events/schema`.

## Envelope

| Field | Type | Notes |
|-------|------|-------|
| `schema_version` | integer | `1`. Raised only when a field is removed, renamed or changes type |
| `id` | string | UUID v4, unique per event |
| `type` | string | one of the types below |
| `occurred_at` | string | UTC, RFC 3339 with milliseconds (`2026-11-04T09:12:33.120Z`) |
| `source` | object | `{ "app": "putio-sync", "version": "0.1.1", "host": "nas01" }` |
| `data` | object | per type |

Consumers MUST ignore fields they do not know.

## Types and `data`

`download.finished`

| Field | Type |
|-------|------|
| `file_id` | integer (Put.io file id) |
| `name` | string (file name as saved) |
| `relative_path` | string (path under the download folder, `/`-separated) |
| `local_path` | string (absolute path on the daemon's disk) |
| `size_bytes` | integer |
| `duration_seconds` | number |
| `average_bytes_per_second` | integer |
| `resumed` | boolean |

`download.failed`

| Field | Type |
|-------|------|
| `file_id`, `name`, `relative_path` | as above |
| `size_bytes` | integer |
| `error` | string (token-free) |
| `reason` | `"network"`, `"http"`, `"io"`, `"integrity"`, `"other"` |
| `attempts` | integer |

`transfer.added`, `transfer.finished`, `transfer.failed`

| Field | Type | Present |
|-------|------|---------|
| `transfer_id` | integer | always |
| `name` | string | always |
| `origin` | `"radarr"`, `"sonarr"`, `"watch"`, `"manual"`, `"putio"` | always |
| `status` | string (Put.io status, e.g. `DOWNLOADING`) | always |
| `size_bytes` | integer or null | always |
| `file_id` | integer or null (the Put.io file or folder made) | always |
| `error` | string or null | always; set on `transfer.failed` |

`sync.completed`

| Field | Type |
|-------|------|
| `trigger` | `"schedule"`, `"manual"` |
| `result` | `"success"`, `"failure"` |
| `error` | string or null |
| `started_at` | string (RFC 3339) |
| `duration_seconds` | number |
| `files_found`, `files_queued`, `files_skipped`, `files_blocked`, `files_failed` | integer |

`storage.low`

| Field | Type |
|-------|------|
| `total_bytes`, `used_bytes`, `free_bytes` | integer |
| `free_percent` | number |
| `warn_free_percent` | integer |

`hook.test`

| Field | Type |
|-------|------|
| `hook` | string (the hook's name) |
| `message` | string |

## Delivery

- Command: the event as one JSON document on stdin (UTF-8, no trailing newline needed),
  plus `PUTIO_SYNC_EVENT_TYPE`, `PUTIO_SYNC_EVENT_ID`, `PUTIO_SYNC_HOOK_NAME`.
- Webhook: `POST`, `Content-Type: application/json`, `User-Agent: putio-sync/<version>`,
  the configured headers, the event as the body.
