# Feature Specification: CPU and Memory Telemetry, Kept Long Term

**Feature Branch**: `041-resource-telemetry-archive`

**Created**: 2026-10-27

**Status**: Draft

**Input**: User description: "i would like telemetry on cpu usage and ram usage on this app
long-term through grafana and maybe some data archive scripts?"

## Context

- Feature 003 added Prometheus metrics on port 9742. They cover what putio-sync does (syncs,
  downloads, Put.io calls, watch folders, disk space), plus its start time and build. None of
  them say how much CPU or memory putio-sync itself uses. The live endpoint on the NAS has no
  CPU, memory, thread or open-file figures today (checked 2026-10-27).
- putio-sync runs as the `putio-sync` Docker container on the NAS (nas01, Unraid). On
  2026-10-27 it was using about 36 MB of memory and almost no CPU. Nothing records how that
  changes over days or months, so a slow memory leak or a CPU spike would go unnoticed.
- Prometheus and Grafana do **not** run on the NAS. They run on the monitoring server
  (monitoring01, 192.168.1.60) and are managed in `~/dev/home-network`. That Prometheus
  already scrapes putio-sync on the NAS every 15 s (job `putio_sync`), and Grafana already has a
  `putio-sync` dashboard provisioned from that repo.
- That Prometheus keeps data for **30 days**, for every app at once. It holds about 11,400
  series in about 3.5 GB of disk, and putio-sync makes up about 195 of them. The monitoring
  server has about 618 GB free. Anything older than 30 days is gone today, so "long term" is
  not possible without a change.
- putio-sync's own database already keeps every finished download (feature 006 charts it). On
  the NAS it is about 4.6 MB, so it does not need trimming for size.
- No other container on the NAS exports per-container CPU or memory to Prometheus.

## Clarifications

### Session 2026-10-27

- Q: How should a year of history be kept: raise Prometheus retention for everything, or keep
  30 days of full detail plus a thinned putio-sync-only copy for a year? → A: The second.
  Prometheus keeps its 30 days. A scheduled job keeps putio-sync's figures for a year, thinned
  to 5-minute points, and Grafana can still draw them.
- Q: Should the database archive script only copy history, or also delete old rows? → A: Copy
  and summarize. It copies the history and never deletes. It also writes a summary of what
  putio-sync did and what it used.
- (from the user) The goal is aggregate figures over time: how much CPU and memory putio-sync
  uses while it is downloading, syncing or idle. The thinned copy, the dashboard and the summary
  are built around that, not around raw detail.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - putio-sync reports its own CPU and memory use (Priority: P1) 🎯 MVP

As the NAS operator, I want putio-sync's metrics to include how much CPU and memory it is using,
so I can see its resource use next to what it is doing.

**Why this priority**: Nothing else in this feature works until these figures exist. On its own,
it already answers "is it using too much memory right now?" with one request.

**Independent Test**: Start putio-sync, read the metrics endpoint, and check that each resource
figure below is present with a believable value. Do some work (for example, a synthetic
download), read again, and check that CPU time went up and that memory changed.

**Acceptance Scenarios**:

1. **Given** putio-sync is running with default metrics settings, **When** metrics are read,
   **Then** they show:
   - total CPU time used since start;
   - memory in use now (resident memory);
   - memory reserved (virtual memory);
   - the number of threads;
   - the number of open files, and the most it is allowed to open;
   - bytes read from and written to disk since start.
2. **Given** the figures above, **When** they are read twice a minute apart while putio-sync is
   busy, **Then** CPU time and disk bytes only go up, and memory and thread counts reflect the
   moment they were read.
3. **Given** putio-sync runs in daemon mode or server mode, **When** metrics are read, **Then**
   both modes show the same resource figures with the same names.
4. **Given** a platform where a figure can't be read (for example, on a Mac during
   development), **When** metrics are read, **Then** that figure is missing rather than shown
   as 0, and putio-sync logs one warning saying which figures are unavailable.
