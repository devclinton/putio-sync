# Contract: HTTP API changes

Only routes that this feature adds or changes are listed. Everything else in
`putio-sync-api/src/routes/**` keeps its current shape. Error bodies keep the existing
`{"error": "...", "details": "..."?}` form (`putio-sync-api/src/error.rs`).

## Route classes

| Class | Routes | Auth |
|---|---|---|
| Public | `GET /health`, `GET /`, `GET /assets/{file}`, `GET /auth/status`, `POST /auth/login` | none |
| Protected | everything else | `Authorization: Bearer <api.auth_token>` (or the raw token in the header, as today) |

- The router mounts the protected routes under the auth middleware and merges the public
  routes after it (`/health` already works this way).
- The legacy `/api/*` placeholder routes are removed.
- The `CorsLayer` is removed. No CORS headers are sent. Same-origin page requests and
  non-browser clients are unaffected.

## Authentication responses

| Situation | Status | Body | Headers |
|---|---|---|---|
| `api.auth_token` not configured, protected route | 401 | `{"error":"API token not configured","details":"Set [api] auth_token in the config file or PUTIO_SYNC_API_TOKEN"}` | — |
| Missing or wrong token, protected route | 401 | `{"error":"Unauthorized"}` | `WWW-Authenticate: Bearer` |
| 6th or later failure from one IP within 60 s | 429 | `{"error":"Too many failed authentication attempts"}` | `Retry-After: <seconds>` |

The limiter counts a failure only for a real failed authentication: a protected request
that presents a token (`Authorization` header with a non-empty value) that doesn't match,
or a `POST /auth/login` whose token doesn't match. A request with no token at all still
gets the 401 but is not counted, so a browser asking for paths nobody links to
(`/robots.txt`, touch icons) cannot lock its operator out. Nor is anything counted while
no token is configured (there is nothing to guess). Failures are keyed by the client IP from `ConnectInfo<SocketAddr>` (`127.0.0.1` when the router is
driven without connect info, as in `oneshot` tests). A successful request does not reset
the window. The 429 is returned before the token is compared.

Log lines (WARN, structured, never the attempted value):

```text
Authentication failed ip=192.168.1.20 failures=3
Authentication rate limited ip=192.168.1.20 retry_after_seconds=41
```

## `GET /auth/status` (new, public)

```json
{ "token_configured": true }
```

## `POST /auth/login` (new, public)

Request: `{ "token": "..." }`. Response: `204 No Content` on match, `401` with
`{"error":"Unauthorized"}` otherwise (counted by the limiter), `401` with the "not configured" body when no token
is set, `429` as above. The handler uses the same constant-time compare as the middleware.
The request body is never logged.

## `GET /status` (new, protected)

One request that backs the status page (FR-010). All times are RFC 3339 UTC strings;
durations are seconds. A section that cannot be produced is `null` and is named in
`unavailable`.

```json
{
  "build": { "version": "0.4.0", "commit": "f96ffd9" },
  "uptime_seconds": 86400,
  "sync": {
    "in_progress": false,
    "last_started_at": "2026-10-25T10:00:00Z",
    "last_finished_at": "2026-10-25T10:00:04Z",
    "last_duration_seconds": 4.2,
    "last_outcome": { "result": "success", "found": 12, "queued": 2, "skipped": 10, "blocked": 0, "failed": 0 },
    "next_due_at": "2026-10-25T10:01:00Z",
    "interval_seconds": 60
  },
  "queue": { "pending": 2, "downloading": 1, "completed": 40, "failed": 0, "paused": 0, "skipped": 1 },
  "downloads": [
    { "file_id": 123, "filename": "a.mkv", "file_size": 1000, "bytes_downloaded": 500,
      "percentage": 50.0, "speed_bps": 1048576, "eta_seconds": 10, "elapsed_seconds": 10 }
  ],
  "workers": { "busy": 1, "configured": 4 },
  "watcher": { "configured": true, "paths": ["/watch"], "files_processed": 3 },
  "disk": { "free_bytes": 1000000000, "total_bytes": 4000000000 },
  "reachability": { "database": "ok", "putio": "ok" },
  "auth": { "putio": "token", "api_token": true },
  "unavailable": []
}
```

