# Data Model: Hold Magnets and Ask the Cache Again (043)

## New table `held_magnets`

One row per magnet accepted but not yet sent to Put.io. Deleted when the hold ends, however it
ends.

```sql
CREATE TABLE IF NOT EXISTS held_magnets (
    hash TEXT PRIMARY KEY,                 -- info hash, lower-case hex
    magnet TEXT NOT NULL,                  -- the magnet to send to Put.io; never logged or returned
    source TEXT NOT NULL CHECK(source IN ('magnet', 'url', 'watch_magnet')),
    category TEXT,                         -- *arr category
    watch_folder TEXT,                     -- US4
    check_id INTEGER,                      -- release_checks.id of the pending check
    attempts INTEGER NOT NULL DEFAULT 1,   -- asks so far, the first included
    last_lookup TEXT,                      -- not_found | timeout | wrong_hash | error
    next_at TIMESTAMP NOT NULL,
    until_at TIMESTAMP NOT NULL,
    held_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS held_magnets_next ON held_magnets(next_at);
```

Validation: `until_at > held_at`; `next_at <= until_at` (the last ask is scheduled at
`until_at`, then the row is released).

## Changed use of existing tables (no schema change)

### `arr_items`

A held item is a row with:

- `transfer_id = NULL`, `state = 'putio'`, `putio_status = 'HELD'`;
- `total_size = 0` until a file list is found.

| From | Event | To |
|---|---|---|
| (new) | add, first ask misses, holding on | `putio`, `HELD`, no transfer |
| `HELD` | re-ask finds a passing list, or the hold ends, or precheck is off | `putio`, transfer set, Put.io status |
| `HELD` | re-ask finds a failing list | `failed`, blocklist `pending` (042 reject) |
| `HELD` | Put.io refuses the add | `failed`, error = Put.io's message |
| `HELD` | removed by the *arr | row deleted |

`follows_putio` skips the row while `transfer_id` is NULL, so the refresh loop leaves it alone.

### `release_checks`

The pending row inserted at add time (`stage = before`, `state = pending`, `transfer_id = NULL`)
is settled by the hold:

| Outcome | Update |
|---|---|
| found, passes | `state = passed`, `lookup = found`, `lookup_cache`, `total_size`, `transfer_id`, `resolved_at` |
| found, rejected | `state = rejected`, `reason`, `lookup = found`, `lookup_cache`, `total_size`, `resolved_at` |
| hold ran out | stays `pending`, `transfer_id` set, so 042 US3 checks it on Put.io |
| precheck turned off | `state = passed`, `reason = unchecked`, `transfer_id` |
| removed | `rejected`, `reason = gone` (042's `resolve_pending_by_hash`) |

`pending_checks` (042 US3) only acts on rows with a transfer, so held rows aren't touched by it.

## Settings (`[precheck]`, `PrecheckConfig`)

| Key | Default | Range | Meaning |
|---|---|---|---|
| `hold_magnet_secs` | 300 | 0..=3600 | how long a magnet is held; 0 = off |
| `hold_retry_secs` | 30 | 10..=600 | time between asks |
| `first_lookup_secs` | 3 | 1..=`lookup_timeout_secs` | the first ask's limit when holding is on |

No env overrides: 042 has them only for `enabled` and `magnet_lookup`, and these need none.
