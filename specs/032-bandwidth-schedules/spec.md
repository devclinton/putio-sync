# Feature Specification: Bandwidth Schedules

**Feature Branch**: `032-bandwidth-schedules` (built on `claude/strange-feistel-755f52`)

**Created**: 2026-11-04

**Status**: Draft

**Input**: User description: "032 Bandwidth schedules: download speed limits by time of
day and day of week (for example unlimited from 01:00 to 07:00, 5 MB/s otherwise), plus a
one-click "pause" or "slow" for an hour. The current limit shows on the Status page. Build
on the existing `[download] bandwidth_limit_bytes_per_sec` limiter rather than adding a
second one. Cover config validation and the doctor, the settings UI, the current limit as
a gauge on the Prometheus endpoint, and time zones (the daemon's local time, documented)."

## Context

- `[download] bandwidth_limit_bytes_per_sec` exists today (config, Settings › Download,
  `download --bandwidth-limit`). It is applied to each file being downloaded on its own, so
  with four workers the NAS can pull four times the number set. The Workers card divides
  the number by the worker count for display, which does not match what happens.
- The limit is read when a file starts. A change in Settings reaches the next file, not the
  ones already running.
- 039 keeps a small table of daemon flags that survive a restart (the Pause NAS downloads
  switch lives there). Its pause switch is a separate, untimed feature that 039 still has
  to finish; this feature does not build it.
- The doctor, the Settings banner (the config assistant, 015) and `Config::validate` are
  the three places where a bad setting is reported.

## Clarifications

### Session 2026-11-04

Answered with the recommended option, without a live user; listed in the hand-over note
for review.

- Q: Should the existing limit become the total across workers, although that slows down
  anyone who relied on it being per file? → A: Yes, the total. It is what the setting and
  the docs say; the changelog lists it under Changed.
- Q: While paused, should running files be stopped and put back in the queue, or held
  where they are? → A: Held where they are (no bytes move); the resume path covers a
  dropped connection.
- Q: What does the limit gauge read when there is no limit? → A: `+Inf`, so `0` keeps
  meaning paused and graphs with `min()` work.
- Q: Which lengths does the UI offer for pause and slow? → A: One hour, the roadmap's
  one click; the API takes any length from 1 minute to 24 hours.
- Q: Does a schedule entry's limit replace the slow limit's ceiling when the schedule is
  lower? → A: The lower of the two always applies (FR-010).

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Speed limits by time of day and day of week (Priority: P1)

A user whose household shares one internet line wants putio-sync to download at full
speed overnight and to stay at 5 MB/s during the day, and to stay off the line entirely on
weekday evenings when everyone is streaming. They add schedule entries in Settings ›
Download (or in the config file), each with days, a start time, an end time and a limit,
and the daemon follows them on its own.

**Why this priority**: it is the feature; the rest builds on the current limit it decides.

**Independent Test**: set a default of 5 MB/s and an entry "every day 01:00–07:00
unlimited"; with the daemon's clock at 02:00 the limit is unlimited, at 08:00 it is
5 MB/s, and a download running across 07:00 slows down without restarting.

**Acceptance Scenarios**:

1. **Given** a default limit of 5 MB/s and an entry "daily 01:00–07:00, unlimited",
   **When** the daemon's local time is 03:00, **Then** downloads are not limited, and at
   07:00 they drop to 5 MB/s total within a few seconds.
2. **Given** an entry "Mon–Fri 18:00–23:00, paused", **When** it is Tuesday 19:00,
   **Then** no new file starts and running files stop moving bytes until 23:00, when they
   carry on from where they were.
3. **Given** an entry "Fri 22:00–06:00" (end before start), **When** it is Saturday 03:00,
   **Then** the entry applies: the window runs past midnight into the next day.
4. **Given** two workers and a limit of 4 MB/s, **When** both download at once, **Then**
   the two together stay at about 4 MB/s, not 8.
5. **Given** a schedule edited in Settings and saved, **When** the save succeeds, **Then**
   the new limit applies to running downloads within a few seconds, without stopping them.
6. **Given** no schedule entries, **Then** the default limit behaves as before (except
   that it is now the total, scenario 4).

---

### User Story 2 - Pause or slow for an hour, in one click (Priority: P2)

A user is about to join a video call. From the Status page they click "Pause for 1 hour"
(or "Slow for 1 hour"); downloads stop (or drop to the slow limit) at once and go back to
the schedule on their own an hour later. "Resume schedule" ends it early.

**Why this priority**: the second thing the roadmap asks for; it needs the shared limit
from story 1 to act on running downloads.

**Independent Test**: click Pause for 1 hour; the Status page shows "Paused until 15:42",
the gauge reads 0, nothing downloads; click Resume schedule and downloads carry on.

**Acceptance Scenarios**:

1. **Given** downloads running, **When** the user clicks Pause for 1 hour, **Then** they
   stop moving bytes within a few seconds, no new file starts, and the Status page shows
   the pause and when it ends.
2. **Given** a pause, **When** the hour is over, **Then** the schedule applies again with
   nobody doing anything.
3. **Given** Slow for 1 hour with a slow limit of 1 MB/s, **Then** the total is about
   1 MB/s for the hour, even inside an unlimited schedule window.
4. **Given** a pause or a slow-down, **When** the daemon restarts within the hour,
   **Then** it is still in force with the same end time.
5. **Given** a pause, **When** the user clicks Resume schedule, **Then** it ends at once.
6. **Given** the API, **When** a client asks for a pause or slow-down of N minutes (1 to
   24 hours), **Then** it gets that length; the UI offers one hour by default.

---

### User Story 3 - See the current limit and why (Priority: P2)

On the Status page a Bandwidth card says what the limit is now, where it comes from (the
default, a schedule entry, a pause, a slow-down), when it changes next and to what, and
the daemon's own time and time zone, so a schedule that runs in the wrong zone is easy to
spot.

**Independent Test**: with the schedule from story 1 at 08:00, the card reads "5 MB/s ·
default · next: unlimited at 01:00" and "Daemon time 08:00 (UTC+01:00)".

**Acceptance Scenarios**:

1. **Given** any state, **Then** the card shows the limit, the source, the next change
   (or "no change scheduled") and the daemon's local time with its UTC offset.
2. **Given** a pause, **Then** the card shows "Paused" with its end time and a Resume
   schedule button.
3. **Given** the Workers card, **Then** it no longer shows a per-worker split of the limit
   that does not happen.

---

### User Story 4 - Watch it from monitoring (Priority: P3)

The home monitoring stack graphs the current limit next to the download rate, so a dip in
speed can be told apart from a schedule window.

**Acceptance Scenarios**:

1. **Given** the metrics endpoint, **Then** it has a gauge with the current limit in
   bytes per second: `+Inf` when unlimited, `0` when paused.
2. **Given** a pause or slow-down, **Then** a second gauge reads 1 while it lasts, else 0.

---

### User Story 5 - Mistakes are caught before they bite (Priority: P3)

A user typing a schedule by hand in the config file makes a mistake (25:00, an unknown day,
an entry that starts and ends at the same time). The daemon refuses the config and says
which entry and field is wrong; the Settings page refuses the save the same way. Entries
that are valid but probably not what was meant (two entries overlapping, a schedule that
pauses for the whole week, a limit below 64 KB/s) show as warnings in the Settings banner
and in the doctor.

**Acceptance Scenarios**:

1. **Given** an entry with `start = "25:00"`, **When** the daemon starts, **Then** it
   stops with an error naming the entry number and the field.
2. **Given** the same mistake in Settings, **Then** the save is refused with the same
   message and nothing is written.
3. **Given** two entries that overlap, **Then** the doctor and the Settings banner warn
   which ones, and the earlier entry in the list wins where they overlap.
4. **Given** a pause in force, **Then** the doctor's Bandwidth check says so as a warning
   with its end time, so a forgotten pause is visible.

### Edge Cases

- **Midnight**: an entry whose end is before its start runs past midnight; its days name
  the day it starts on. An end of `24:00` means the end of the day.
- **Same start and end**: refused; an all-day entry is `00:00`–`24:00`.
- **Overlaps**: allowed with a warning; the first matching entry in list order wins.
- **Daylight saving**: entries follow the wall clock. When the clock skips an hour, an
  entry starting inside the skipped hour starts when the clock jumps past it; when an hour
  repeats, the repeated hour is in the window both times.
- **Time zone**: the daemon's local time zone (the `TZ` variable in Docker). It is shown
  on the Status page and in the doctor; the docs say so.
- **Clock changes** (NTP step, manual change): the limit follows the new time within a few
  seconds.
- **Pause and in-flight files**: a paused file keeps its partial data. If Put.io drops the
  idle connection during a long pause, the normal resume path continues the file
  afterwards; it never counts as a failed download by itself.
- **Pause and the stall detector**: time spent paused or held back by the limit is not a
  stall.
- **Slow limit above the schedule**: Slow for 1 hour never raises the limit; the lower of
  the slow limit and the schedule's limit applies.
- **Very small limits**: any limit of at least 1 byte per second works; below 64 KB/s gets
  a warning (downloads crawl and Put.io may drop the connection).
- **Setup mode**: the new routes answer 503 like every other route until setup is done.
- **CLI one-shot downloads** (`download --bandwidth-limit`) keep working, with the number
  as the total.
- **Restarts**: a pause or slow-down whose end time passed while the daemon was down is
  dropped at start.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The configuration MUST accept a list of schedule entries, each with days of
  the week, a start time, an end time (`HH:MM`, 24-hour, `24:00` allowed as an end) and a
  limit that is a number of bytes per second, unlimited, or paused.
- **FR-002**: Days MUST accept single days (`mon` … `sun`), ranges (`mon-fri`, wrapping
  like `fri-mon`) and the words `daily`, `weekdays` and `weekends`, in any case.
- **FR-003**: Outside every entry, the existing default limit
  (`bandwidth_limit_bytes_per_sec`) MUST apply, unlimited when unset.
- **FR-004**: The first entry in list order whose window contains the daemon's local time
  MUST decide the limit.
- **FR-005**: The limit MUST be the total across every concurrent download, not per file.
- **FR-006**: A change of limit (schedule boundary, quick action, settings save) MUST reach
  downloads already running within 5 seconds, without restarting or failing them.
- **FR-007**: While the limit is paused, no new file MUST start downloading and running
  files MUST move no bytes; they continue from where they were when the pause ends.
- **FR-008**: Time held back by the limit or a pause MUST NOT count as a stall or a failure.
- **FR-009**: Users MUST be able to pause, or slow to a configured slow limit, for a length
  of their choice from 1 minute to 24 hours (one hour in the UI), and to end it early.
- **FR-010**: The slow limit MUST be configurable, default 1 MiB/s, and MUST never raise
  the limit above what the schedule gives.
- **FR-011**: A pause or slow-down MUST survive a daemon restart until its end time.
- **FR-012**: The Status page MUST show the current limit, its source, the next change and
  the daemon's local time with its UTC offset, plus the quick actions.
- **FR-013**: The API MUST expose the same current state (limit, source, override and its
  end, next change, daemon local time and offset) for the UI and other clients.
- **FR-014**: The metrics endpoint MUST have a gauge with the current limit in bytes per
  second (`+Inf` unlimited, `0` paused) and a gauge that is 1 while a pause or slow-down
  is in force.
- **FR-015**: Invalid entries (bad time, unknown day, empty days, same start and end, a
  limit of zero written as a number) MUST be refused by config loading, by the setup
  wizard's validation, by Settings saves and by settings import, each naming the entry
  and field.
- **FR-016**: Overlapping entries, a schedule that leaves no time unpaused in the week and
  limits below 64 KB/s MUST be reported as warnings by the Settings banner and the doctor.
- **FR-017**: The doctor MUST have a Bandwidth check showing the current limit and source
  and the daemon's time zone, warning while a pause is in force.
- **FR-018**: Settings › Download MUST let the user add, edit, reorder and remove schedule
  entries and set the slow limit, with limits entered in MB/s.
- **FR-019**: The Workers card and `/local/workers` MUST stop reporting a per-worker share
  of the limit that does not exist; they report the total limit instead.
- **FR-020**: The README, `config.example.toml` and the changelog MUST describe the
  schedule, the total-limit change, and that times are the daemon's local time.

### Key Entities

- **Schedule entry**: days, start, end, limit (bytes per second, unlimited, or paused).
- **Override**: a pause or slow-down with an end time, persisted as a daemon flag.
- **Current limit**: the effective limit now, its source (default, entry *n*, pause,
  slow), and the next change (time and limit).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With a limit of L and any number of workers, the total download rate over a
  minute stays within 10% of L.
- **SC-002**: A schedule boundary, pause, resume or settings save changes the rate of a
  running download within 5 seconds, with no download failing or starting over.
- **SC-003**: Pausing from the Status page takes one click and nothing downloads after it.
- **SC-004**: Every invalid schedule a user can type is refused with a message naming the
  entry and field, in the config file and in Settings.
- **SC-005**: A user can tell from the Status page alone what the limit is, why, and when
  it changes next.

## Assumptions

- Limits in the config stay in bytes per second, like the existing key; the UI enters and
  shows MB/s (MiB-based, matching how the rest of the UI shows sizes).
- "Paused" in a schedule entry is written `limit = "paused"`; "unlimited" is written
  `limit = "unlimited"`; a number is bytes per second. An absent limit is refused, so an
  entry always says what it does.
- The quick actions live on the Status page; the Active page's untimed Pause NAS downloads
  switch (039) is separate and unchanged.
- Uploads (torrent files to Put.io) and Put.io API calls are not limited; only downloads.
- No per-folder or per-worker limits.
- One-shot CLI downloads follow the same schedule as the daemon (they read the same
  `[download]` table); `--bandwidth-limit` replaces the default limit only. Pause and
  slow-down are daemon-only.
