# Feature Specification: Put.io Doctor

**Feature Branch**: `010-putio-doctor`

**Created**: 2026-10-26

**Status**: Draft

**Input**: User description: "a putio doctor would be nice as well". The operator was offered a
list of candidate checks and chose all of them, with safe one-click fixes that preview anything
destructive and ask for confirmation.

## Context

- When putio-sync misbehaves today, the operator pieces the cause together from the live log,
  the Put.io website and the NAS shell. Typical findings so far: queue rows for files deleted
  on Put.io (logged as "File … no longer exists on Put.io"), transfers frozen at 97%+, empty
  folders piling up, a full Put.io account, and config surprises such as `skip_existing`.
- Several building blocks already exist: stalled-transfer detection and Re-add (feature 007),
  empty-folder cleanup with a dry run (feature 005), transfer retry, the download queue and
  history, and the web UI with token authentication (feature 004).
- The doctor brings these together in one place: run every check, say in plain English what is
  wrong and why it matters, and offer the fix where one is safe.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Run the doctor from the command line (Priority: P1)

The operator runs `putio-sync doctor` on the NAS (or inside the container) and gets a list of
checks grouped by area (Put.io, Sync state, Local, Connectivity). Each check shows OK, Warning
or Problem, one sentence saying what was found, and, when something is wrong, what to do about
it. With `--json` the same report comes out as machine-readable JSON. The exit code tells a
script whether anything needs attention.

**Why this priority**: Works without the daemon or the web UI running, which is exactly when
it is needed most (the daemon will not start, the UI is unreachable).

**Independent Test**: With a mocked Put.io and a temporary config, database and download
directory set up with known faults, run the command and check every expected check result,
the JSON shape, and the exit code.

**Acceptance Scenarios**:

1. **Given** a healthy setup, **When** the operator runs `putio-sync doctor`, **Then** every
   check reads OK and the exit code is 0.
2. **Given** a queue row whose Put.io file has been deleted, **When** the doctor runs, **Then**
   the dead-queue-rows check is a Warning that names the file and says the row can be removed.
3. **Given** an invalid Put.io token, **When** the doctor runs, **Then** the authentication
   check is a Problem explaining that the token was rejected and how to log in again, the
   checks that need Put.io are reported as skipped with that reason, and the local checks still
   run.
4. **Given** `--json`, **When** the doctor runs, **Then** the output is a single JSON document
   with an overall status and one entry per check (id, area, title, status, summary, details,
   available fixes), and nothing else is printed to standard output.
5. **Given** at least one Problem, **When** the doctor finishes, **Then** the exit code is 2;
   with Warnings but no Problems it is 1.

---

### User Story 2 - See the doctor report in the web UI and apply fixes (Priority: P1)

The operator opens the Doctor page in the web UI. It shows the latest report (with when it ran),
grouped by area, with a summary at the top ("2 problems, 3 warnings"). Each check that is not OK
can be expanded to show the affected items. Where a fix is available, a button applies it. A
"Run now" button runs every check again.

**Why this priority**: The UI is where the operator already looks; one-click fixes are the main
value beyond what the log offers.

**Independent Test**: Against the mock UI backend with seeded faults, open the Doctor page,
check the summary and the expanded items, apply a non-destructive fix, and walk through the
preview-and-confirm flow of a destructive one.

**Acceptance Scenarios**:

1. **Given** a report exists, **When** the operator opens the Doctor page, **Then** they see the
   overall summary, the time of the run, and each check with its status and summary.
2. **Given** a failed transfer, **When** the operator clicks its Retry fix, **Then** the
   transfer is retried and the check is re-run and shown as updated.
3. **Given** dead queue rows, **When** the operator clicks Remove, **Then** a preview lists the
   rows that will be removed, and nothing changes until they confirm.
4. **Given** empty folders on Put.io, **When** the operator chooses the cleanup fix, **Then** a
   dry run is run first and its list shown, and the real cleanup only runs after they confirm.
5. **Given** the operator dismisses a confirmation, **When** they return to the page, **Then**
   nothing was changed.
6. **Given** a request without a valid UI token, **When** it reaches the doctor endpoints,
   **Then** it is refused.

---

### User Story 3 - Alert on doctor results (Priority: P2)

The daemon runs the doctor on a schedule and publishes the result of each check as a metric, so
the monitoring stack can alert when a check turns into a Problem without anyone opening the UI.

**Why this priority**: The NAS runs unattended; trouble should be noticed before downloads stop.

**Independent Test**: Run the scheduled doctor once against seeded faults and scrape the
metrics endpoint: each check has a status value, a count of affected items, and the time of the
last run.

**Acceptance Scenarios**:

1. **Given** the daemon is running with the doctor enabled, **When** the interval passes,
   **Then** the doctor runs and the metrics reflect the new results.
2. **Given** a check that is a Problem, **When** metrics are scraped, **Then** that check's
   status metric reads 2 (Warning 1, OK 0, skipped -1), labelled only by check id.