5. **Given** any new figure, **When** its labels are examined, **Then** none carry a file name,
   path, id or token.

---

### User Story 2 - CPU and memory over months in Grafana (Priority: P1)

As the operator, I want a Grafana dashboard that shows putio-sync's CPU and memory over days,
weeks and months, so I can spot a slow leak or a change after an upgrade.

**Why this priority**: This is what the user asked for. The figures from US1 are only useful
once they are kept and drawn.

**Independent Test**: In `~/dev/home-network`, the repo's own check (`make check`) passes
with the new dashboard. After the operator deploys, the dashboard loads against live data with
no panel errors, at both a 24-hour and a 30-day range.

**Acceptance Scenarios**:

1. **Given** the dashboard is opened, **Then** it shows, over the chosen time range:
   - CPU use as a percentage of one core;
   - memory in use and memory reserved;
   - threads;
   - open files against the limit;
   - disk read and write rates;
   - uptime, with restarts and new versions marked on the charts;
   - what putio-sync was doing at the same time (downloads running, download speed, syncs), on
     the same time axis as CPU and memory.
2. **Given** the dashboard is opened, **Then** a "use by activity" section shows average and
   peak CPU and memory for each activity state over the chosen range:
   - idle (no download running and no sync in progress);
   - syncing only;
   - downloading, split by how many downloads were running at once (1, 2, 3, and so on up to
     the configured worker count).

   It also shows memory and CPU per MB/s of download speed, so the cost of downloading can be
   compared across months and versions.
3. **Given** a time range of a year, **When** the dashboard is opened, **Then** it loads in
   under 10 seconds. Beyond 30 days it draws from the one-year copy (US3), with average and
   peak for each point.
4. **Given** a restart (for example the weekly backup restart), **When** the CPU and disk
   charts cross it, **Then** they show no false spike or dip caused by counters starting
   again at 0.
5. **Given** the existing `putio-sync` dashboard, **When** this feature ships, **Then** it
   links to the new one, and the new one has the same `host` filter.

---

### User Story 3 - A thinned year of putio-sync history (Priority: P1)

As the operator, I want a year of putio-sync's resource and activity figures, thinned to
5-minute points, so I can look at months of use without keeping every app's full detail for a
year.

**Why this priority**: Without it, US2 can never show more than 30 days, and the user asked for
long term.

**Independent Test**: Load a test Prometheus with a known week of putio-sync series at 15-second
detail. Run the thinning job and check that, for each 5-minute point, the kept average and peak
match the ones worked out by hand from the raw series, and that the activity state matches. Check
that the dashboard draws the thinned copy for a range older than the raw data.

**Acceptance Scenarios**:

1. **Given** the job is set up, **When** it runs on its schedule, **Then** it keeps, for every
   5 minutes of putio-sync data:
   - average and peak CPU use;
   - average and peak memory in use, and the peak of memory reserved;
   - average and peak threads and open files;
   - disk read and write totals;
   - downloads running (average and peak), bytes downloaded, and whether a sync ran;
   - the activity state the 5 minutes mostly spent in (idle, syncing, downloading with N at
     once).
2. **Given** the thinned copy, **When** the operator looks at putio-sync from up to one year
   ago in Grafana, **Then** the data is there, at 5-minute detail.
3. **Given** the thinned copy is a year old, **When** the job runs, **Then** points older than
   a year (a setting) are dropped, and only putio-sync's own copy is affected.
4. **Given** Prometheus keeps its 30-day retention, **When** the change is deployed, **Then**
   no other app's data is kept for longer, and the extra disk use is small (estimate in the app
   page, well under 1 GB a year).
5. **Given** the job missed runs (monitoring server off for a day), **When** it next runs,
   **Then** it fills in the missed points that are still in Prometheus, and marks the rest as a
   gap.

---

### User Story 4 - Archive and summary scripts (Priority: P2)

