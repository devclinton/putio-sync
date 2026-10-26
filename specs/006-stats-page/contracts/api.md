# API Contract: Stats

## `GET /stats/summary` (new, token required)

Everything the Stats page draws, pre-aggregated for one range and filter set.

### Query

| Param | Values | Default |
|---|---|---|
| `range` | `24h`, `7d`, `30d`, `90d`, `1y`, `all` | `7d` |
| `tz` | minutes east of UTC, −840..840 | `0` |
| `category` | `video`, `audio`, `archive`, `image`, `document`, `subtitle`, `other` | none |
| `ext` | an extension, or `(none)`; overrides `category` | none |
| `source` | `torrent`, `url`, `other`, `unknown` | none |
| `tracker` | a tracker host name | none |
| `private` | `true`, `false` | none |
| `origin` | `watch`, `manual`, `putio`, `unknown` | none |
| `watch` | a watch folder path (as in `options.watch_folders`) | none |
| `folder` | a top-level Put.io folder name, or `(root)` | none |

Empty values are the same as absent.

### 200

```json
{
  "range": {
    "range": "7d",
    "since": 1761004800,
    "until": 1761609600,
    "bucket": "6h",
    "bucket_seconds": 21600,
    "tz": -60
  },
  "totals": { "files": 42, "bytes": 123456789012, "seconds": 9876, "avg_bps": 12500000 },
  "series": [ { "start": 1761004800, "bytes": 1234567, "files": 2 } ],
  "heatmap": {
    "files": [[0, 0, 1, "… 24 per row"], "… 7 rows, Monday first"],
    "bytes": [[0, 0, 1048576, "…"], "…"]
  },
  "types": [
    {
      "category": "video", "files": 30, "bytes": 100000000000,
      "extensions": [ { "ext": "mkv", "files": 25, "bytes": 90000000000 } ]
    }
  ],
  "sources": {
    "types":    [ { "key": "torrent", "files": 30, "bytes": 1 } ],
    "trackers": [ { "key": "tracker.example.org", "files": 12, "bytes": 1 } ],
    "privacy":  [ { "key": "private", "files": 5, "bytes": 1 } ],
    "origins":  [ { "key": "watch", "files": 20, "bytes": 1 } ],
    "watch_folders": [ { "key": "/watch/tv", "files": 12, "bytes": 1 } ],
    "folders":  [ { "key": "Movies", "files": 9, "bytes": 1 } ]
  },
  "options": {
    "extensions": ["mkv", "mp4", "srt", "(none)"],
    "trackers": ["tracker.example.org"],
    "watch_folders": ["/watch/tv"],
    "folders": ["Movies", "TV", "(root)"]
  }
}
```

- Times are UTC epoch seconds. `since` is `null` for `all` with no downloads.
- `bucket` is `1h`, `6h`, `1d`, `1w` or `4w`; `series` has one entry per bucket from the
  bucket holding `since` to the one holding `until`, including empty ones, oldest first.
- `totals.seconds` is the sum of durations; `avg_bps` is the bytes of downloads that have a duration over `seconds` (0 when
  `seconds` is 0).
- `heatmap` cells use local time per `tz`; row 0 is Monday, column 0 is 00:00–00:59.
- `types` is sorted by bytes descending; each category's `extensions` by bytes, at most 8,
  then one `{ "ext": "…other", … }` entry for the rest.
- `sources.types` keys: `torrent`, `url`, `other`, `unknown`. `trackers`: top 10 hosts by
  bytes, then `other trackers` for the rest, then `no tracker` for torrents without a
  host. `privacy` keys: `private`, `public` (torrents only), `unknown` (no source).
  `origins` keys: `watch`, `manual`, `putio`, `unknown` (no row counts as `unknown`).
  `watch_folders`: per watch folder, for origin `watch`. `folders`: top 10 Put.io folders
  by bytes, then `other folders`, then `unknown` for rows without a folder.
  Groups with zero files are omitted.
- `options` lists every extension, tracker host, watch folder and Put.io folder in the range **before** the type and
  source filters, so the filter menus don't shrink to the current selection. Extensions
  by file count, trackers by name.
- Totals, series, heatmap, types and sources honour all filters.

### Errors

- 400 `{"error": "Invalid query", "details": "range must be one of 24h, 7d, 30d, 90d, 1y, all"}`
  (and likewise for `tz`, `category`, `source`, `private`, over-long `ext`/`tracker`).
- 401 without a valid token.
- 500 `{"error": "Database not available"}` when the API runs without a database.

## `GET /putio/transfers` (changed)

Each transfer gains `transfer_type` (Put.io `type`, e.g. `TORRENT`) and `tracker_host`
(host only, or `null`). The full tracker URL, magnet URI and source are never returned.

## `POST /putio/transfers` (changed)

On success, also records `transfer_origins(transfer_id, 'manual')`. A failed record is
logged and does not fail the request.