- `sync.last_outcome` is `{"result":"failure","error":"..."}` after a failed sync, or
  `null` before the first one. `next_due_at` and `interval_seconds` are `null` when no
  sync loop runs (dev `server` mode).
- `reachability.putio` is `ok` when the last sync succeeded, `failed` when it failed,
  `unknown` before the first sync. No Put.io request is made by this route.
- `reachability.database` is `unavailable` when the state has no database or the counts
  query fails; `queue` is then `null` and `unavailable` holds
  `{"section":"queue","reason":"database unavailable"}`.
- `auth.putio` is `token`, `app_password` or `none`; `auth.api_token` is always `true`
  (the request was authenticated). No values.
- `disk` is `null` with reason `"statvfs failed: <os error>"` when the download folder
  cannot be inspected.
- Target: under 200 ms with a 1,000-row queue.

## `POST /local/sync` (new, protected)

Starts a folder sync now (FR-013).

| Result | Status | Body |
|---|---|---|
| Sync requested (loop woken, or run spawned in `server` mode) | 202 | the `sync` object from `/status` with `in_progress: true` |
| A sync is already running | 409 | `{"error":"A sync is already in progress"}` |
| No database (cannot sync) | 503 | `{"error":"Database unavailable"}` |

## `POST /local/queue` (new, protected)

Queues a Put.io file, or every file under a Put.io folder, through the same path the
folder sync uses: blocked-extension and pattern filters apply, already-downloaded and
already-queued files are skipped (FR-014). Unlike `POST /local/downloads` (unchanged,
downloads one file at once outside the queue), this is the UI's "download" action.

Request: `{ "file_id": 123 }`.

| Result | Status | Body |
|---|---|---|
| Enumerated and queued | 202 | `{ "found": 5, "queued": 3, "skipped": 2, "blocked": 0, "failed": 0 }` |
| Put.io file not found | 404 | `{"error":"File not found"}` |
| No database | 503 | `{"error":"Database unavailable"}` |

## `GET /history` (changed, protected)

Query gains `name` (optional, case-insensitive substring, `%`, `_` and `\` are escaped so
they match literally). It combines with `action`, `limit` and `offset`. `total` in the
response counts the filtered rows. `GET /history/stats` is unchanged.

```text
GET /history?action=downloaded&name=ubuntu&limit=50&offset=0
```

## `DELETE /putio/files/{id}` (changed, protected)

Before calling Put.io, the handler looks the id up in the queue. If the row's status is
`pending`, `downloading` or `paused`, it answers 409:

```json
{ "error": "File is queued or downloading", "details": "Skip or cancel it first" }
```

Other statuses, or no row, proceed as today. When the state has no database the guard is
skipped (dev mode).

## `GET /stats/disk` (changed, protected)

Reports the download folder's filesystem (statvfs through `putio-sync-telemetry`) rather
than the root filesystem. Shape unchanged.

## `GET /` and `GET /assets/{file}` (new, public)

| Request | Response |
|---|---|
| `GET /` | 200, `text/html; charset=utf-8`, the embedded `index.html` |
| `GET /assets/app.css`, `/assets/app.js`, `/assets/api.js`, `/assets/format.js`, `/assets/pages/<page>.js` | 200 with `text/css` or `text/javascript; charset=utf-8` |
| unknown asset | 404 (plain) |

Headers on every asset: `Cache-Control: no-cache`, `ETag: "<crate version>"`; a request
with a matching `If-None-Match` gets 304. The assets never include the token. The index
page does not use inline scripts, so a later CSP can be strict.

## Removed

- `/api/*` legacy placeholder routes.
- Open mode (requests allowed when no token is configured).