As the operator, I want scripts that save putio-sync's history to plain files I keep, and that
sum it up per day and per month, so I can see how resource use relates to the work done, even
after a Prometheus wipe or a monitoring-server rebuild.

**Why this priority**: US3 keeps the thinned year inside the monitoring stack. These scripts are
an offline copy, and the summary answers "what does putio-sync use while downloading?" without
opening Grafana.

**Independent Test**:
- Run the metrics archive script against a test thinned copy holding a known week. Check that it
  writes one compressed file per day, and that a second run skips the days already saved.
- Run the database archive script against a copy of a putio-sync database. Check that the copy
  has every download row and that the database was not changed.
- Run the summary script over the same week and database. Check each daily and monthly figure
  against values worked out by hand.

**Acceptance Scenarios**:

1. **Given** the metrics archive script, **When** it runs, **Then** it saves the thinned
   5-minute putio-sync figures (US3) for each finished day not yet saved, into one compressed
   file per day in a folder the operator chooses.
2. **Given** a day already saved, **When** the script runs again, **Then** that day is left as
   it is, so the script can safely run every day on a schedule.
3. **Given** a saved archive, **When** the operator wants it back in Grafana, **Then** a
   documented restore step loads the files back into the thinned copy (or a spare one) so the
   same dashboard can draw them.
4. **Given** the database archive script, **When** it runs, **Then** it saves a consistent copy
   of putio-sync's download history to a dated, compressed file, while putio-sync keeps running.
   It never deletes or changes rows in the live database.
5. **Given** the summary script, **When** it runs, **Then** it writes, for each day and each
   month, one row with:
   - downloads finished, bytes downloaded, time spent downloading, and average speed (from the
     download history);
   - time spent idle, syncing and downloading (from the thinned figures);
   - average and peak CPU and memory in each of those activity states;
   - average CPU and memory per MB/s of download speed;
   - restarts and the versions that ran that day.

   The summary is a plain table that opens in a spreadsheet, plus a short readable text version
   for the latest month.
6. **Given** a day with no thinned figures (for example before this feature, or a gap), **When**
   the summary is written, **Then** the download columns are filled from the history and the
   resource columns are left blank, not 0.
7. **Given** any of the scripts fails (Prometheus unreachable, folder not writable, disk full),
   **When** it runs, **Then** it exits with a non-zero status and a clear message, and it leaves
   no half-written file that a later run would mistake for a finished day.
8. **Given** archives older than a limit the operator sets (default: keep forever), **When** a
   script runs, **Then** it removes only its own files older than that limit.

---

### User Story 5 - Alerts when putio-sync uses too much (Priority: P3)

As the operator, I want to be told when putio-sync's memory keeps growing or its CPU stays
high, instead of finding out when the NAS slows down.

**Why this priority**: A nice payoff of US1–US3, but the dashboard already shows these by eye.

**Independent Test**: The home-network repo's alert-rule check passes. Each new rule fires on a
synthetic "bad" series and stays silent on a healthy one, in the same alert-test file that
feature 003 uses.

**Acceptance Scenarios**:

1. **Given** putio-sync's memory in use stays above 1 GB for 30 minutes, **Then** a warning
   fires.
2. **Given** putio-sync's memory grows steadily for 24 hours with no restart, at a rate that
   would pass 1 GB within 7 days, **Then** a "possible memory leak" warning fires.
3. **Given** putio-sync uses more than 90% of one core for 30 minutes, **Then** a warning
   fires.
4. **Given** open files pass 80% of the limit, **Then** a warning fires.

### Edge Cases

- **Restart**: CPU time and disk bytes start again at 0. The dashboard and alerts use rates, so
  a restart shows as a gap at most, never as a spike or a negative value.
- **Container memory limit**: the container on the NAS has no memory limit today, so the host's
  memory (15.4 GB) is the only ceiling. If a limit is set later, the dashboard still works; the
  limit itself is out of scope.
