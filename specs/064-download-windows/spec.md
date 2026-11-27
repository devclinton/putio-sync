# Feature 064: Download windows and a storage budget

**Roadmap**: 064 · **Status**: built

## Problem

The bandwidth schedule (032) limits speed, not whether to download. There is no way to
say "only at night", no stop before the download disk is full other than holding single
files (the disk hold), and Put.io fills up until someone presses Free up space (033).
Nothing says when either disk will be full.

## What it does

Four rules, each off until set, each applied without a restart:

- **Download hours**: downloads run only inside set hours per weekday.
- **Disk pause**: every download pauses under a set free space and goes on above a
  higher one.
- **Storage budget**: the Put.io account is kept under a set share by deleting the
  oldest files that are confirmed downloaded.
- **Forecast**: the Stats page says when the download disk and the account fill.

## User stories

- **US1** Outside the download hours nothing downloads, and each waiting row says when
  the hours reopen.
- **US2** Under the pause level every download pauses; it goes on once the resume level
  is free, and the row gives both levels.
- **US3** With the budget on, the account goes back under it without me; no file that is
  not on my disk is ever deleted.
- **US4** Before I turn the budget on I see what it would delete now.
- **US5** The Stats page gives a date for each disk, or says honestly that it can't.

## Requirements

- **FR-001** `[[download.windows]]` entries are `days`, `start`, `end` as in the bandwidth
  schedule, in the daemon's local time. No entries: always open. An end before the start
  runs past midnight and belongs to its start day. Up to 50 entries; overlaps add up.
- **FR-002** A window follows the wall clock when clocks change: a start in the skipped
  hour opens at the next real time, and the repeated hour is inside both times.
- **FR-003** `download.pause_free_space_mb` (0 = off) pauses when free space is under it;
  the pause ends at `download.resume_free_space_mb` or more (0 = 1024 MB above the pause
  level). Free space that can't be read keeps the state it had.
- **FR-004** Both rules stop new claims and pause running downloads in place, through
  the rate limiter the bandwidth pause uses. They come after a bandwidth pause and before
  the Pause switch and the disk hold in the order of reasons.
- **FR-005** Waiting codes: `download_window` (`until_local`, setting `download.windows`)
  and `disk_pause` (`free_bytes`, `min_free_bytes`, `resume_bytes`, setting
  `download.pause_free_space_mb`). Rows that were downloading show them too.
- **FR-006** `storage.budget_enabled` (default false) and `storage.budget_used_percent`
  (10–99, default 90). After each account reading (15 minutes) the daemon, when over
  budget, deletes the oldest eligible files until under.
- **FR-007** Safety. Eligible is 033's rule and nothing else: a completed download record
  of the same size, the local file present with that size, not queued, not shared, not
  under an unfinished transfer. Each file is checked again right before its delete.
  Every deletion is logged, added to History and recorded as a run with trigger
  `budget`. `cleanup.deleted` is sent with reason `storage_budget`.
- **FR-008** `POST /putio/storage/budget/preview` (`used_percent` optional) returns the
  plan and deletes nothing; its id cannot be confirmed by `/putio/storage/free`. The
  Settings switch is locked until a preview of the entered percent was shown, unless
  that percent is already saved on.
- **FR-009** One usage reading an hour of the account and the download disk is stored
  (30 days kept). `GET /stats/forecast` fits a line through the last week: `filling`
  with `days_left` and `full_at`, `not_enough_data` (under 3 readings or under a day),
  `not_growing` (level, falling, or over ten years away) or `full`.
- **FR-010** Config validation names the entry and field. None of the keys needs a
  restart.

## Out of scope

- A server-side lock that refuses `budget_enabled = true` without a preview: the config
  file can always set it, so the lock is in the Settings page only.
- Budget runs under `server` (no schedule loop there; the settings save says so).
- Windows per folder or per transfer.
