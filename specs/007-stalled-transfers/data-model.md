# Data Model: Stalled Put.io Transfer Detection and Re-add

All state is in memory (research R4). Nothing is persisted.

## Transfer (client model, `putio-sync-client::models::Transfer`) — additions

| Field | Type | Notes |
|---|---|---|
| `magneturi` | `Option<String>` | Put.io field `magneturi` |
| `torrent_link` | `Option<String>` | kept for completeness, unused |
| `source` | `Option<String>` | may be a magnet link |
| `transfer_type` | `Option<String>` | Put.io field `type` (`TORRENT`, `URL`, …) |
| `status_message` | `Option<String>` | |
| `tracker_message` | `Option<String>` | |
| `current_ratio` | `Option<f64>` | now accepts number, numeric string or null (R9) |

Derived: `Transfer::magnet() -> Option<&str>` (R7).

## ProgressRecord (`putio-sync-core::stall`, private)

| Field | Type | Notes |
|---|---|---|
| `downloaded` | `i64` | last byte count seen |
| `changed_at` | `SystemTime` | when that value was first seen |
| `stall` | `Option<Stall>` | current verdict |

Keyed by transfer id in `StallTracker`.

## Stall (`putio-sync-core::stall`, public)

| Field | Type | Notes |
|---|---|---|
| `since` | `SystemTime` | `changed_at` of the record |
| `reason` | `StallReason` | `Availability(u8)`, `NoPeers`, `NoProgress` |

`StallReason::label()` → `"availability 92%"`, `"no peers"`, `"no progress"`.

## State transitions (per check, per transfer in the list)

```text
not tracked ──DOWNLOADING──▶ tracking(downloaded, now)
tracking ──downloaded changed──▶ tracking(new value, now), stall = None
tracking ──unchanged, now - changed_at ≥ threshold──▶ stall = Some(reason recomputed)
tracking ──status ≠ DOWNLOADING──▶ removed
any ──transfer absent from list──▶ removed
check fails (Put.io error) ──▶ no change
```

## StallTracker API (core)

- `StallTracker::new(threshold: Duration)`
- `observe(&self, transfers: &[Transfer], now: SystemTime) -> usize` — updates records,
  returns the stalled count.
- `get(&self, id: i64) -> Option<Stall>`
- `stalled_count(&self) -> usize`

## Config (`putio-sync-core::config`)

New `[transfers]` section, `TransfersConfig`:

| Key | Type | Default |
|---|---|---|
| `stall_threshold_hours` | `u64` | `6` |
| `stall_check_interval_secs` | `u64` | `600` |

Read when the daemon starts; changes apply on restart.
