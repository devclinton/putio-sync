# Data Model: Bandwidth Schedules

## Limit (core)

`Unlimited | Paused | Rate(u64 bytes/s)`. Written in config and JSON as `"unlimited"`,
`"paused"` or a positive integer. Ordered `Paused < Rate(a) < Rate(b) (a < b) < Unlimited`
so `min` gives the stricter one.

## ScheduleEntry (config, `[[download.bandwidth_schedule]]`)

| Field | Type | Rule |
|-------|------|------|
| `days` | string | items separated by commas: day, range, `daily`, `weekdays`, `weekends` |
| `start` | string `HH:MM` | `00:00`–`23:59` |
| `end` | string `HH:MM` | `00:01`–`24:00`, not equal to `start`; before `start` wraps past midnight |
| `limit` | Limit | required; a number must be > 0 |

At most 50 entries. Parsed into `Window { days: [bool; 7], start: u16, end: u16, limit }`
(minutes; Monday is index 0).

## Other `[download]` keys

- `bandwidth_limit_bytes_per_sec` (existing): the default outside every window; absent is
  unlimited; 0 refused.
- `slow_limit_bytes_per_sec` (new, default 1 048 576): the slow-down limit; 0 refused.

## Override (core, stored in `daemon_flags` key `bandwidth_override`)

`{ "mode": "pause" | "slow", "until": "<RFC 3339 UTC>" }`. Length 1–1440 minutes when
set. In force while `now < until`.

## Policy (core)

`{ default: Limit, windows: Vec<Window>, slow: u64 }`, from `[download]` leniently (bad
entries skipped; validation reports them).

- `at(local, utc, override) -> Current { limit, source }` where `source` is `default`,
  `schedule` (with the 1-based `entry`), `pause` or `slow`.
- `next_change(local, utc, override) -> Option<Change { at_local, limit, source }>`: the
  first boundary (window starts and ends over the next 8 days, and the override end) where
  the limit differs from now.

## BandwidthStatus (API and Status page)

| Field | Meaning |
|-------|---------|
| `limit` | Limit now |
| `bytes_per_sec` | the number, or `null` when unlimited, `0` when paused |
| `source`, `entry` | where it comes from |
| `override` | `{ mode, until }` or `null` |
| `next_change` | `{ at, limit, source }` (local time, `YYYY-MM-DDTHH:MM`) or `null` |
| `local_time` | daemon wall clock `YYYY-MM-DDTHH:MM:SS` |
| `utc_offset` | `+01:00` |
| `slow_limit_bytes_per_sec` | for the Slow button label |
