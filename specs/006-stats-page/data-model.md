# Data Model: Stats Page

## Stored

### `download_history` (existing, unchanged)

| Column | Use here |
|---|---|
| `file_id` | joins to `download_sources` |
| `extension` | type breakdown and filter (lower case, `NULL` → `(none)`) |
| `file_size` | bytes |
| `action` | only `downloaded` rows count |
| `timestamp` | finish time, UTC, `YYYY-MM-DD HH:MM:SS` |
| `duration_seconds` | start = finish − duration (R2); `NULL`/0 → all bytes at finish |

### `download_sources` (new)

```sql
CREATE TABLE IF NOT EXISTS download_sources (
    file_id INTEGER PRIMARY KEY,           -- Put.io file id, as in download_history
    transfer_id INTEGER,                   -- Put.io transfer it was matched to; NULL if none
    source_type TEXT CHECK(source_type IN ('torrent', 'url', 'other')), -- NULL if no transfer
    tracker_host TEXT,                     -- host name only, lower case; NULL if none
    is_private INTEGER NOT NULL DEFAULT 0, -- 1 for private torrents
    origin TEXT NOT NULL CHECK(origin IN ('watch', 'manual', 'putio', 'unknown')),
    watch_folder TEXT,                     -- configured watch directory, for origin 'watch'
    putio_folder TEXT,                     -- first segment of the Put.io path, '(root)' at the root
    recorded_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
)
```

- Written by `enqueue_tree` for every file it adds to the queue (R4, R11). Re-queueing the
  same file updates the row.
- `tracker_host` holds no scheme, port, path, query or user info (R5).
- No row → source "unknown".

### `transfer_origins` (new)

```sql
CREATE TABLE IF NOT EXISTS transfer_origins (
    transfer_id INTEGER PRIMARY KEY,
    origin TEXT NOT NULL CHECK(origin IN ('watch', 'manual')),
    watch_folder TEXT,
    recorded_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
)
```

Written by the watcher after an upload and by `POST /putio/transfers` (R11).

## In memory

### `DownloadSource` (`putio-sync-db::models`)

`{ transfer_id: Option<i64>, source_type: Option<SourceType>, tracker_host: Option<String>,
is_private: bool, origin: Origin, watch_folder: Option<String>, putio_folder: Option<String> }`
with `SourceType::{Torrent, Url, Other}` and `Origin::{Watch, Manual, Putio, Unknown}`; built from a Put.io `Transfer` in
`putio-sync-download`: `TORRENT` → torrent, `URL` → url, anything else → other.

### `Transfer` (`putio-sync-client::models`, changed)

Gains:
- `transfer_type: Option<String>` from `type`;
- `tracker_host: Option<String>` from `tracker`, host only (R5);
- `magnet_tracker_host: Option<String>` from `magneturi`, host of the first `tr=` only,
  never serialised.

### `StatsQuery` (`putio-sync-db::analytics`)

| Field | Type | From |
|---|---|---|
| `since` | `Option<DateTime<Utc>>` | `range` (none for `all`) |
| `until` | `DateTime<Utc>` | now |
| `tz_offset_minutes` | `i32`, −840..=840 | `tz` |
| `category` | `Option<Category>` | `category` |
| `extension` | `Option<String>` | `ext` (`(none)` matches `NULL`) |
| `source` | `Option<SourceFilter>` | `source`: torrent, url, other, unknown |
| `tracker` | `Option<String>` | `tracker` |
| `private` | `Option<bool>` | `private` |
| `origin` | `Option<Origin>` | `origin`: watch, manual, putio, unknown |
| `watch_folder` | `Option<String>` | `watch` |
| `putio_folder` | `Option<String>` | `folder` |

### `Category`

`Video, Audio, Archive, Image, Document, Subtitle, Other`, from a fixed extension table
(spec Assumptions); unknown and missing extensions are `Other`.

### `BucketSize`

`Hour (3600), SixHours (21600), Day (86400), Week (604800), FourWeeks (2419200)`; the
smallest with ≤ 120 buckets over `[since, until]` (R6). Buckets align to local time:
hours to the hour, six hours to 00/06/12/18, days to midnight, weeks and four-week blocks
to Monday 00:00 (four-week blocks count from the Monday on or before `since`).

### `StatsSummary` (response)

See [contracts/api.md](contracts/api.md). Sections: `range`, `totals`, `series`,
`heatmap`, `types`, `sources`, `options`.

## Validation

- `range` ∈ {`24h`, `7d`, `30d`, `90d`, `1y`, `all`}; default `7d`.
- `tz` integer within ±840; default 0.
- `category` ∈ the categories above (lower case); `source` ∈ {torrent, url, other,
  unknown}; `private` ∈ {true, false}. Anything else → 400 with a message naming the
  parameter.
- `origin` ∈ {watch, manual, putio, unknown}.
- `ext` and `tracker`: at most 64 characters, lower-cased before comparing. `watch` and
  `folder`: at most 512 characters, compared exactly.
