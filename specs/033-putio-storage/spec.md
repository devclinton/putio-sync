# Feature Specification: Put.io Storage Management

**Feature Branch**: `033-putio-storage` (built on `claude/quizzical-mcclintock-8a20a5`)

**Created**: 2026-11-03

**Status**: Draft

**Input**: User description: "033 Put.io storage management. Show Put.io storage use and
what fills it. Warn when free space is low, and optionally delete the oldest files that are
already downloaded and verified until a target amount is free, with a preview and a list of
what was deleted."

## Context

- A Put.io account has a fixed storage size. When it is full, new transfers fail. putio-sync
  downloads files to the NAS, and with `delete_after_download` off (the default) the Put.io
  copy stays, so the account fills up over time with files that are already safe locally.
- Put.io reports the account's size, used and free bytes. Each folder in a listing carries
  the total size of everything in it.
- The doctor (010) already has a "Put.io storage" check with fixed levels (warning at 90%
  used, problem at 98%). Nothing else in the UI shows storage use.
- Every finished download writes a `completed` record with the Put.io file id, its size and
  the local path. The queue knows which files are still pending or downloading.
- Empty-folder cleanup (005) already deletes empty folders account-wide with a dry run first,
  a confirm step, and an opt-in daily run; it protects the root, the watch upload folder, the
  sync folder, shared folders and the save folders of unfinished transfers. Deleting files
  leaves folders empty; that cleanup, not this feature, removes them.
- Put.io bulk delete (008) and the files page let a user delete chosen files by hand.
- 013's setup mode: an unconfigured daemon serves only the setup wizard; other routes 503.

## Decisions

- Q: Should freeing space also run on its own? → A (user): yes, as an opt-in daily run, off
  by default, that runs only when free space is below the warning level. The manual run is
  always a preview first, then a confirm.
- Q: The UI bundle has almost no room left under its size cap. → A (user): raise the cap to
  384 KB.
- Q: What counts as "downloaded and verified"? → A: a Put.io file with a completed download
  record for the same file id and the same size, whose local file still exists at the
  recorded path with that same size. Content checks (CRC32) belong to 034; this feature uses
  the checks the downloader already made when it marked the download complete.
- Q: Which files are "oldest"? → A: by the date the file was added to Put.io, oldest first;
  ties by file id.
- Q: How are the levels set? → A: as percentages of the account size, so the defaults work
  for any plan: warn when free space is below 10%, and free up to 20% free.
- Q: What does the confirm step delete? → A: exactly the files the preview listed (by id),
  each checked again just before deletion; it never picks new files the user did not see.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See storage use and what fills it (Priority: P1)

On the Put.io page a storage card shows how much of the account is used, how much is free,
and the account size, as a bar and numbers. Below it the largest items at the top level of
the account are listed by size, each with its share of the used space, and each opens its
folder in the files list.

**Why this priority**: the user cannot act on storage they cannot see; every other story
builds on this view.

**Independent Test**: with a mocked Put.io account of 100 GB with 80 GB used in three top
folders, open the Put.io page: the card shows 80 GB used of 100 GB, 20 GB free, and the
three folders largest first with their sizes and shares.

**Acceptance Scenarios**:

1. **Given** a signed-in user on the Put.io page, **Then** the storage card shows used,
   free and total space and the percentage used.
2. **Given** top-level items of different sizes, **Then** the largest items are listed
   largest first (up to 10), each with its size and share of used space, and clicking a
   folder opens it in the files list.
3. **Given** Put.io can't be reached, **Then** the card says storage could not be read and
   the rest of the page still works.

---

### User Story 2 - Be warned when free space is low (Priority: P1)

When free space falls below the warning level, the storage card shows a warning, the Status
page shows a "Put.io storage low" notice that links to the card, and the doctor's Put.io
storage check reports a warning using the same level. The current numbers are also exported
as metrics so the home monitoring can alert on them.

**Why this priority**: a full account silently breaks new transfers; the warning is the
cheapest way to prevent that.

**Independent Test**: set the warning level to 10% and mock 95 GB used of 100 GB: the card
and the Status page show the low-space warning and the doctor's storage check is a warning.
Mock 50 GB used: no warning anywhere.

**Acceptance Scenarios**:

1. **Given** free space below the warning level, **Then** the storage card shows a warning
   with the free amount and the level, and offers "Free up space".
2. **Given** free space below the warning level, **Then** the Status page shows a notice
   linking to the storage card.
3. **Given** the doctor runs, **Then** its Put.io storage check is a warning when free space
   is below the warning level and a problem when the account is nearly full (under 2% free),
   and its suggestion names the "Free up space" action.
4. **Given** the metrics endpoint, **Then** it exposes the Put.io account size and used
   bytes, refreshed at least every 15 minutes while the daemon runs.

---

### User Story 3 - Free space by deleting old downloaded files (Priority: P2)

