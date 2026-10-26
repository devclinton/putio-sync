# Feature Specification: Put.io Empty-Folder Cleanup

**Feature Branch**: `005-putio-empty-folder-cleanup`

**Created**: 2026-10-26

**Status**: Draft

**Input**: User description: "Put.io empty-folder cleanup. Remove empty folders from the
user's Put.io account, automatically once a day and on demand."

## Context

- putio-sync downloads everything in the Put.io account (from the root) to the NAS and, with
  `delete_after_download`, deletes each file from Put.io once the local copy is complete. It
  never deletes the folders those files lived in, so empty folders pile up on Put.io.
- The existing `[download] delete_empty_folders` setting only tidies **local** folders after
  a one-off `download` run. Nothing removes empty folders on Put.io.
- The operator runs putio-sync unattended on the NAS (the `daemon` command, with the web UI
  from feature 004). They want the cleanup to happen by itself every day, and to be able to
  run it by hand from the CLI, the API or the web UI.

## Clarifications

### Session 2026-10-26

- Q: Does a folder that holds only empty folders count as empty? → A: Yes. Removal is
  bottom-up within one run: empty leaves first, then any parent left empty by that.
- Q: Which folders are protected? → A: A built-in set, with no extra config: the root, the
  watch upload folder (`watch.parent_folder_id`), `sync.remote_parent_id`, and shared
  folders.
- Q: Dry run? → A: Manual runs only (CLI, API, and the web UI, which previews with a dry
  run to show the count). The daily run has no dry-run mode: when enabled, it deletes.
- Q: When does the daily run happen, and is there a minimum age? → A: At a configurable
  local time, default 03:00. Folders created less than a configurable minimum age (default
  24 hours) ago are kept.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Clean up empty folders by hand from the CLI (Priority: P1) 🎯 MVP

As the operator, I want to run one command that finds every empty folder in my Put.io
account and removes it, with a dry run that shows what it would remove first, so I can clear
the backlog of empty folders today.

**Why this priority**: It delivers the core value (an account without empty folders) and is
the base the daily run, the API and the UI reuse.

**Independent Test**: Against a mocked Put.io account with a mix of empty, nested-empty,
non-empty, young, shared and protected folders, run the command with and without dry run
and check which folders are reported and which deletions are requested.

**Acceptance Scenarios**:

1. **Given** an account with folders `A/` (empty), `B/C/` (both empty) and `D/` (holding a
   file), **When** the operator runs the cleanup, **Then** `A`, `C` and `B` are deleted,
   `D` is kept, and the summary reports 3 folders removed.
2. **Given** the same account, **When** the operator runs the cleanup with dry run, **Then**
   the same 3 folders are listed with their paths, and nothing is deleted.
3. **Given** an empty folder created 2 hours ago and a minimum age of 24 hours, **When** the
   cleanup runs, **Then** that folder is kept and reported as skipped because it is too new.
4. **Given** the watch upload folder is empty, **When** the cleanup runs, **Then** it is
   kept.
5. **Given** a folder `E/` holding only an empty folder `F/` that is too new, **When** the
   cleanup runs, **Then** neither `E` nor `F` is deleted (`E` is not empty while `F` stays).

---

### User Story 2 - Daily automatic cleanup in the daemon (Priority: P2)

As the operator, I want the daemon to run the cleanup once a day at a quiet time, so empty
folders never pile up again without me doing anything.

**Why this priority**: It stops the problem from coming back, but it needs the cleanup from
US1 first.

**Independent Test**: With the schedule enabled and the clock controlled in a test, check
that the run fires once at the configured time, not again the same day, and not at all when
disabled.

**Acceptance Scenarios**:

1. **Given** the cleanup is enabled with run time 03:00, **When** the daemon is running at
   03:00 local time, **Then** one cleanup runs and its summary is logged.
2. **Given** the cleanup is disabled, **When** 03:00 passes, **Then** nothing runs.
3. **Given** the daemon starts at 10:00, **When** it computes the next run, **Then** the
   first run is at 03:00 the next day (no catch-up run at startup).
