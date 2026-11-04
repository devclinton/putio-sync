# Research: Bandwidth Schedules

## R1 What exists

**Finding**: `putio_sync_download::RateLimiter` (a per-second window, `&mut self`) is
built inside `stream_to_temp` from `bandwidth_limit_bytes_per_sec`, one per stream, so the
total is the limit times the running downloads. `spawn_worker` divides the limit by the
worker count only for `WorkerStats` (`/local/workers`). `RateLimiter::new(0)` would panic
(`Duration::from_secs_f64(inf)`), so no working config uses 0.

**Decision**: keep `RateLimiter` and make it the shared limiter: the manager owns one
(`Arc` inside), every stream borrows it. `stream_to_temp` / `stream_download` take
`Option<&RateLimiter>` instead of the number. No second limiter.

## R2 Token bucket shape

**Decision**: tokens in an `f64` with a capacity of one second at the current rate. A
`consume(n)` takes `n` tokens at once (going negative is allowed); while the balance is
negative it sleeps `min(debt / rate, 1 s)` and re-checks, re-reading the limit, so a lower
or higher limit applies within a second. Unlimited returns at once. Paused sleeps in 1 s
steps until not paused. Callers wrap the wait in `unless_stopped`, so a stop still ends a
paused download at once.

**Rationale**: one mutex, no background task, fair enough between streams (each waits in
turn on the shared debt), exact over a minute (SC-001).

**Alternatives**: `governor` crate (new dependency, Constitution VII); a semaphore of
bytes refilled by a task (needs a task per manager and tests that wait on it).

## R3 When the limit is evaluated

**Decision**: `Policy::at(local, utc, override)` is pure. The limiter caches the result
with the `Instant` it was computed and recomputes when it is older than 1 s or when the
policy or override is replaced. The clock is a boxed `Fn() -> (NaiveDateTime,
DateTime<Utc>)`, `Local::now()` in production, fixed in tests.

**Rationale**: wall-clock time is all the schedule needs; one evaluation a second costs
nothing; clock steps (NTP) are followed within a second.

## R4 Daylight saving and time zones

**Decision**: the schedule compares `Local::now().naive_local()` (wall clock) to the
entry. Skipped hour: the window starts when the clock passes it. Repeated hour: inside the
window both times. The zone is the process's (`TZ` in Docker, `/etc/localtime`
otherwise). `GET /bandwidth` reports `local_time` and `utc_offset` (`+01:00`), the Status
card and the doctor show them, the README says so.

## R5 Windows and days

**Decision**: `start` `00:00`–`23:59`, `end` `00:01`–`24:00` (minutes, `HH:MM`).
`start < end` is a same-day window; `end < start` runs past midnight and `days` name the
day it starts on (Friday 22:00–06:00 covers Saturday 03:00). `start == end` is refused.
`days` is one string: comma-separated items, each a day (`mon`, `monday`…), a range
(`mon-fri`, `fri-mon` wraps), or `daily`, `weekdays`, `weekends`; case-insensitive. At
most 50 entries.

**Rationale**: one string reads well in TOML (`days = "mon-fri"`) and in the UI field.

## R6 Overlap and the order rule

**Decision**: the first entry in list order whose window contains the time wins. Overlaps
are warnings, found by walking the 10 080 minutes of a week once (cheap at ≤ 50 entries).
The same walk finds "paused all week".

## R7 Override

**Decision**: `Override { mode: pause | slow, until: DateTime<Utc> }`, 1 to 1440 minutes,
stored as JSON under the `daemon_flags` key `bandwidth_override`. Slow applies
`min(slow_limit, schedule limit)`; pause applies `paused`. A new override replaces the old
one. Expired overrides are ignored by everyone (limiter, doctor) and dropped from memory;
the row is deleted on `DELETE` and overwritten on the next `PUT`.

**Rationale**: end times are absolute so a restart keeps the same end (FR-011). 039's
untimed `nas_downloads_paused` is a different switch with its own semantics (stops and
re-queues files); it stays separate.

## R8 Pause and the queue

**Decision**: `process_queue` returns `Ok(false)` without claiming while the limiter reads
paused (the worker loop then sleeps 1 s). Running files wait in `consume`. A connection
Put.io drops during a long pause errors on the next read and goes through the existing
retry with resume; the stall timeout wraps only the network read, not the wait, so a pause
is never a stall (FR-008).

## R9 Metrics

**Decision**: `putio_sync_download_bandwidth_limit_bytes_per_second` (Gauge, `+Inf`
unlimited, `0` paused; starts at `+Inf`) and `putio_sync_download_bandwidth_override`
(IntGauge, 1 while a pause or slow-down is in force). Set by the limiter when it
recomputes. No labels.

## R10 Doctor and assistant

**Decision**: a new doctor check `bandwidth` ("Bandwidth limit", Area Sync) computes the
limit from the config and the stored override (the doctor has the config and the
database, not the download manager). OK: "Downloads are limited to 5.0 MB/s now (default
limit). Times use the daemon's local time (UTC+01:00)." Warning while a pause is in force.
The schedule warnings are assistant risks (`bandwidth_overlap`, `bandwidth_paused_week`,
`bandwidth_low`, Level::Warning, key `download.bandwidth_schedule` or
`download.bandwidth_limit_bytes_per_sec`), shown by the Settings banner and the doctor's
Config check.

## R11 Validation

**Decision**: `Config::validate` calls `bandwidth::validate(&download)`: each entry parsed
strictly (`download.bandwidth_schedule entry 2: start '25:00': expected HH:MM between 00:00
and 23:59`), at most 50 entries, a numeric limit of 0 refused (write `"paused"`), and
`bandwidth_limit_bytes_per_sec = 0` and `slow_limit_bytes_per_sec = 0` refused. Settings
saves, the wizard and import all go through `Config::validate` already.

## R12 Workers

**Decision**: `WorkerStats.bandwidth_limit` and `/local/workers` `bandwidth_limit_bps`
stop carrying a per-worker share; they are `null`, and `total_bandwidth_limit_bps` is the
current limit in bytes per second (`null` when unlimited, `0` when paused).