The user clicks "Free up space". putio-sync lists the oldest files that are already
downloaded and verified, oldest first, until deleting them would bring free space up to the
target, and shows the list with each file's path, size, date added and local path, the total
it would free, and the free space after. The user confirms, and putio-sync deletes exactly
those files from Put.io and shows what was deleted.

**Why this priority**: the action that actually solves a full account; it depends on the
view (US1) but is the main value of the feature.

**Independent Test**: mock an account 95% full with five downloaded files (with local copies
present) and two files not downloaded, target 20% free: the preview lists only downloaded
files, oldest first, just enough to reach 20% free; confirming deletes exactly those, and
nothing is deleted if the user cancels.

**Acceptance Scenarios**:

1. **Given** free space below the target, **When** the user asks to free up space, **Then**
   a preview lists the oldest eligible files, oldest first, stopping as soon as the listed
   sizes bring free space to the target, and shows the total and the resulting free space.
   Nothing is deleted by the preview.
2. **Given** a preview, **When** the user confirms, **Then** exactly the listed files are
   deleted from Put.io, each checked again just before, and a result lists every file
   deleted, every file skipped (with why), and the space freed.
3. **Given** a preview, **When** the user cancels, **Then** nothing is deleted.
4. **Given** a file that is not downloaded, is downloaded with a different size, whose local
   copy is missing or a different size, or that is pending, downloading or paused in the
   queue, **Then** it is never listed or deleted.
5. **Given** a file in a shared folder, or in the save folder of an unfinished transfer,
   **Then** it is never listed or deleted.
6. **Given** not enough eligible files to reach the target, **Then** the preview lists all
   eligible files and says how far short of the target it falls.
7. **Given** free space already at or above the target, **Then** the preview says nothing
   needs to be deleted.
8. **Given** the command line, **Then** the same run is available with a dry-run option that
   only prints the preview, and otherwise asks for confirmation unless told not to.

---

### User Story 4 - See what was deleted (Priority: P2)

Every run that deleted files (manual or automatic) is recorded: when, what started it, how
much it freed, and each file deleted (path, size, date added, local path). The storage card
lists recent runs, and each run expands to its files. Deleted files also appear in History
as "deleted".

**Why this priority**: deleting a user's files must leave a trail they can check; it also
lets them confirm the local copies are where they expect.

**Independent Test**: run a free-up that deletes two files, reload the Put.io page: the
recent runs list shows the run with two files and the bytes freed; History shows both files
as deleted.

**Acceptance Scenarios**:

1. **Given** a finished run that deleted files, **Then** it appears in the recent runs list
   with its time, trigger (manual or automatic), file count and bytes freed.
2. **Given** a run in the list, **When** the user opens it, **Then** every file it deleted is
   shown with path, size, date added and local path.
3. **Given** a deleted file, **Then** History shows a "deleted" entry for it.
4. **Given** a preview or a run that deleted nothing, **Then** nothing is recorded.

---

### User Story 5 - Free space automatically (Priority: P3)

The user can turn on an automatic run (off by default). Once a day, at a chosen time, if
free space is below the warning level, putio-sync deletes the oldest eligible files until
the target is free, using the same rules as the manual run, and records the run like a
manual one. The setting lives with the other storage settings in the settings page and the
config file.

**Why this priority**: convenience on top of the manual run; off by default because it
deletes files without asking.

**Independent Test**: turn the automatic run on with a mocked account below the warning
level: at the run time it deletes the eligible files and records an "automatic" run; with
the account above the warning level it deletes nothing; with it turned off nothing runs.

**Acceptance Scenarios**:

1. **Given** the automatic run is off (the default), **Then** nothing is ever deleted
   without the user confirming a preview.
2. **Given** it is on and free space is below the warning level at the run time, **Then** a
   run frees space up to the target and is recorded as automatic.
3. **Given** it is on and free space is at or above the warning level, **Then** nothing is
   deleted and the check is logged.
4. **Given** a manual run is going when the automatic run starts (or the other way round),
   **Then** the second one is refused and says a run is already going.

### Edge Cases

- The same file was downloaded twice to different places: it is eligible if any completed
  record's local copy is present with the right size.
- A file was renamed or moved on Put.io after it was downloaded: it is matched by file id,
  so it stays eligible.
- The local download folder is not mounted (the NAS share is missing): local copies are not
  found, so nothing is eligible and nothing is deleted.
- Put.io's free space does not grow after deletion (for example while Put.io moves files to
  its trash): the run reports the space it deleted by file sizes and the free space Put.io
  reports afterwards; it does not delete more to make up the difference.
- A file listed in the preview is deleted or changed elsewhere before the confirm: it is
  skipped with a reason; the rest go ahead.
- A deletion fails for one file: it is recorded as failed, the rest go ahead, and the run's
  result shows the failure.
- The account is huge: the scan stops at a time limit and says it was cut short; files not
  seen are not listed.
