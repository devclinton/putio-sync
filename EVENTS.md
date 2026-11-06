# Events

putio-sync puts what happens on one internal event bus. [Event hooks](README.md#event-hooks)
deliver each event as JSON to a command or a webhook, and later features (Lua plugins,
notifications, media-server refresh) read the same events.

This page is the reference for schema version **1**. The machine-readable form is the JSON
Schema at [`putio-sync-core/src/events/schema.json`](putio-sync-core/src/events/schema.json),
which the API also serves at `GET /events/schema`.

## Envelope

Every event has the same outer fields:

| Field | Type | Notes |
|-------|------|-------|
| `schema_version` | integer | `1` |
| `id` | string | UUID v4, unique per event |
| `type` | string | one of the types below |
| `occurred_at` | string | UTC, RFC 3339 with milliseconds, such as `2026-11-04T09:12:33.120Z` |
| `source` | object | `app` (`"putio-sync"`), `version` (the daemon's version) and `host` (the machine's host name) |
| `data` | object | the fields of that type, below |

## Delivery

**Commands** get the event as one JSON document on stdin. They run as the daemon's user,
with a clean environment: `PATH`, `HOME`, `LANG` and `TZ` from the daemon, the hook's own
`env` entries, and:

| Variable | Value |
|----------|-------|
| `PUTIO_SYNC_EVENT_TYPE` | the event's `type` |
| `PUTIO_SYNC_EVENT_ID` | the event's `id` |
| `PUTIO_SYNC_HOOK_NAME` | the hook's `name` |

No other variable of the daemon's reaches the command, so its Put.io and API tokens do
not. Exit code 0 is a success; any other code, a signal or the timeout is a failure.

**Webhooks** get a `POST` with the event as the body, `Content-Type: application/json`,
`User-Agent: putio-sync/<version>` and the hook's `headers`. A 2xx answer is a success.
A connection error, a timeout, 429 or a 5xx is tried again after 5 s and then 30 s; any
other answer is a failure at once. Redirects are followed only to the same host and port,
at most 3 times.

Events reach each hook in the order they happened, one at a time per hook. A hook that
is slow or down never holds up a sync or another hook; up to 1 000 events wait for it,
and past that the oldest are dropped and counted in
`putio_sync_hook_events_dropped_total`.

## Versioning

- New types and new fields can arrive in any release without a version change. Ignore
  the fields and types you do not know.
- `schema_version` goes up only when a field is removed or renamed, or changes type.

## Event types

### `download.finished`

A file was saved to the download folder. Skipped files (already on disk) send nothing.

| Field | Type | Notes |
|-------|------|-------|
| `file_id` | integer | the Put.io file id |
| `name` | string | the file name as saved |
| `relative_path` | string | the path under the download folder, `/`-separated |
| `local_path` | string | the absolute path on the daemon's disk |
| `size_bytes` | integer | |
| `duration_seconds` | number | how long the download took |
| `average_bytes_per_second` | integer | |
| `resumed` | boolean | `true` when it carried on from a partial file |

```json
{
  "schema_version": 1,
  "id": "5d0b6f1e-6a8e-4c38-9b55-2f9f3e0c1a11",
  "type": "download.finished",
  "occurred_at": "2026-11-04T09:12:33.120Z",
  "source": { "app": "putio-sync", "version": "0.1.1", "host": "nas01" },
  "data": {
    "file_id": 1234567890,
    "name": "Big.Buck.Bunny.2008.1080p.mkv",
    "relative_path": "Movies/Big Buck Bunny (2008)/Big.Buck.Bunny.2008.1080p.mkv",
    "local_path": "/downloads/Movies/Big Buck Bunny (2008)/Big.Buck.Bunny.2008.1080p.mkv",
    "size_bytes": 2147483648,
    "duration_seconds": 214.6,
    "average_bytes_per_second": 10006912,
    "resumed": false
  }
}
```

### `download.failed`

A download gave up after its retries. A download you stop sends nothing.

| Field | Type | Notes |
|-------|------|-------|
| `file_id` | integer | the Put.io file id |
| `name` | string | the file name |
| `relative_path` | string | where it would have been saved |
| `size_bytes` | integer | |
| `error` | string | what went wrong; URLs in it are shortened, so it holds no token |
| `reason` | string | `network`, `http`, `io`, `integrity` or `other` |
| `attempts` | integer | the most tries the download had (`max_retries` + 1) |

```json
{
  "schema_version": 1,
  "id": "0f6e3b2a-19c4-4f0e-8d0a-6c1b7a9e2d33",
  "type": "download.failed",
  "occurred_at": "2026-11-04T09:20:01.004Z",
  "source": { "app": "putio-sync", "version": "0.1.1", "host": "nas01" },
  "data": {
    "file_id": 1234567891,
    "name": "episode.mkv",
    "relative_path": "TV/Show/Season 01/episode.mkv",
    "size_bytes": 734003200,
    "error": "HTTP 403 Forbidden from https://s100.put.io/…",
    "reason": "http",
    "attempts": 4
  }
}
```

### `transfer.added`

A transfer appeared on Put.io. The transfers seen when hooks first start are taken as
known, so turning hooks on does not send one event per old transfer. A transfer that is
already finished or failed when it is first seen sends `transfer.added` and then
`transfer.finished` or `transfer.failed`.

| Field | Type | Notes |
|-------|------|-------|
| `transfer_id` | integer | the Put.io transfer id |
| `name` | string | |
| `origin` | string | who added it: `radarr` or `sonarr` (a Radarr or Sonarr category that names the app), `arr` (another Radarr or Sonarr category), `watch`, `manual` or `putio` (outside putio-sync) |
| `status` | string | the Put.io status, such as `IN_QUEUE` or `DOWNLOADING` |
| `size_bytes` | integer or null | |
| `file_id` | integer or null | the Put.io file or folder it made, once known |
| `error` | string or null | |

```json
{
  "schema_version": 1,
  "id": "a3c1f0d2-7b4e-4e8a-9c1d-0e2f3a4b5c66",
  "type": "transfer.added",
  "occurred_at": "2026-11-04T08:00:12.500Z",
  "source": { "app": "putio-sync", "version": "0.1.1", "host": "nas01" },
  "data": {
    "transfer_id": 98765,
    "name": "Big Buck Bunny (2008)",
    "origin": "radarr",
    "status": "IN_QUEUE",
    "size_bytes": null,
    "file_id": null,
    "error": null
  }
}
```

### `transfer.finished`

Put.io finished a transfer (its files are ready to download).

The fields are those of `transfer.added`.

```json
{
  "schema_version": 1,
  "id": "b4d2e1f3-8c5f-4f9b-8d2e-1f3a4b5c6d77",
  "type": "transfer.finished",
  "occurred_at": "2026-11-04T08:41:57.031Z",
  "source": { "app": "putio-sync", "version": "0.1.1", "host": "nas01" },
  "data": {
    "transfer_id": 98765,
    "name": "Big Buck Bunny (2008)",
    "origin": "radarr",
    "status": "COMPLETED",
    "size_bytes": 2147483648,
    "file_id": 1234567800,
    "error": null
  }
}
```

### `transfer.failed`

Put.io gave up on a transfer. A failed transfer that later finishes sends
`transfer.finished` too.

The fields are those of `transfer.added`; `error` holds Put.io's message.

```json
{
  "schema_version": 1,
  "id": "c5e3f2a4-9d6a-4a0c-9e3f-2a4b5c6d7e88",
  "type": "transfer.failed",
  "occurred_at": "2026-11-04T10:02:11.870Z",
  "source": { "app": "putio-sync", "version": "0.1.1", "host": "nas01" },
  "data": {
    "transfer_id": 98766,
    "name": "some.torrent",
    "origin": "watch",
    "status": "ERROR",
    "size_bytes": null,
    "file_id": null,
    "error": "No seeders"
  }
}
```

### `sync.completed`

A sync run (a pass over Put.io for new files to download) ended, on the schedule or
because you asked for one.

| Field | Type | Notes |
|-------|------|-------|
| `trigger` | string | `schedule` or `manual` |
| `result` | string | `success` or `failure` |
| `error` | string or null | set when `result` is `failure` |
| `started_at` | string | RFC 3339 |
| `duration_seconds` | number | |
| `files_found` | integer | files the run looked at |
| `files_queued` | integer | files it queued for download |
| `files_skipped` | integer | files already downloaded or excluded |
| `files_blocked` | integer | files held back by a rule |
| `files_failed` | integer | files it could not queue |

```json
{
  "schema_version": 1,
  "id": "d6f4a3b5-0e7b-4b1d-8f4a-3b5c6d7e8f99",
  "type": "sync.completed",
  "occurred_at": "2026-11-04T09:00:04.210Z",
  "source": { "app": "putio-sync", "version": "0.1.1", "host": "nas01" },
  "data": {
    "trigger": "schedule",
    "result": "success",
    "error": null,
    "started_at": "2026-11-04T09:00:00.000Z",
    "duration_seconds": 4.21,
    "files_found": 120,
    "files_queued": 3,
    "files_skipped": 117,
    "files_blocked": 0,
    "files_failed": 0
  }
}
```

### `storage.low`

Free space on Put.io fell below `[storage] warn_free_percent`. It is sent once when the
space drops below the line, and again only after it has gone back above it.

| Field | Type | Notes |
|-------|------|-------|
| `total_bytes` | integer | |
| `used_bytes` | integer | |
| `free_bytes` | integer | |
| `free_percent` | number | |
| `warn_free_percent` | integer | the configured line |

```json
{
  "schema_version": 1,
  "id": "e7a5b4c6-1f8c-4c2e-9a5b-4c6d7e8f9aa0",
  "type": "storage.low",
  "occurred_at": "2026-11-04T09:15:00.000Z",
  "source": { "app": "putio-sync", "version": "0.1.1", "host": "nas01" },
  "data": {
    "total_bytes": 107374182400,
    "used_bytes": 102005473280,
    "free_bytes": 5368709120,
    "free_percent": 5.0,
    "warn_free_percent": 10
  }
}
```

### `verify.passed`

A new download matched Put.io's CRC32 (or its size, where Put.io has no CRC32). It is sent
just before `download.finished`, after the file got its real name. A "verify everything"
run does not send it (feature 034).

| Field | Type | Notes |
|-------|------|-------|
| `file_id` | integer | the Put.io file id |
| `name` | string | the file name as saved |
| `relative_path` | string | the path under the download folder, `/`-separated |
| `local_path` | string | the absolute path on the daemon's disk |
| `size_bytes` | integer | |
| `method` | string | `crc32` or `size` |
| `crc32` | string or null | Put.io's CRC32 in lower case; `null` when the check was by size |
| `redownloads` | integer | how many times it was downloaded again before it matched |
| `trigger` | string | always `download` |

```json
{
  "schema_version": 1,
  "id": "7c1f0e2a-3b4d-4e5f-8a9b-0c1d2e3f4a5b",
  "type": "verify.passed",
  "occurred_at": "2026-11-04T09:12:33.100Z",
  "source": { "app": "putio-sync", "version": "0.1.1", "host": "nas01" },
  "data": {
    "file_id": 1234567890,
    "name": "Big.Buck.Bunny.2008.1080p.mkv",
    "relative_path": "Movies/Big Buck Bunny (2008)/Big.Buck.Bunny.2008.1080p.mkv",
    "local_path": "/downloads/Movies/Big Buck Bunny (2008)/Big.Buck.Bunny.2008.1080p.mkv",
    "size_bytes": 2147483648,
    "method": "crc32",
    "crc32": "8f2c1a07",
    "redownloads": 0,
    "trigger": "download"
  }
}
```

### `verify.failed`

A file did not match Put.io. For a new download (`trigger` `download`) the bad copy never
gets its real name; it is downloaded again while `will_redownload` is `true`, and when the
limit is spent `download.failed` follows with reason `integrity`. For a "verify everything"
run (`trigger` `run`) the file on disk is left as it is and queued to be downloaded over,
unless the run was "report only".

| Field | Type | Notes |
|-------|------|-------|
| `file_id` | integer | the Put.io file id |
| `name` | string | the file name |
| `relative_path` | string | the path under the download folder, `/`-separated |
| `local_path` | string | the absolute path on the daemon's disk |
| `size_bytes` | integer | the size Put.io gives |
| `expected_crc32` | string or null | Put.io's CRC32; `null` for a size mismatch |
| `actual_crc32` | string or null | what the file hashed to; `null` for a size mismatch |
| `attempt` | integer | which download this was, `1` for the first; `1` for a run |
| `will_redownload` | boolean | whether it is being downloaded again |
| `trigger` | string | `download` or `run` |

```json
{
  "schema_version": 1,
  "id": "a3d9c2b1-5e6f-4a7b-8c9d-0e1f2a3b4c5d",
  "type": "verify.failed",
  "occurred_at": "2026-11-04T09:12:33.090Z",
  "source": { "app": "putio-sync", "version": "0.1.1", "host": "nas01" },
  "data": {
    "file_id": 1234567890,
    "name": "Big.Buck.Bunny.2008.1080p.mkv",
    "relative_path": "Movies/Big Buck Bunny (2008)/Big.Buck.Bunny.2008.1080p.mkv",
    "local_path": "/downloads/Movies/Big Buck Bunny (2008)/Big.Buck.Bunny.2008.1080p.mkv",
    "size_bytes": 2147483648,
    "expected_crc32": "8f2c1a07",
    "actual_crc32": "00a1b2c3",
    "attempt": 1,
    "will_redownload": true,
    "trigger": "download"
  }
}
```

### `verify.redownload`

A file is being downloaded again after a mismatch: a new download starting over, or a
"verify everything" run putting it back on the queue.

| Field | Type | Notes |
|-------|------|-------|
| `file_id` | integer | the Put.io file id |
| `name` | string | the file name |
| `relative_path` | string | the path under the download folder, `/`-separated |
| `size_bytes` | integer | |
| `attempt` | integer | the download about to start (`2` is the first re-download); `1` for a run |
| `max_redownloads` | integer | `[download] verify_max_redownloads` |
| `trigger` | string | `download` or `run` |

```json
{
  "schema_version": 1,
  "id": "b4e0d3c2-6f7a-4b8c-9d0e-1f2a3b4c5d6e",
  "type": "verify.redownload",
  "occurred_at": "2026-11-04T09:12:33.095Z",
  "source": { "app": "putio-sync", "version": "0.1.1", "host": "nas01" },
  "data": {
    "file_id": 1234567890,
    "name": "Big.Buck.Bunny.2008.1080p.mkv",
    "relative_path": "Movies/Big Buck Bunny (2008)/Big.Buck.Bunny.2008.1080p.mkv",
    "size_bytes": 2147483648,
    "attempt": 2,
    "max_redownloads": 2,
    "trigger": "download"
  }
}
```

### `pipeline.step_failed`

A processing pipeline step failed (feature 038). Sent after every failed attempt of a hook step.

| Field | Type | Notes |
|-------|------|-------|
| `key` | string | the item key, such as `transfer:123` |
| `name` | string | the item name |
| `stage` | string | `pre_download`, `pre_processing`, `subtitles`, `translating`, `post_processing` or `notifications` |
| `step` | string | the step name (the hook's name) |
| `attempt` | integer | the attempt that failed, from 1 |
| `required` | boolean | `true` when the item now waits for you; `false` when the step is retried or given up |
| `error` | string | what went wrong, with secrets removed |

```json
{
  "schema_version": 1,
  "id": "c5f1e4d3-7a8b-4c9d-8e1f-2a3b4c5d6e7f",
  "type": "pipeline.step_failed",
  "occurred_at": "2026-11-05T09:12:33.095Z",
  "source": { "app": "putio-sync", "version": "0.1.1", "host": "nas01" },
  "data": {
    "key": "transfer:991",
    "name": "Some.Show.S01E01.1080p",
    "stage": "post_processing",
    "step": "tidy",
    "attempt": 1,
    "required": true,
    "error": "exited with code 3"
  }
}
```

### `pipeline.completed`

An item finished its pipeline and at least one optional step was given up (feature 038). Items that
pass cleanly send no event, so nothing changes for people who set no pipeline steps.

| Field | Type | Notes |
|-------|------|-------|
| `key` | string | the item key |
| `name` | string | the item name |
| `warnings` | array of string | the optional steps that were given up |

```json
{
  "schema_version": 1,
  "id": "d6a2f5e4-8b9c-4dae-9f20-3b4c5d6e7f80",
  "type": "pipeline.completed",
  "occurred_at": "2026-11-05T09:20:01.412Z",
  "source": { "app": "putio-sync", "version": "0.1.1", "host": "nas01" },
  "data": {
    "key": "transfer:991",
    "name": "Some.Show.S01E01.1080p",
    "warnings": ["tidy"]
  }
}
```

### `hook.test`

Sent only to one hook, by the **Send test event** button in Settings › Integrations › Event hooks or
`POST /hooks/test`. A hook gets it whatever its `events` say, and `"*"` does not include
it. Nothing else receives it.

| Field | Type | Notes |
|-------|------|-------|
| `hook` | string | the hook's name |
| `message` | string | |

```json
{
  "schema_version": 1,
  "id": "f8b6c5d7-2a9d-4d3f-8b6c-5d7e8f9aabb1",
  "type": "hook.test",
  "occurred_at": "2026-11-04T09:30:00.000Z",
  "source": { "app": "putio-sync", "version": "0.1.1", "host": "nas01" },
  "data": {
    "hook": "notify",
    "message": "Test event from putio-sync"
  }
}
```

### `plugin.test`

Sent only to one plugin, by the **Send test event** button in Settings › Plugins or
`POST /plugins/test`. A plugin gets it whatever its `events` say, and `"*"` does not
include it. Nothing else receives it, hooks included.

| Field | Type | Notes |
|-------|------|-------|
| `plugin` | string | the plugin's name |
| `message` | string | |

```json
{
  "schema_version": 1,
  "id": "0c3e7a51-6b2f-4f0e-9a1d-2b7c8d9e0f12",
  "type": "plugin.test",
  "occurred_at": "2026-11-04T09:30:00.000Z",
  "source": { "app": "putio-sync", "version": "0.1.1", "host": "nas01" },
  "data": {
    "plugin": "ntfy",
    "message": "Test event from putio-sync"
  }
}
```