4. **Given** a run fails part-way (e.g. Put.io errors), **When** the next day comes,
   **Then** the schedule still runs, and the failure was logged and counted.

---

### User Story 3 - Run it from the web UI with a confirm dialog (Priority: P3)

As the operator using the web UI, I want a "Remove empty folders" button on the Put.io page
that tells me how many folders it will delete and asks me to confirm before it deletes
them.

**Why this priority**: Convenience on top of US1; the CLI already covers manual runs.

**Independent Test**: Against the mocked backend, click the button, check the dialog shows
the count from a dry run, cancel (nothing deleted), then confirm (the real run is requested
and the listing refreshes).

**Acceptance Scenarios**:

1. **Given** 5 deletable empty folders, **When** the operator clicks the button, **Then** a
   dialog says 5 folders will be deleted and asks to confirm.
2. **Given** the dialog is open, **When** the operator cancels, **Then** nothing is deleted.
3. **Given** the dialog is open, **When** the operator confirms, **Then** the folders are
   deleted, a result message gives the count removed (and failures, if any), and the file
   listing refreshes.
4. **Given** no empty folders, **When** the operator clicks the button, **Then** they are
   told there is nothing to remove, with no confirm step.

---

### User Story 4 - Run it through the API (Priority: P3)

As a script author or the web UI, I want an authenticated API endpoint that runs the
cleanup (or a dry run) and returns what it found and did.

**Why this priority**: The web UI needs it; it is small once US1 exists.

**Independent Test**: Call the endpoint with and without a valid token and with dry run on
and off against a mocked Put.io account.

**Acceptance Scenarios**:

1. **Given** no or a wrong token, **When** the endpoint is called, **Then** it answers 401
   and nothing is listed or deleted.
2. **Given** a valid token and dry run, **When** the endpoint is called, **Then** it returns
   the folders it would delete (id, path) and the skipped counts, and deletes nothing.
3. **Given** a cleanup is already running (daily or manual), **When** another run is
   requested, **Then** the API answers 409 and does not start a second run.

---

### User Story 5 - Configure the daily run on the Settings page (Priority: P4)

As the operator, I want to turn the daily cleanup on or off and change its run time and
minimum age on the Settings page, so I don't have to edit `config.toml` by hand.

**Independent Test**: Read and update the cleanup settings through the settings API and
check `config.toml` is changed in place with comments kept.

**Acceptance Scenarios**:

1. **Given** the Settings page, **When** it loads, **Then** it shows the enable flag, run
   time and minimum age from the config.
2. **Given** the operator changes the run time to 04:30 and saves, **Then** `config.toml`
   gets the new value, other keys and comments are unchanged, and the daemon uses 04:30 from
   the next scheduled run on.
3. **Given** an invalid run time (e.g. `25:00`), **When** saved, **Then** the save is
   refused with a clear error and the file is unchanged.

---

### Edge Cases

- A folder with more entries than one listing page returns: it is not empty, so it is
  kept. Sub-folders on the first page are still visited; those on later pages are not
  (rare; they are left alone, never deleted unseen).
- Put.io returns an error listing a folder: that folder and all its ancestors are treated as
  not empty for this run (never delete what was not fully seen); the error is logged and
  counted, and the run continues with other branches.
- A delete fails for one folder: it is logged and counted as a failure, its parent is kept
  (it still holds that folder), and the run continues.
- A folder is the save target of a transfer that has not finished: it is kept.
- A shared folder (or anything under the "shared with you" area): kept, and not descended
  into.
- A folder's creation time is missing or cannot be read: kept (treated as too new).
- The account root itself is never a candidate.
- The cleanup runs while the daemon's sync is adding to the download queue: empty folders
  hold no files, so nothing queued can be lost; a file that lands in a folder after it was
  listed is protected by the minimum age for new transfer folders.
- Very deep trees: there is a depth limit; folders below it are treated as not empty.
- The daily time falls in a DST gap or repeats in a DST overlap: it runs once that day.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST find every folder in the Put.io account that contains no
  files, directly or in any sub-folder, walking the whole account from the root.
