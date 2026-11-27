# Feature 066: Say why something is waiting

**Roadmap**: 066 · **Status**: built

## Problem

A row that sits in the queue is usually doing the right thing: a schedule paused the
downloads, the disk is under its floor, every worker is busy. The page says none of that.
The user sees "pending" and has to guess, or read the log.

## What it does

Each row on the Active and Queue pages that is not downloading says why in one plain line,
and links to the setting that caused it. A failed row says what happened and what to do,
with a link to the troubleshooting guide.

## User stories

- **US1** A queue row that is not downloading shows one line with the reason.
- **US2** An Active item that is not downloading shows the same line.
- **US3** The line links to the setting behind the reason, when a setting is.
- **US4** A failed row says what failed and what to do next, and links to the guide.

## Requirements

- **FR-001** The daemon works out the reason, where the knowledge is, in the order the
  workers check before they claim a row: bandwidth pause, the Pause local downloads
  switch, free space, room for this file, a free worker.
- **FR-002** `GET /local/queue` rows and `GET /active` items carry `waiting`: null, or
  `{code, ...fields}`. A code is stable; fields are sent only when set.
- **FR-003** The web UI turns the code into words. An unknown code shows nothing, so an
  older UI is safe with a newer daemon.
- **FR-004** A reason is shown only when the daemon knows it. A pending row with a free
  worker and nothing in the way has no reason.
- **FR-005** `setting` is the config key behind the reason; the UI links it to its field.
- **FR-006** A failed row carries the kind of failure (`network`, `http`, `io`,
  `disk_full`, `integrity`, `other`) when this run of the daemon saw it fail.
- **FR-007** Other features add codes (064 adds its own); no code is renamed or reused.

## Codes

| Code | Meaning | Fields | Setting |
|---|---|---|---|
| `bandwidth_pause` | a timed pause is in force | `until` | - |
| `bandwidth_schedule` | a schedule entry pauses downloads | `entry`, `until_local` | `download.bandwidth_schedule` |
| `bandwidth_limit` | the default limit is "paused" | - | `download.bandwidth_limit_bytes_per_sec` |
| `downloads_paused` | the Pause local downloads switch | - | - |
| `disk_space` | free space is under the floor | `free_bytes`, `min_free_bytes` | `download.min_free_space_mb` |
| `disk_unreadable` | floor on, free space can't be read | - | `download.target_directory` |
| `file_too_big` | this file doesn't fit above the floor | `free_bytes`, `min_free_bytes` | `download.min_free_space_mb` |
| `waiting_turn` | every worker is busy | `workers` | `download.workers` |
| `paused` | the row is paused | - | - |
| `held` | paused while on Put.io | - | - |
| `failed` | the download failed | `kind` | - |
| `putio_queue` | Put.io has not started it | - | - |
| `waiting_for_sync` | done on Put.io, not yet queued | `until` | - |

## Left out, and why

- **A folder rule, a filter or the release pre-check, per row.** A file they leave out
  never becomes a queue row, so there is no row to put a reason on. The Active page says
  how many of an item's files were left out and links to Settings › Rules; it can't say
  which rule, because that is not stored.
- **A setting that needs a restart.** None of the restart-only settings (database, API
  address, logging, pre-check) holds a queue row back.
- **The failure kind after a restart.** It is kept in memory; a row that failed before
  the restart says "Failed" with no kind.
- **A worker backing off after Put.io could not be reached.** Not tracked per row.

## Success criteria

- **SC-001** With the switch on, a low disk, a schedule pause or all workers busy, every
  pending row names that reason within one poll.
- **SC-002** No row shows a reason the daemon did not work out.
