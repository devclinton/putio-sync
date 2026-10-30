# Contract: putio-sync API for Radarr/Sonarr settings (protected)

Every route below is under the protected router, behind the same web UI authentication as the
rest of Settings (FR-028).

- Errors use the existing shape, `{"error": "...", "details": "..."}`.
- No response ever contains the download-client password or an API key (FR-027).
- Request bodies that carry them are never logged.

`/transmission/rpc` (feature 011) is unchanged on the wire. It now reads `enabled` and
`password` per request, so changes apply without a restart.

## `GET /arr`: Settings card data (replaces feature 011's shape)

```json
{
  "client": {
    "enabled": true,
    "username": "putio-sync",
    "password_set": true,
    "env": {"password": false},
    "connection": {"implementation": "Transmission", "port": 15432, "ssl": false,
                   "url_base": "/transmission/", "hosts": ["putio-sync", "192.168.1.20"]},
    "download_dir": "/downloads"
  },
  "applications": [
    {"index": 0, "kind": "radarr", "name": "Radarr", "url": "http://radarr:7878",
     "host": "radarr", "port": 7878, "ssl": false, "url_base": "",
     "api_key_set": true, "api_key_env": false,
     "categories": ["radarr"], "client_present": true,
     "read_at": "2026-10-28T10:00:00Z", "read_error": null,
     "legacy_category": null,
     "path_check": {"status": "visible", "folder": "/downloads", "arr_path": "/downloads",
                    "mappings": [], "detail": null}}
  ]
}
```

- `connection.hosts` are the R5 candidate hosts, to copy by hand.
- When `legacy_category` is non-null and differs from `categories`, the UI shows the FR-015
  warning.
- `items` and `clients_seen` move to `GET /arr/status`.

## `GET /arr/status`: Status page card (FR-002)

```json
{"enabled": true,
 "clients_seen": [{"agent": "Sonarr", "last_contact": "2026-10-28T10:01:02Z"}],
 "items": {"putio": 1, "local": 0, "done": 3, "failed": 0},
 "held": [{"hash": "…", "name": "Show.S01E04", "since": "2026-10-28T10:00:00Z",
           "until": "2026-10-28T10:05:00Z", "attempts": 1, "last_lookup": "timeout"}]}
```

`held` lists the magnets held until a cache has their file list (feature 043), never the
magnet. The Queue page shows it.

## `PUT /arr/client`: Download client form (US3)

Request: `{"enabled": true, "username": "putio-sync", "password": "<new or empty>"}`

- An empty `password` keeps the current one (FR-004).
- A field whose value comes from env is ignored.
- 200 `{"message": "Saved. Applies now.", "password_changed": true}`. When `password_changed` is
  true, the UI shows the FR-008 warning and the update offer.
- 400 `{"error": "Set or generate a password first"}` when it's turned on with no password
  (FR-006).
- 500 `{"error": "Could not write the config file", "details": "..."}`. The running daemon is
  unchanged.

## Applications (US4)

The application body used by the routes below:
`{"kind": "radarr", "name": "Radarr", "host": "radarr", "port": 7878, "ssl": false, "url_base": "", "api_key": "<or empty>"}`

### `POST /arr/applications/test`

Tests unsaved form values. Send `"index": n` to use the saved key when `api_key` is empty.

- 200 `{"ok": true, "app": "Radarr", "version": "5.14.0.9383", "categories": ["radarr"], "client_present": true, "path_check": {…}}`
- 200 `{"ok": false, "reason": "<FR-026 reason>", "detail": "…"}`

### `POST /arr/applications/{index}/test`

Same as the route above, for a saved application. It also refreshes that application's
`ClientInfo`.

### `POST /arr/applications`

Adds an application.

- 201 `{"index": n, "message": "Saved. Applies now."}`
- 400 for an invalid URL, a duplicate URL, or a missing API key.

The UI only sends this after a passing test, or after the operator confirms (FR-012).

### `PUT /arr/applications/{index}`

Edits an application. An empty `api_key` keeps the saved key (US4 #4).

- 200 on success.
- 404 when the index doesn't exist.

### `DELETE /arr/applications/{index}`

Removes the application from the config only (FR-013). Returns 204, or 404 when the index
doesn't exist.

## Auto-setup (US1, US2, US5)

### `POST /arr/discover`

Runs discovery for at most 10 s.

```json
{"instances": [{"kind": "radarr", "url": "http://radarr:7878", "found_by": "container"}],
 "tried": ["http://radarr:7878", "http://radarr:8989", "…"]}
```

- 409 `{"error": "Discovery already running"}` when another discovery is running.

### `POST /arr/setup/preview`

Request: a SetupRequest (see [data-model.md](../data-model.md)).

- Checks the address and key, and reads the *arr's version and clients. Changes nothing.
- 200 SetupPlan.
- 200 `{"ok": false, "reason": "...", "detail": "..."}` for unreachable, unauthorized, not_arr,
  unsupported_version or invalid_category.

**Bare host** (US2 #3): a `url` without a scheme or port is tried as `http://host:7878`, then
`http://host:8989`. The first that answers as an *arr is used.

### `POST /arr/setup`

Request: a SetupRequest. It runs the flow in [data-model.md](../data-model.md) and returns 200
with a SetupResult, which may be a failed one.

- Saving the API key with "Remember" happens in the same call.
- **FR-008 password update**: the update offer calls this route with
  `{"target": {"application": n}}`.

## Removed

`POST /arr/servers/{index}/test` (feature 011). It's replaced by
`POST /arr/applications/{index}/test`. The web UI is its only caller.
