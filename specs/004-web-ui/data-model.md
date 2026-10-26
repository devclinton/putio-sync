# Data Model: Web UI Served by the API

This feature adds no persisted data. The sync status and the auth limiter live in
memory; the new settings live in the existing TOML file; the session lives in the
browser. Route shapes are in [contracts/api.md](contracts/api.md).

## ApiConfig (existing `[api]`, extended)

| Field | Type | Default | Env override | Validation | Notes |
|---|---|---|---|---|---|
| `listen` | bool | `true` | `PUTIO_SYNC_API_LISTEN` | `true`/`false`/`1`/`0` | **new**. Whether `daemon` opens the API/UI listener. |
| `bind` | socket address | `0.0.0.0:15432` | `PUTIO_SYNC_API_BIND` | parses as `SocketAddr` | **new**. Ignored by `server`, whose `--bind`/`--port` flags win. |
| `auth_token` | string | unset | `PUTIO_SYNC_API_TOKEN` | non-empty when set, no whitespace | existing. Required for every protected route. |
| `enabled` | bool | `false` | — | — | existing, unchanged: daemon delegates downloads to a remote API at `url`. |
| `url` | string | unset | — | — | existing, unchanged. |

- Resolved once at startup into `Resolved<ApiSettings> { listen, bind, token:
  Option<String>, source }` by `Config::resolve_api()`, same shape as `resolve_metrics()`.
- Not live-reloaded; a change needs a restart.
- `Debug` for `ApiSettings` prints `token: <set>` or `<unset>`, never the value
  (Principle VI).

## SyncStatus (new, in `putio-sync-download`)

| Field | Type | Meaning |
|---|---|---|
| `in_progress` | bool | a `sync_downloads` call is running |
| `last_started_at` | Option<SystemTime> | when the last sync began |
| `last_finished_at` | Option<SystemTime> | when it ended |
| `last_duration` | Option<Duration> | how long it took |
| `last_outcome` | Option<SyncOutcome> | see below |
| `next_due_at` | Option<SystemTime> | when the loop will run next; `None` when no loop is running (dev `server` mode) |
| `interval` | Option<Duration> | the loop interval; `None` without a loop |

`SystemTime` rather than `chrono` so `putio-sync-download` gains no dependency; the API
converts to RFC 3339.

### SyncOutcome

- `Success { found, queued, skipped, blocked, failed }` — mapped from `SyncStats`
  (`total_files`, `files_downloaded`, `files_skipped`, `files_blocked`, `files_failed`).
- `Failure { error: String }` — the error chain text as logged by the loop. It never
  contains the token: the client already redacts it in `Display`.

### Transitions

```text
Idle ──request_sync() or interval elapsed──▶ InProgress ──Ok──▶ Idle (last_outcome = Success, next_due_at = now + interval)
                                                        └──Err─▶ Idle (last_outcome = Failure, next_due_at = now + interval)
InProgress ──request_sync()──▶ Err(SyncRequestError::InProgress), state unchanged
```

`request_sync()` while Idle with a loop running: wakes the loop (`Notify`), returns
`Ok(())`; the loop then transitions as above. Without a loop: spawns one `sync_downloads`
and returns `Ok(())`.

## StatusSummary (response of `GET /status`, composed in `api`)

Every section is `null` when its source is unavailable, and `unavailable` lists why.

| Section | Fields | Source |
|---|---|---|
| `build` | `version`, `commit` | `CARGO_PKG_VERSION`, `PUTIO_SYNC_GIT_SHA` or `"unknown"` |
| `uptime_seconds` | u64 | `AppState.started_at` |
| `sync` | SyncStatus fields with ISO-8601 times and seconds | `download_manager.sync_status()` |
| `queue` | `pending`, `downloading`, `completed`, `failed`, `paused`, `skipped` | `db.queue().counts_by_status()` |
| `downloads` | list of `{file_id, filename, file_size, bytes_downloaded, percentage, speed_bps, eta_seconds, elapsed_seconds}` | `get_active_downloads()` |
| `workers` | `busy`, `configured` | `active_workers()`, config `download.workers` |
| `watcher` | `configured`, `paths`, `files_processed` | `file_watcher.get_stats()` or config |
| `disk` | `free_bytes`, `total_bytes` | `disk_usage(download.target_directory)` |
| `reachability` | `database: ok\|unavailable`, `putio: ok\|failed\|unknown` | counts query result; last sync outcome |
| `auth` | `putio: token\|app_password\|none`, `api_token: true` | config, values never included |
| `unavailable` | list of `{section, reason}` | built while composing |

## Operator session (browser only)

| Item | Where | Rule |
|---|---|---|
| `putio-sync.token` | `localStorage` | set after a successful `POST /auth/login`; cleared on sign-out or on any 401 |
| `putio-sync.page` | `localStorage` | last hash route, restored on load (convenience) |

No server-side session state exists. The token is the session.

## Failed-auth limiter (in memory, `api`)

| Field | Type | Rule |
|---|---|---|
| key | `IpAddr` | from `ConnectInfo<SocketAddr>`; `127.0.0.1` when absent |
| `window_start` | `Instant` | reset when older than 60 s |
| `failures` | u32 | incremented on a failed `/auth/login` or a wrong token on a protected route (not on a missing token, nor while no token is configured) |

- Limit: 5 failures per 60 s window. At the 6th and later, the response is 429 with
  `Retry-After: <seconds left in the window>` before the token is even compared.
- Entries older than 60 s are pruned on each insert; the map never grows past the number
  of distinct client IPs seen in the last minute.
- Logged at WARN: `Authentication failed ip=… failures=…`; at the limit: `Authentication
  rate limited ip=…`. Never the attempted value.

## Existing entities reused unchanged

Queue item (`QueueItemResponse`), active download (`DownloadProgressResponse`), worker
(`WorkerStatsResponse`), Put.io file and transfer (client models), history entry
(`HistoryEntry`), settings groups (`DownloadSettings`, `WatcherConfig`,
`LoggingSettings`, `NotificationSettings`). Their shapes are already in
`putio-sync-api/src/routes/**` and are not changed by this feature, except that
`HistoryQuery` gains `name`.