- **Metrics turned off** (`metrics.enabled = false`): no resource figures are served either,
  and nothing extra is collected.
- **Reading the figures is slow or fails**: the metrics endpoint still answers within 1 s with
  everything else. A figure that can't be read this time is left out of that reading.
- **Archive folder on the NAS share disappears** (share unmounted): the script fails as in US4
  scenario 5. It does not write into an empty mount point.
- **Gaps in Prometheus** (monitoring server down for a day): the archive for that day is saved
  with the points that exist and is marked as partial. The script does not wait or retry
  forever.
- **Clock change or daylight saving**: archive days are UTC days, so no day is saved twice or
  skipped.
- **Restoring an archive into the thinned copy**: restored points must not clash with the ones
  already there. The restore step only loads days that are missing, or goes into a spare copy.
- **Short bursts**: a download that starts and ends inside one 5-minute point still counts.
  Each point keeps the peak as well as the average, so a short spike in CPU or memory is not
  averaged away.
- **Worker count changes**: if the configured worker count changes, "downloading with N at
  once" still means N downloads actually running, so months stay comparable.
- **Summary over a restart**: a day with a restart is summed across both runs. Running totals
  that started again at 0 are not counted twice or lost.

## Requirements *(mandatory)*

### Functional Requirements

**Resource figures in putio-sync**

- **FR-001**: putio-sync MUST add these figures about its own process to its existing metrics:
  - total CPU time since start;
  - resident memory;
  - virtual memory;
  - thread count;
  - open files and the open-file limit;
  - bytes read from and written to disk since start.

  Start time and build info already exist and are kept as they are. putio-sync MUST also
  report whether a sync is running right now, so time spent syncing can be told apart from
  idle time. Downloads running already exists.
- **FR-002**: The new figures MUST follow the existing metric naming rules (counters for running
  totals, base units of bytes and seconds) and carry no labels with file names, paths, ids or
  tokens.
- **FR-003**: The figures MUST be read when metrics are requested, so each scrape gets a fresh
  value. Reading them MUST NOT delay the metrics response past 1 s or slow down downloads.
- **FR-004**: The figures MUST be on whenever metrics are on, at both the `basic` and
  `detailed` settings. No new setting is needed to turn them on.
- **FR-005**: On a platform where a figure can't be read, putio-sync MUST leave that figure out
  and log one warning at startup naming the missing figures. It MUST keep running.
- **FR-006**: Daemon mode and server mode MUST expose the same figures with the same names.

**Dashboard** (changes in `~/dev/home-network`)

- **FR-007**: Add a Grafana dashboard for putio-sync's resource use, provisioned by that repo's
  Grafana role next to the existing `putio-sync` dashboard. It MUST show CPU, memory in use and
  reserved, threads, open files against the limit, disk read and write rates, and uptime, with
  restarts and version changes marked. It MUST show what putio-sync was doing on the same time
  axis, and a "use by activity" section with average and peak CPU and memory for idle, syncing,
  and downloading with N at once, plus CPU and memory per MB/s of download speed (US2).
- **FR-008**: The dashboard MUST stay quick over a one-year range by drawing coarser points as
  the range grows. It MUST have the same `host` filter as the existing dashboard. The two
  dashboards MUST link to each other.
- **FR-009**: The same dashboard MUST also be shipped in the putio-sync repo as a file anyone
  can import into their own Grafana, choosing their own data source on import. The home-network
  copy and the putio-sync copy MUST be kept the same, with one of them named as the source.

**Long-term history** (changes in `~/dev/home-network`)

- **FR-010**: Prometheus MUST keep its current 30-day retention for full detail. A scheduled job
  MUST keep a thinned copy of putio-sync's figures only, at one point every 5 minutes with
  average and peak values and the activity state, as listed in US3. The copy MUST be kept for
  one year by default (a setting) and MUST be drawable by the same Grafana dashboard.