- **FR-002**: A folder MUST count as empty only if every entry it holds is a folder that is
  itself empty and deletable in the same run (bottom-up). An empty folder is removed
  together with its empty sub-folders; a kept folder (protected, too new, unreadable) is
  never inside a removed one.
- **FR-003**: The system MUST never delete: the root; the watch upload folder
  (`watch.parent_folder_id`); `sync.remote_parent_id`; shared folders and anything under
  them; the save folder of any transfer that has not finished.
- **FR-004**: The system MUST keep folders created less than the minimum age ago (default
  24 hours, configurable), and folders whose creation time is unknown. A kept folder makes
  its parents non-empty.
- **FR-005**: The system MUST support a dry run that reports exactly what a real run would
  delete, and deletes nothing.
- **FR-006**: Each run MUST produce a summary: folders scanned, folders deleted (or that
  would be), folders kept by reason (protected, too new), and failures. Each deleted folder
  MUST be logged with its id, path and age.
- **FR-007**: Only one cleanup MUST run at a time across the daily schedule, the API and the
  web UI in one process; a second request while one runs MUST be refused.
- **FR-008**: The CLI MUST offer a subcommand that runs the cleanup, with a dry-run flag,
  printing the folders and the summary.
- **FR-009**: The API MUST offer an authenticated endpoint that runs the cleanup or a dry
  run and returns the summary and the list of folders (id and path).
- **FR-010**: The web UI's Put.io page MUST offer a button that runs a dry run, shows a
  confirm dialog with the folder count, and on confirm runs the real cleanup and shows the
  result.
- **FR-011**: The daemon MUST run the cleanup once a day at a configurable local time
  (default 03:00) when the cleanup is enabled (default off). It MUST NOT catch up missed
  runs at startup.
- **FR-012**: The enable flag, run time and minimum age MUST live in `config.toml`, be
  documented in `config.example.toml`, be validated on load and save, and be readable and
  editable on the Settings page. A change made from the Settings page MUST apply from the
  next scheduled run without a restart.
- **FR-013**: The system MUST expose metrics for cleanup runs (by outcome), folders deleted,
  delete failures and the time of the last successful run, following the existing metric
  naming rules (bounded labels, no folder names or ids).
- **FR-014**: Tokens and secrets MUST NOT appear in cleanup logs, errors or API responses.

### Key Entities

- **Cleanup run**: one pass over the account. Attributes: dry run or not, trigger (daily,
  CLI, API), start and end time, summary counts, list of removed (or would-be removed)
  folders, failures.
- **Removed folder**: id, path from the root, age at removal.
- **Cleanup settings**: enabled, daily run time (HH:MM, local), minimum age in hours.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: After one real run, the account has no deletable empty folders: a dry run
  straight after reports 0 folders.
- **SC-002**: In tests covering every protected kind (root, upload folder, sync folder,
  shared, active transfer target, too new, unknown age), 0 protected folders are deleted.
- **SC-003**: A dry run's folder list equals the real run's deletions on the same account
  state in 100% of test cases.
- **SC-004**: With the schedule enabled, exactly one run happens per day; with it disabled,
  none.
- **SC-005**: From the Put.io page, the operator can clear empty folders in two clicks
  (button, confirm), seeing the count before anything is deleted.
- **SC-006**: All tests run with no network access to Put.io and no real token.

## Assumptions

- "Shared" folders are those Put.io marks as shared (`is_shared`); their contents belong to
  someone else's share and are never touched.
- Put.io's delete may move folders to the account trash if trash is on; that is Put.io's
  behaviour and is fine.
- Local time is the process's local time zone (`TZ` in the container). If `TZ` is unset the
  container runs in UTC, so 03:00 means 03:00 UTC; the README says so.
- The daily run is off by default so upgrading does not start deleting folders without the
  operator choosing it.
- The CLI subcommand talks to Put.io directly (like the other CLI Put.io commands); it does
  not need the daemon running, so the one-at-a-time guard (FR-007) covers the daemon process
  (schedule, API, UI) only.
- The existing `[download] delete_empty_folders` (local folders) is unrelated and unchanged.
- Out of scope: deleting non-empty folders, a user-configurable exclude list, a dry-run mode
  for the daily run, and deploying to the NAS.
