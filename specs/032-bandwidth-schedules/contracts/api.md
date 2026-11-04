# Contract: HTTP API

All routes need the API token like every protected route and answer 503 in setup mode.

## GET /bandwidth

```json
{
  "limit": 5242880,
  "bytes_per_sec": 5242880,
  "source": "default",
  "entry": null,
  "override": null,
  "next_change": { "at": "2026-11-05T01:00", "limit": "unlimited", "source": "schedule", "entry": 1 },
  "local_time": "2026-11-04T08:00:12",
  "utc_offset": "+01:00",
  "slow_limit_bytes_per_sec": 1048576
}
```

`limit` is `"unlimited"`, `"paused"` or a number. `source`: `default`, `schedule`, `pause`,
`slow`. `entry` is 1-based when `source` is `schedule`.

## PUT /bandwidth/override

Body `{ "mode": "pause" | "slow", "minutes": 60 }` (`minutes` 1–1440, default 60).
Answers the new `GET /bandwidth` body. 400 for a bad mode or length. Replaces any
override in force. Persisted when the daemon has a database.

## DELETE /bandwidth/override

Ends the override; answers the `GET /bandwidth` body (200 even when there was none).

## GET /status

Gains `"bandwidth": <GET /bandwidth body>`.

## GET /local/workers

`workers[].bandwidth_limit_bps` is `null`; `total_bandwidth_limit_bps` is the current
limit (`null` unlimited, `0` paused).

## GET / PUT /settings/download

Gain `bandwidth_schedule` (array of `{ days, start, end, limit }`) and
`slow_limit_bytes_per_sec`. Both are live: the next second of every running download.

## Metrics

- `putio_sync_download_bandwidth_limit_bytes_per_second` gauge: `+Inf` unlimited, `0`
  paused.
- `putio_sync_download_bandwidth_override` gauge: 1 while a pause or slow-down is in force.
