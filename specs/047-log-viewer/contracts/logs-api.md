# Contract: /logs (047)

All routes are protected by the same middleware as the rest of the API: a bearer token or
Tailscale identity. Without either they answer 401. In setup mode they answer 503.

## GET /logs

Query (all optional): `after` (seq), `level` (`error|warn|info|debug|trace`), `target`,
`q`, `limit` (1..=capacity).

200:
```json
{
  "lines": [LogLine, …],          // oldest first; see data-model.md
  "last_seq": 1234,               // newest seq kept (0 when empty)
  "oldest_seq": 235,              // oldest seq kept (0 when empty)
  "capacity": 2000,
  "evicted": 17,
  "targets": ["putio_sync_core::config", "putio_sync_hooks::service"]   // distinct, sorted
}
```

400: an invalid `level`.

## GET /logs/stream

Query: `after` (seq; default `last_seq`, so only new lines are sent), plus the same `level`,
`target` and `q` filters.

Response: `text/event-stream`. Events:
- `event: line` with `data: <LogLine JSON>`, for each new matching line, in seq order.
- `event: skipped` with `data: {"count": N}`. N lines after the last sent seq are no longer
  kept (the viewer fell behind or reconnected too late).
- a comment keep-alive every 15 s.

Lines between `after` and the newest kept line are sent first, so nothing is missed or sent
twice.

## GET /logs/level

200: `LevelStatus`
```json
{
  "base": "info",
  "source": "config",              // "config" | "env"
  "effective": "info,putio_sync_hooks=debug",
  "global": null,                  // or {"level": "debug", "until": "2026-11-05T12:30:00Z" | null}
  "modules": [{"target": "putio_sync_hooks", "level": "debug", "until": "…" }],
  "verbose": true,
  "available": true                // false when logging was not set up with a reload handle
}
```

## PUT /logs/level

Body: `{"target": null | "putio_sync_hooks", "level": "debug", "minutes": 30 | null}`.
`minutes: null` means keep until restart.

- 200: the new `LevelStatus`.
- 400: an invalid level, target or minutes, or a 21st module (the message says which).
  The level in effect is unchanged.
- 409: logging has no reload handle (`available: false`).

## DELETE /logs/level

Query: `target` (optional). With a module, that module's override is cleared; with
`target=` (empty), the overall override; without it, every override is cleared.

200: the new `LevelStatus`.

## Settings

`GET/PUT /settings/logging` gains `buffer_lines` (integer, 100..=50000, default 2000). A PUT
outside the range answers 400. A saved change resizes the buffer at once.

## Metrics

`putio_sync_log_lines_dropped_total{reason="evicted"|"lagged"}` (counter).

## Doctor

Check id `log_level`, title "Log level", area `local`:
- `ok` when nothing is at debug or trace;
- `warning` otherwise, with the effective directive, its source and any override end time.
