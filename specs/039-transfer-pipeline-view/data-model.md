# Data Model: Active Transfers Pipeline View

## Stored (SQLite, new tables)

### `pipeline_holds`: transfers paused while still on Put.io (R9)

| Column | Type | Notes |
|---|---|---|
| `transfer_id` | INTEGER PRIMARY KEY | Put.io transfer id |
| `name` | TEXT NOT NULL | for logs and the UI when Put.io is unreachable |
| `held_at` | TIMESTAMP DEFAULT CURRENT_TIMESTAMP | |

- Created by Pause on an item in the Put.io stage or Waiting for sync.
- Read on enqueue: a file whose transfer is held is inserted as `paused`.
- Deleted by Resume, or by the daemon loop when the transfer is in `ERROR` or gone from Put.io.

### `daemon_flags`: runtime switches that survive a restart (R11)

| Column | Type | Notes |
|---|---|---|
| `key` | TEXT PRIMARY KEY | only `nas_downloads_paused` for now |
| `value` | TEXT NOT NULL | `"1"` or `"0"` |
| `updated_at` | TIMESTAMP DEFAULT CURRENT_TIMESTAMP | |

A missing key means off.

### `pipeline_finished`: Recently finished (R12)

| Column | Type | Notes |
|---|---|---|
| `key` | TEXT PRIMARY KEY | item key (below) |
| `name` | TEXT NOT NULL | |
| `size` | INTEGER NOT NULL | bytes of the downloadable files |
| `outcome` | TEXT NOT NULL CHECK (`done`, `failed`) | |
| `reason` | TEXT | failure reason, for `failed` only |
| `added_at` | TIMESTAMP | `transfer.created_at`; NULL for items with no transfer |
| `putio_done_at` | TIMESTAMP | `transfer.finished_at` |
| `queued_at` | TIMESTAMP | earliest `download_sources.recorded_at` of its files |
| `done_at` | TIMESTAMP NOT NULL | when it became done, or when a failed item left the active list |

- Written once, on a transition (R6): when an item that was active on the loop's previous
  tick becomes done, or fails and can no longer be retried (spec FR-002). A failed item that
  can still be retried stays active and is not written. After a restart, done items are written
  only if their last download finished within the retention period. Old transfers still on
  Put.io are therefore never written, and a pruned row is never written again. A later write
  for the same key (for example a retried transfer that finishes again) replaces the row.
- Pruned when `done_at` is older than `[transfers] finished_retention_hours` (default 24).
- Stage times are derived from these columns: on Put.io = `putio_done_at - added_at`, waiting
  for sync = `queued_at - putio_done_at`, to the NAS = `done_at - queued_at`. A time is
  missing when either of its ends is NULL.

### Changes to existing data

- `download_queue`: no schema change. `QueueRepository::add` takes the initial status
  (`pending` or `paused`). `pause_files`, `resume_files` and `retry_files` are added.
  `pause_all` and `resume_all` are removed.
- `download_sources`: no change. It is the transfer-to-file link (R1).
- `downloads`: no change. A `completed` row marks a file as done, because the queue row is
  deleted on completion (R1).

A file of an item is **in the queue** (has a queue row), **done** (no queue row, `downloads`
row `completed`) or **skipped** (neither). Skipped files count toward neither `files.total`
nor `size`.

## In memory

### `PipelineItem` (built by `download::pipeline::build`)

| Field | Type | Notes |
|---|---|---|
| `key` | String | `transfer:<id>`, `folder:<relative_path>` or `file:<file_id>` |
| `name` | String | transfer name; else (no transfer, or transfer gone from Put.io) the top folder of its files' `relative_path`, or the file name (R6) |
| `transfer_id` | Option<i64> | |
| `stage` | Stage | `putio` or `nas` |
| `state` | State | see below |
| `held` | bool | a `pipeline_holds` row exists |
| `paused` | bool | its own pause or hold (not the switch) |
| `size` | i64 | Put.io size in the Put.io stage; total of its accepted files in the NAS stage |
| `done_bytes` | i64 | done files' sizes plus bytes downloaded of files in the queue |
| `percent` | f64 | 0–100 |
| `speed_bps` | Option<u64> | `down_speed` on Put.io; sum of its running files on the NAS |
| `eta_seconds` | Option<i64> | `estimated_time` on Put.io; remaining bytes ÷ speed on the NAS |
| `files` | Option<FileCounts> | NAS stage: done, downloading, queued, paused, failed, skipped, total (total leaves out skipped) |
| `putio_status` | Option<String> | raw Put.io status, for the note and unknown states |
| `seeding` | bool | |
| `stall` | Option<Stall> | from feature 007 |
| `origin` | Option<Origin> | `arr:<category>` from `arr_items.category` (by `transfer_id`); else `watch` or `manual` from `transfer_origins`; else `download_sources.origin` (`putio` when known, none for `unknown`) (FR-009) |
| `reason` | Option<String> | failure or error text |
| `next_sync_at` | Option<timestamp> | Waiting for sync only |
| `putio_as_of` | Option<timestamp> | when the Put.io figures were fetched (FR-029) |
| `note` | Option<String> | for example "Started again: the partial download was gone" (R8) |

### `Stage` and `State`

```text
Stage::Putio  → Queued | Downloading | Stalled | Finishing | Error
Stage::Nas    → WaitingForSync | Queued | Downloading | Paused | Failed
terminal      → Done | Failed (moves to pipeline_finished; never in the active list again)
```

Transitions:

```text
Putio(Queued) → Putio(Downloading) ⇄ Putio(Stalled) → Putio(Finishing)
             → Nas(WaitingForSync) → Nas(Queued) → Nas(Downloading) ⇄ Nas(Paused) → Done
Putio(*) → Putio(Error)       (Retry on the Transfers page goes back to Putio(Queued))
Nas(*)   → Nas(Failed)        (Retry on the Queue page goes back to Nas(Queued))
Putio(Error) → Failed         (transfer removed on Put.io)
Nas(*) with no file left in the queue:
             → Done           (at least one file done; note "N files skipped" if any)
             → Failed         (every file skipped: "nothing downloaded")
```

Held does not change the state in the Put.io stage. It is a flag shown as "On hold". After
Put.io finishes, a held transfer's files arrive paused, so it becomes `Nas(Paused)`.

### `Counts`

- `putio`: a count per Put.io state, with zeros, plus `on_hold` (outside the sum).
- `nas`: a count per NAS state, with zeros.

The sum of `putio` and `nas` equals the number of active items (SC-004).

### Stop signals (manager)

`HashMap<file_id, StopSignal>` where `StopSignal = { flag: AtomicBool, notify: Notify }`. An
entry is registered when a file starts and removed when it ends.

### Transfers snapshot (manager)

`Mutex<Option<(Instant, Arc<Vec<Transfer>>)>>`. `transfers_snapshot(max_age)` returns the
cached list if it is fresh enough, and otherwise fetches it once. On a Put.io error it returns
the last list with its age, so the API can mark it as out of date.

## Validation rules

- Pause and resume take item keys. Unknown keys get a per-item `not_found` result, not an
  error for the whole request.
- Pausing an item that is already paused or held is a no-op that reports `already`. The same
  goes for resuming an item that is not paused.
- A `folder:` or `file:` item has no transfer, so it can never be held. Pause on it pauses its
  files.
- `finished_retention_hours` must be from 1 to 720. A value outside that range is refused by the
  config lint with a clear message.