3. **Given** the doctor disabled in config, **When** the daemon runs, **Then** no scheduled run
   happens, but on-demand runs from the CLI and UI still work.

---

### User Story 4 - Apply fixes from the command line (Priority: P3)

The operator runs `putio-sync doctor --fix <fix-id>` to apply a fix listed in the report. A
fix that removes or deletes something prints the preview and asks for confirmation; `--dry-run`
shows the preview only; `--yes` skips the question for scripting.

**Why this priority**: Useful on a headless box, but the UI covers the same fixes.

**Independent Test**: Run a removal fix with `--dry-run` and check nothing changed; run it with
`--yes` and check exactly the previewed items were removed.

**Acceptance Scenarios**:

1. **Given** dead queue rows, **When** the operator runs the remove fix with `--dry-run`,
   **Then** the rows are listed and none are removed.
2. **Given** the same, **When** they run it without `--yes` and answer no, **Then** nothing is
   removed.
3. **Given** a fix id that does not exist or no longer applies, **When** it is requested,
   **Then** the command says so and exits non-zero without changing anything.

### Edge Cases

- Put.io is unreachable or the token is rejected: Put.io checks are reported as skipped with
  the reason; local and sync-state checks that do not need Put.io still run.
- A check itself fails unexpectedly (for example a listing error part-way through a walk): that
  check is reported as a Problem saying it could not complete and why; the other checks run.
- A very large Put.io account: one file-tree walk per doctor run is shared by the duplicate,
  orphaned-torrent and dead-queue checks (the empty-folder check uses the feature 005 dry run),
  and the run has an overall time limit; a check that runs out of time says so instead of
  reporting a partial result as complete.
- A fix is applied to a stale report (the item was already handled): the fix re-validates
  each item before acting and reports items that no longer apply as skipped.
- Two runs requested at once (UI "Run now" while the scheduled run is in progress): the second
  request waits for and returns the running one instead of starting another.
- The daemon is downloading while the orphan-partial-file check runs: partial files that belong
  to a queued or in-progress download, or that changed in the last hour, are never reported.
- Item lists can be long: each check reports a total count and shows at most 100 items.

## Requirements *(mandatory)*

### Functional Requirements

**Report**

- **FR-001**: The doctor MUST run a fixed set of checks and report each with: a stable id, its
  area (Put.io, Sync state, Local, Connectivity), a title, a status (OK, Warning, Problem or
  Skipped), a one-sentence plain-English summary, a suggestion when the status is not OK, the
  affected items (total count plus up to 100 entries), and the fixes available.
- **FR-002**: The report MUST carry an overall status (the worst check status, ignoring
  Skipped), counts per status, the time it started and how long it took.
- **FR-003**: A check that needs something that is unavailable (no valid token, Put.io
  unreachable, watch mode disabled) MUST be reported as Skipped with the reason, and MUST NOT
  block the other checks.

**Put.io checks**

- **FR-010 (auth)**: Verify that the configured token is accepted by Put.io. Problem if it is
  rejected or missing, with instructions to log in again.
- **FR-011 (storage)**: Report account storage use. Warning at 90% or more, Problem at 98% or
  more; the summary states used and total space.
- **FR-012 (transfers)**: Report transfers in the error state and transfers currently stalled
  (using the feature 007 detection). Fixes: Retry for errored transfers; Re-add for stalled
  transfers that have a magnet link (confirmation required, as in feature 007).
- **FR-013 (empty folders)**: Report the empty folders the feature 005 cleanup would remove,
  using its rules (age, protected and shared folders, unfinished transfer targets). Fix: run
  the cleanup, always previewed by a dry run and confirmed.
- **FR-014 (duplicates)**: Report files on Put.io that share the same name and size in more
  than one place. Report only, no fix.
- **FR-015 (orphaned torrents)**: Report `.torrent` files stored in the Put.io account that no
  transfer was created from. Report only, no fix.
- **FR-016 (stuck watch files)**: When watch mode is on, report `.torrent` and `.magnet` files
  in the local watch folders that have been there for more than one hour without being turned
  into a transfer. Report only, no fix; the summary points at the log for the upload error.

**Sync state checks**

- **FR-020 (dead queue rows)**: Report queue rows whose Put.io file no longer exists. Fix:
  remove those rows (preview and confirmation required).
- **FR-021 (failing downloads)**: Report Put.io files whose download is in the failed state or
  has failed 3 or more times. Fix: put them back in the queue for another attempt.
- **FR-022 (orphan partial files)**: Report partial download files under the download directory
  that belong to no queued or in-progress download and have not changed for at least one hour.
  Fix: delete those files (preview and confirmation required).

**Local checks**

- **FR-030 (download directory)**: Verify the download directory exists and is writable, by
  creating and removing a probe file. Problem otherwise.
