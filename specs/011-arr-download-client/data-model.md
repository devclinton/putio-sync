# Data Model: Radarr/Sonarr Download Client (011)

## Table `arr_items` (new, SQLite)

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK AUTOINCREMENT | The Transmission `id` the *arr apps see |
| `hash` | TEXT NOT NULL UNIQUE | Info hash, 40 lowercase hex characters |
| `name` | TEXT NOT NULL | Display name: the transfer name first, then the Put.io file name once known |
| `category` | TEXT NOT NULL | Relative folder path below the reported download folder (`sonarr`, `movies/4k`, or `''`) |
| `transfer_id` | INTEGER | Put.io transfer id; NULL for items rejected before Put.io (042) |
| `file_id` | INTEGER | Put.io file or folder the transfer produced, once complete |
| `output_name` | TEXT | Name of that file or folder on disk (`<target>/<category>/<output_name>`) |
| `total_size` | INTEGER NOT NULL DEFAULT 0 | Bytes: from Put.io while downloading there, then the sum of accepted files |
| `putio_status` | TEXT | Last Put.io status seen (`IN_QUEUE`, `DOWNLOADING`, `COMPLETED`, `SEEDING`, `ERROR`, `GONE`) |
| `putio_percent` | INTEGER NOT NULL DEFAULT 0 | 0–100 |
| `putio_eta` | INTEGER | Seconds, from Put.io |
| `state` | TEXT NOT NULL CHECK IN (`putio`,`local`,`done`,`failed`) | See the state machine below |
| `error` | TEXT | Message reported as `errorString` |
| `blocklist` | TEXT NOT NULL DEFAULT `none` CHECK IN (`none`,`pending`,`done`,`gave_up`) | *arr blocklist call state |
| `blocklist_attempts` | INTEGER NOT NULL DEFAULT 0 | Retry limit: 5 |
| `added_at` | TIMESTAMP DEFAULT CURRENT_TIMESTAMP | |
| `updated_at` | TIMESTAMP DEFAULT CURRENT_TIMESTAMP | |

## Table `arr_item_files` (new)

| Column | Type | Notes |
|---|---|---|
| `hash` | TEXT NOT NULL | → `arr_items.hash`; rows are deleted along with the item |
| `file_id` | INTEGER NOT NULL | Put.io file accepted by the filters |
| `size` | INTEGER NOT NULL | |
| PK | (`hash`, `file_id`) | |

Local progress is the sum of `size` over files whose `downloads` row is `completed`, plus
`bytes_downloaded` of any file in `DownloadManager::get_active_downloads()`.

## State machine

```text
            add
             │
             ▼
  ┌────── putio ──────┐  Put.io ERROR, or transfer missing   ┌────────┐
  │  (IN_QUEUE /      ├─────────────────────────────────────▶│ failed │
  │   DOWNLOADING)    │                                      └────────┘
  └────────┬──────────┘                                           ▲
           │ COMPLETED/SEEDING + file_id                          │ no accepted files
           ▼                                                      │
         local ───────────────────────────────────────────────────┘
           │ every accepted file completed
           ▼
          done
```

- `failed` is final unless the Put.io transfer recovers (for example after a retry from the UI).
  An item in `ERROR` that goes back to `DOWNLOADING` returns to `putio` and clears `error`.
- `torrent-remove` deletes the item from any state.
- When an item enters `failed` and an *arr server is configured for its category, `blocklist`
  goes `none → pending`. A refresh then tries the call: success → `done`, not found in the *arr
  queue → `done` (nothing to do), error → `pending` with `attempts + 1`, and after 5 attempts
  → `gave_up`.

Known limitation: a local download that fails for good (its queue row reaches `failed`) leaves
the item in `local`. The *arr sees it stuck downloading and can remove it.

## Transmission view of an item (computed, not stored)

| Field | Value |
|---|---|
| `id` | `rowid` of the item (stable while tracked) |
| `hashString` | `hash` |
| `name` | `name` (once known, equal to `output_name`) |
| `downloadDir` | `<target>` + `/` + `category` (no trailing slash; just `<target>` when the category is empty) |
| `totalSize` | `total_size` |
| `leftUntilDone` | `putio`: `total_size × (100 − putio_percent)/100`, plus the local half (see below); `local`: `total_size − local bytes`; `done`: 0; `failed`: `total_size` |
| `percentDone` | `1 − leftUntilDone/totalSize` (0 when `totalSize` is 0) |
| `status` | `putio` + `IN_QUEUE` → 3; `putio`/`local` → 4; `done`/`failed` → 0 |
| `isFinished` | `state == done` |
| `errorString` | `error` when `state == failed`, else `""` |
| `error` | 3 when failed, else 0 |
| `eta` | `putio_eta` while on Put.io; while local, from bytes left and the current speed; else −1 |
| `labels` | `[]` |
| `seedRatioMode`, `seedIdleMode` | 0 |
| `seedRatioLimit`, `seedIdleLimit` | 0 |
| `uploadedEver`, `downloadedEver`, `secondsSeeding`, `secondsDownloading` | 0, local bytes, 0, 0 |
| `fileCount` | number of `arr_item_files` rows |

"Plus the local half": progress is split into a Put.io half and a local half, so the *arr never
sees 100% before the files are on disk. While on Put.io, `leftUntilDone` is
`total_size × (1 − 0.5 × putio_percent/100)`. While local, it is
`total_size × 0.5 × (1 − local/total)`.

## Config (`putio-sync-core`)

```text
[arr]
enabled: bool = false
username: String = "putio-sync"
password: Option<String>          # secret; env PUTIO_SYNC_ARR_PASSWORD
servers: Vec<ArrServer> = []

[[arr.servers]]
kind: "radarr" | "sonarr"
url: String                        # http://host:port[/base]
api_key: Option<String>            # secret; env PUTIO_SYNC_RADARR_API_KEY / PUTIO_SYNC_SONARR_API_KEY (first of that kind)
category: String                   # relative folder path; matches arr_items.category
```

Validation:

- `enabled` with no password: warn at startup and keep the RPC refusing requests (503
  "download client has no password").
- A server's `url` must be http(s).
- Two servers must not share a category.
- A category may not contain `..` or empty segments, and is normalised without leading or
  trailing `/`.