- Put.io reports a size of zero or no size: the card shows used space only, levels are not
  applied, and the automatic run does nothing.
- The setup wizard is running (013): the storage routes answer like other routes in setup
  mode.
- Deleting files can leave empty folders; they stay until empty-folder cleanup removes them.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST report the Put.io account's size, used and free bytes, and the
  percentage used.
- **FR-002**: The system MUST report the largest top-level items of the account (up to 10),
  largest first, with size and share of used space, and whether each is a folder.
- **FR-003**: The system MUST have a warning level (default: free space below 10% of the
  account size) and a target (default: 20% free), both percentages of the account size,
  settable in the config file and the settings page; the target MUST be above the warning
  level.
- **FR-004**: When free space is below the warning level, the storage card and the Status
  page MUST show a low-space warning, and the doctor's Put.io storage check MUST report a
  warning (a problem under 2% free) using the configured level, suggesting "Free up space".
- **FR-005**: The daemon MUST export the account size and used bytes as metrics, refreshed
  at least every 15 minutes.
- **FR-006**: A file MUST be eligible for deletion only if all hold: it is a file (not a
  folder); it is not shared and not inside a shared folder; it is not inside the save folder
  of an unfinished transfer; it has a completed download record with the same file id and
  size; that record's local file exists with that size; and the file is not pending,
  downloading or paused in the queue.
- **FR-007**: A preview MUST list eligible files oldest first (date added to Put.io, then
  file id) and stop at the first file that brings planned free space to the target; it MUST
  show for each file its Put.io path, size, date added and local path, plus the total, the
  free space after, and any shortfall. A preview MUST NOT delete anything.
- **FR-008**: Confirming MUST delete only the files the preview listed, by id; each MUST be
  checked against FR-006 again just before deletion, and any that fail the check MUST be
  skipped with a reason.
- **FR-009**: The result of a run MUST list every deleted file, every skipped or failed file
  with its reason, the bytes deleted, and the free space Put.io reports afterwards.
- **FR-010**: Every run that deleted at least one file MUST be recorded with its time,
  trigger (manual, automatic), bytes freed and each deleted file (Put.io id, path, size, date
  added, local path), and each deleted file MUST also be added to History as deleted.
- **FR-011**: The UI MUST list the recent runs (newest first) and show each run's files.
- **FR-012**: An automatic daily run MUST exist, off by default, at a configured local time;
  when on, it MUST free space to the target only when free space is below the warning level,
  using the same eligibility and recording as the manual run.
- **FR-013**: Only one free-up run (manual or automatic) MUST run at a time; a second MUST
  be refused with a clear message.
- **FR-014**: The command line MUST offer the free-up run with a dry-run option and a
  confirmation prompt that can be skipped.
- **FR-015**: All storage routes MUST require the same sign-in as the rest of the API and
  MUST answer like other routes in setup mode.
- **FR-016**: Runs MUST be logged with structured fields (trigger, files, bytes, failures)
  and counted in metrics (runs by trigger and result, files and bytes deleted).
- **FR-017**: The UI bundle size cap MUST be raised to 384 KB.

### Key Entities

- **Storage summary**: the account size, used and free bytes, the percentage used, whether
  free space is below the warning level, and the largest top-level items.
- **Storage settings**: warning level and target (percent free), automatic run on/off and
  its time.
- **Candidate**: a Put.io file that passes the eligibility rules, with its path, size, date
  added and local path.
- **Free-up plan (preview)**: the ordered candidates chosen to reach the target, their
  total, the free space after, any shortfall, and whether the scan was complete.
- **Free-up run**: a confirmed or automatic run: when, trigger, files deleted, skipped and
  failed, bytes freed. Recorded only when it deleted something.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A user sees used, free and total Put.io storage and the biggest items within
  one page load of the Put.io page.
- **SC-002**: When free space is below the warning level, the warning shows on the Put.io
  page, the Status page and the doctor, in 100% of tested cases; above it, none of them
  shows it.
- **SC-003**: In every tested case, no file without a verified local copy, and no file the
  user did not see in the preview, is deleted.
- **SC-004**: A user can go from "storage low" to space freed in three clicks (Free up
  space, review, confirm).
- **SC-005**: Every deleted file can be found afterwards in the recent runs list and in
  History.
- **SC-006**: With default settings, nothing is ever deleted without a confirmed preview.

## Assumptions

- Put.io's account info is the source of the size, used and free numbers; folder sizes in
  listings are the source of what fills it.
- Deleting through Put.io's API frees the space (Put.io may keep deleted files in its trash
  for a while; emptying the trash is out of scope).
- The scan for candidates walks the account like the doctor's walk, with a time limit.
- Notifications for low storage are part of 030 and out of scope here.
- Keeping chosen folders out of empty-folder cleanup is 021 and out of scope.
- The storage card is part of the Put.io page in the existing no-build web UI.
