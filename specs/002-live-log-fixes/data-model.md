# Data Model: Fixes from the Live NAS Logs

No schema migrations are needed. These are the entities whose behaviour changes.

## Queue item (`download_queue`)

```
pending ──claim (atomic, one worker)──▶ downloading ──▶ completed | failed
   ▲                                                       │
   └──────────────── retry / requeue ──────────────────────┘
```

- **Claim**: one `UPDATE … WHERE file_id = (SELECT … LIMIT 1) AND status='pending' RETURNING *`.
  - It returns either the claimed row or nothing.
  - Under contention it waits up to the busy timeout (10 s) instead of erroring.
- **Invariant**: a row moves pending→downloading at most once per claim, and no two workers
  receive the same `file_id`.
- **Store mode**: WAL journal, `synchronous=NORMAL`, `busy_timeout=10s`. These are
  connection-level settings, not schema.

## Processed watch record (`processed_torrents`)

- Keyed by the SHA-256 of the magnet text or the torrent bytes. Unchanged.
- **New meaning**: a match now means "already uploaded, so apply the post-upload action".
  It no longer means "skip".

### Watch file processing outcome (in memory)

| Outcome | Upload? | Post-upload action? | Log |
|---------|---------|---------------------|-----|
| `Uploaded` | yes | yes | INFO |
| `AlreadyUploaded` | no | yes | INFO when it cleans up, otherwise DEBUG |
| `Invalid(reason)` | no | no | WARN once per path per run |
| `Gone` / `Unsupported` | no | no | DEBUG |

`WatchStats` gains `cleaned_up: usize`. A scan is "idle" when processed + cleaned_up +
failed = 0.

## Blocked-extension list (`download.blocked_extensions`)

- A list of extension strings without the dot, compared case-insensitively against the
  file name's final extension.
- **Default**: `exe msi bat cmd com scr pif vbs js jar ps1 lnk apk dmg pkg`.
  - A missing key gives the default.
  - `[]` disables the guard.
  - A configured list replaces the default; it is not merged with it.
- `SyncStats` gains `files_blocked: usize`.
- Warn-once state: a set of Put.io file ids per `DownloadManager` lifetime. It is reset on
  restart.

## Daemon config

- New field `download_interval: u64`, in seconds, default 300. It is set from
  `--download-interval` and drives the Put.io sync loop.
- `watch_interval` keeps driving the watcher and the worker loop.