- **FR-010a**: The dashboard MUST use the full-detail data for ranges inside the last 30 days
  and the thinned copy for older ranges, without the operator having to switch by hand.
- **FR-011**: The app page for putio-sync in that repo MUST state how long each kind of data is
  kept, how much disk the thinned copy is expected to use per year, how to check that the job is
  running, and what to do when it has stopped.
- **FR-011a**: An alert MUST fire when the thinning job has not saved new points for 2 hours.
- **FR-012**: An alert MUST fire when the monitoring server's disk for Prometheus data passes a
  set level (default: 80% full), if no such alert exists already.

**Archive scripts**

- **FR-013**: A metrics archive script MUST save the thinned 5-minute putio-sync figures for
  each finished UTC day not yet saved, one compressed file per day, in a folder the operator
  sets. It MUST be safe to run every day and MUST skip days already saved.
- **FR-014**: The metrics archive MUST be in a documented, plain format that can be read
  without putio-sync. A documented restore step MUST load archived days back into the thinned
  copy, or a spare one, so the dashboard can show them again.
- **FR-015**: A database archive script MUST save a consistent copy of putio-sync's download
  history while putio-sync is running, as a dated, compressed file. It MUST NOT delete or change
  anything in the live database.
- **FR-015a**: A summary script MUST write one row per day and one per month with the figures in
  US4 scenario 5: downloads, bytes, time downloading and average speed from the history; time
  idle, syncing and downloading from the thinned figures; average and peak CPU and memory in
  each activity state; CPU and memory per MB/s; and restarts and versions. It MUST write a table
  that opens in a spreadsheet and a short text version for the latest month. Missing resource
  figures MUST be left blank, never 0. Re-running it for the same period MUST give the same
  result.
- **FR-016**: All three scripts MUST write each file under a temporary name and rename it only when
  complete, so an interrupted run leaves nothing that looks finished. On any failure they MUST
  exit non-zero with a clear message.
- **FR-017**: The two archive scripts MUST support an optional "keep for N days" setting (default: keep
  forever) that removes only their own archive files older than N days.
- **FR-018**: The scripts MUST NOT need or print the Put.io token or the web UI token. The
  Prometheus address and archive folder MUST be settable by option or environment variable.
- **FR-019**: The scripts MUST run on the NAS (Unraid, from its scheduled user scripts) and on
  the monitoring server, with no install step beyond what each machine already has, or with the
  extra tools named in the docs.

**Alerts** (changes in `~/dev/home-network`, P3)

- **FR-020**: Add alert rules for: high memory, steady memory growth, sustained high CPU, and
  open files near the limit, with the thresholds in US5. They go in the existing `putio-sync`
  alert group and are tested the same way as feature 003's alerts.

**Documentation**

- **FR-021**: The README's metrics section MUST list the new figures, with type and meaning,
  and say which platforms support each one.
- **FR-022**: The scripts MUST be documented: what they save, where, how to schedule them on
  Unraid and on the monitoring server, how to restore, how much disk to expect, and what each
  summary column means.

### Key Entities

- **Resource figure**: a measurement of putio-sync's own process (CPU time, memory, threads,
  open files, disk I/O). It is read fresh at each scrape and kept in Prometheus, never in
  putio-sync's database.
- **Activity state**: what putio-sync was doing at a moment: idle, syncing, or downloading with
  N at once. It is worked out from figures putio-sync already reports, plus the new "sync
  running" figure.
- **Thinned point**: 5 minutes of putio-sync figures: average and peak CPU, memory, threads and
  open files, disk totals, downloads running, bytes downloaded and the activity state. Kept for a
  year in the thinned copy.
- **Metrics archive day**: one compressed file holding the thinned points for one UTC day. It
  records the day, when it was saved, and whether it is partial.
- **Database archive**: one dated, compressed copy of putio-sync's download history, taken while
  putio-sync runs.
- **Usage summary row**: one day or one month of downloads done and resources used, by activity
  state, as listed in US4 scenario 5.