- **FR-031 (disk space)**: Report free space on the download directory's disk. Warning below
  10% free or below the size of the pending queue; Problem below 2% free.
- **FR-032 (database)**: Run the database's built-in integrity check. Problem if it reports
  anything.
- **FR-033 (config)**: Report unknown keys in the config file (likely typos, which are ignored
  today) and settings known to be risky, starting with an explicit `skip_existing = false`
  (re-downloads files already on disk). Warning per finding; report only.

**Connectivity checks**

- **FR-040 (reachability)**: Measure a lightweight Put.io API call. Problem if it fails;
  Warning if it takes more than 2 seconds.
- **FR-041 (rate limiting)**: Warning if Put.io answered any doctor request with "too many
  requests", or reports rate-limit headroom below 10%. OK with "not reported" when Put.io gives
  no rate-limit information.
- **FR-042 (clock skew)**: Compare the local clock with the time in Put.io's response. Warning
  at more than 1 minute of difference, Problem at more than 5 minutes.

**Fixes**

- **FR-050**: Every fix MUST be listed in the report with an id, a label and whether it is
  destructive. Destructive means it removes or deletes something (queue rows, local files,
  Put.io folders) or loses progress (Re-add).
- **FR-051**: A destructive fix MUST first return a preview of exactly what it will act on, and
  MUST act only when confirmed; when confirmed, it acts only on items from that preview.
- **FR-052**: Before acting on each item, a fix MUST check that the item still qualifies, and
  report items that no longer qualify as skipped.
- **FR-053**: After a fix, the result MUST say what was done, what was skipped and what failed,
  and the affected check MUST be re-run.
- **FR-054**: Every fix applied MUST be logged with the fix id, item count and outcome.

**Surfaces**

- **FR-060**: A `putio-sync doctor` command MUST run the doctor and print a readable report;
  `--json` MUST print the report as JSON only. Exit code: 0 all OK, 1 warnings, 2 problems.
- **FR-061**: `putio-sync doctor --fix <fix-id>` MUST apply a fix, supporting `--dry-run` and
  `--yes` as described in User Story 4.
- **FR-062**: The HTTP API MUST offer, behind the existing UI token: read the latest report,
  run the doctor now, preview a fix, and apply a fix.
- **FR-063**: The web UI MUST have a Doctor page showing the report, Run now, and the fix
  buttons with preview and confirmation for destructive fixes.
- **FR-064**: The daemon MUST run the doctor on an interval (default every 60 minutes),
  configurable along with an enable flag, documented in the example config. The first run
  happens shortly after start-up.
- **FR-065**: The metrics endpoint MUST expose, per check id: its status (OK 0, Warning 1,
  Problem 2, Skipped -1) and its affected item count; plus the time and duration of the last
  run. Labels are limited to the check id.

**Safety**

- **FR-070**: The doctor's checks MUST be read-only; only an explicitly applied fix changes
  anything.
- **FR-071**: Tokens, magnet links and other secrets MUST NOT appear in the report, the logs or
  the metrics.

### Key Entities

- **Doctor report**: one run of all checks: start time, duration, overall status, counts, and
  the list of check results. The latest report is kept in memory by the daemon.
- **Check result**: id, area, title, status, summary, suggestion, item count, items, fixes.
- **Item**: one affected thing (a transfer, Put.io file or folder, queue row, local file,
  config key) with a short description and the identifier a fix needs.
- **Fix**: id, label, destructive flag, the check it belongs to; previewing gives the item list
  it would act on, applying gives done / skipped / failed counts.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The operator can find out what is wrong with their setup with one command or one
  page, without reading the log or visiting Put.io, for every fault listed in the requirements.
- **SC-002**: Every check that can be wrong is covered by an automated test that seeds that
  fault and expects the right status, using a mocked Put.io.
- **SC-003**: A full doctor run on an account of a few thousand files finishes in under one
  minute.
- **SC-004**: No destructive fix changes anything without a preview and a confirmation, in the
  UI or the CLI; this is covered by automated tests.
- **SC-005**: A Problem on any check can trigger an alert in the monitoring stack without
  anyone opening the UI.

## Assumptions

- "Duplicate" means same name and same size, for files of at least 1 MB (so small extras such
  as release notes and samples are not reported); content hashes are not compared. Duplicates,
  orphaned torrents and stuck watch files get no fix in this version: deleting from Put.io
  needs a judgement call the doctor cannot make, so it only points at them.
- "Token scope": Put.io tokens have no scope the API can report, so the auth check verifies the
  token is accepted and the account can be read.
- An orphaned `.torrent` is one stored in the account whose name matches no current or recent
  transfer's name.
- The CLI runs the checks itself against the config and database; it does not need the daemon.
  The daemon's schedule only feeds the UI and the metrics.
- The thresholds above are fixed defaults in this version; only the schedule and enable flag
  are configurable.
- Alert rules are configured in the home-network monitoring stack, not in this repository.