- **Retention settings**: Prometheus keeps full detail for 30 days (unchanged); the thinned copy
  keeps one year by default; archive files are kept forever by default.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The operator can answer "how much memory and CPU has putio-sync used over the last
  30 days, and did it change after the last upgrade?" from one dashboard in under 1 minute,
  without SSH.
- **SC-002**: One year after deploy, putio-sync's memory and CPU for any day in that year can
  be drawn in Grafana at 5-minute detail, with average and peak.
- **SC-002a**: The operator can answer "how much memory and CPU does putio-sync use while idle,
  and while running 1, 2 or 4 downloads at once?" for any month in the last year, from one
  dashboard section or one summary row, in under 1 minute.
- **SC-002b**: The thinned copy adds well under 1 GB a year to the monitoring server's disk,
  and no other app's data is kept longer than 30 days.
- **SC-003**: With the new figures on, the metrics endpoint still answers in under 1 s, and
  download speed drops by no more than 2% compared with before (the same check as feature 003).
- **SC-004**: The daily metrics archive for putio-sync is under 5 MB per day, and the database
  archive is under 10 MB per copy at today's history size.
- **SC-005**: A deleted or rebuilt thinned copy can be refilled from the archive, so that a
  dashboard view of any archived month matches what was there before, at 5-minute detail.
- **SC-005a**: Every figure in the summary for a test week matches the value worked out by hand
  from the same data.
- **SC-006**: Every new alert fires on its synthetic "bad" series and stays silent on a healthy
  one, and none fires on a week of real data after deploy.
- **SC-007**: No new figure, archive file or script output contains a token, file name or path
  from Put.io.

## Assumptions

- The monitoring server's Prometheus and Grafana stay where they are (monitoring01), and stay
  managed in `~/dev/home-network`. This feature does not install Prometheus or Grafana on
  the NAS.
- **Already in place (checked 2026-10-27)**: Prometheus scrapes putio-sync on the NAS every
  15 s; the NAS container publishes port 9742; Grafana has a provisioned `putio-sync`
  dashboard and a `putio-sync` alert group.
- **Has to be set up**: the new resource figures in putio-sync (new image on the NAS); the new
  dashboard and alerts (deploy of the monitoring stack); the thinning job and the place its
  one-year copy lives (US3); and scheduling the archive and summary scripts (an Unraid user script on the NAS and/or a scheduled job on
  the monitoring server). The operator does the NAS and deploy steps; this feature documents
  them.
- Retention belongs in the monitoring stack, not in putio-sync. putio-sync keeps metrics in
  memory only, as feature 003 decided, and does not start storing its own metric history.
- The thinned copy runs on the monitoring server, next to Prometheus. The plan picks where it
  lives: for example as summary series kept by Prometheus-side rules in a small second store
  with a one-year retention. It MUST stay drawable by Grafana. At about 195 series and one
  point every 5 minutes, a year is on the order of tens of MB.
- Archives are kept by default on the NAS, under the putio-sync appdata share, because it has
  about 1.5 TB free and the appdata backup plugin already copies it every week. The folder is a
  setting, so another place can be chosen.
- The figures cover putio-sync's own process. Per-container figures for other containers on the
  NAS (for example via a container exporter) are out of scope, but could be added later in
  `~/dev/home-network`.
- The NAS runs Linux, where every figure in FR-001 can be read. On macOS (development and
  Homebrew installs) some figures, such as disk bytes and the open-file count, may be missing,
  which is allowed by FR-005.
- Showing CPU and memory in putio-sync's own web UI (Status or Stats page) is out of scope. The
  Stats page keeps using the download history only, as feature 006 decided.
- Raising Prometheus retention for every app is out of scope (decided 2026-10-27).
- The summary is about putio-sync's own use. Comparing it with the NAS's total CPU and memory
  is out of scope, since no host exporter runs on the NAS today.
