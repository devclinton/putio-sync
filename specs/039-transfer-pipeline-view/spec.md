# Feature Specification: Active Transfers Pipeline View

**Feature Branch**: `039-transfer-pipeline-view`

**Created**: 2026-10-27

**Status**: Draft

**Input**: User description: "we should have other feature to track active transfers by state in
putio in dashboard and then locally (you can be downloading there and then you can also by
syncing(downloading) to our machine too)". Meaning: one web UI view that follows each active
transfer through both of its stages, first on Put.io and then down to the NAS.

## Context

- Getting a release onto the NAS takes two downloads. First Put.io downloads the torrent
  (stage 1). Then putio-sync downloads the finished files from Put.io to the NAS (stage 2).
- Today the two stages live on different pages, and nothing links them:
  - The **Transfers** page lists Put.io transfers with state and progress, and the Stalled
    badge from feature 007. It says nothing about the local download.
  - The **Queue** page and the Status page's **Active downloads** card list local files being
    downloaded. They show single files, not the transfer they came from, so a 40-file season
    pack shows as 40 unrelated rows.
  - The Status page's **Queue** card counts local files by state. Nothing counts Put.io
    transfers by state.
- Pausing: Put.io has no pause for a transfer, only cancel. The local queue has Pause all and
  Resume all, but nothing for one transfer or one file.
- Between the stages there is a gap: a transfer finishes on Put.io, but its files are only
  picked up at the next sync run. Today no page shows that a finished transfer is waiting.
- Feature 011 already follows Radarr/Sonarr items through the same two stages (Put.io, then
  local, then done or failed), but it reports them to the *arr apps, not in putio-sync's UI.
- Feature 038 (queued) will add steps after the local download (hooks, subtitles,
  translation). This view covers the two download stages only. 038 can later add its steps
  after stage 2.

## Clarifications

### Session 2026-10-27

- Q: Should the Status page's "Active downloads" card stay as a list of local files with a link
  to the new page, or be replaced by a short per-transfer summary? → A: Replace it with a short
  per-transfer summary (FR-030). The user thinks in transfers, not files. A season pack would
  otherwise fill the card with dozens of file rows, and the Status and Active pages would
  describe the same downloads in two different ways.
- Q: Should the Queue page's "Pause all" / "Resume all" become the new lasting Pause NAS
  downloads switch? → A: Yes (FR-020a). There is one meaning of "pause everything".

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See every active transfer and which stage it is in (Priority: P1)

The operator opens a new **Active** page in the web UI. Each active transfer has one row. The
row shows its name, which stage it is in (Put.io or local), its state in that stage, and the
progress, speed and time left for that stage. A transfer still on Put.io shows Put.io's
figures. Once Put.io finishes, the same row moves to the local stage and shows the NAS
download's figures for all of its files together. When every file is on the NAS, the row is
marked done.

**Why this priority**: This answers "where is my download?" on one page, without matching
names across the Transfers and Queue pages by hand.

**Independent Test**: Against a mocked Put.io, add a transfer and move it through these steps:
queued, downloading, finishing, completed, then local files queued, downloading and all done.
Open the page after each step. Check that one row exists the whole time, that its stage and
state change as expected, and that progress, speed and time left come from the right stage.

**Acceptance Scenarios**:

1. **Given** a transfer that is downloading on Put.io, **When** the operator opens the page,
   **Then** one row shows it in the Put.io stage with Put.io's progress, speed and time left.
2. **Given** a transfer that has finished on Put.io and whose files are not yet queued
   locally, **When** the page refreshes, **Then** the row shows "Waiting for sync" in the local
   stage, with when the next sync run is due.
3. **Given** a finished transfer with 12 files, of which 4 are on the NAS, 1 is downloading and
   7 are queued, **When** the page refreshes, **Then** the row shows the local stage, "4 of 12
   files", progress for all 12 files by size, the current local speed and time left.
4. **Given** every downloadable file of a transfer is on the NAS, **When** the page refreshes,
   **Then** the row is marked done and moves out of the active list (see User Story 6).
5. **Given** a transfer that fails on Put.io, or a transfer with a local file that failed,
   **When** the page refreshes, **Then** the row shows the failed state and the reason.
6. **Given** nothing is active, **When** the operator opens the page, **Then** it says so
   plainly, with a link to the Transfers page to add one.

---

### User Story 2 - Counts by state for each stage (Priority: P1)

At the top of the page, two rows of counts give the overview at a glance:

- **On Put.io**: Queued, Downloading, Stalled, Finishing, Error. (A seeding transfer is already in the NAS stage; see Edge Cases.)
- **On the NAS**: Waiting for sync, Queued, Downloading, Paused, Failed.

Each count can be clicked to show only the transfers in that state.

**Why this priority**: The operator often only wants to know "is anything stuck or failed?".
Counts answer that without reading every row.

**Independent Test**: Against a mocked Put.io with transfers in each Put.io state, and local
files in each local state, open the page. Check that each count equals the number of rows in
that state, and that clicking a count filters the list to those rows.

**Acceptance Scenarios**:

1. **Given** 2 transfers downloading and 1 in error on Put.io, and 1 transfer downloading
   locally, **When** the page opens, **Then** the counts read Downloading 2 and Error 1 on
   Put.io, and Downloading 1 on the NAS.
2. **Given** a transfer marked stalled by feature 007, **When** the page opens, **Then** it is
   counted under Stalled, not Downloading, and its row shows the Stalled badge and reason.
3. **Given** the operator clicks a count, **When** the list redraws, **Then** it shows only the
   rows in that state, and a clear way to show all rows again.
4. **Given** a count is zero, **When** the page opens, **Then** it is shown as 0 and dimmed, so
   the set of states always looks the same.

---

### User Story 3 - Pause and resume one transfer, at either stage (Priority: P2)

On any row, the operator clicks **Pause**, and later **Resume**.

- **In the local stage**, Pause stops the NAS download of every file of that transfer. A file
  that was downloading stops and keeps what it already has, so Resume carries on from there.
  Other transfers keep downloading and take the free download slots.
- **In the Put.io stage**, Put.io cannot pause a transfer: it only offers cancel. So Pause puts
  the transfer **on hold** instead. Put.io keeps downloading it, and the page says so plainly.
  When Put.io finishes, its files are queued for the NAS already paused, and nothing comes down
  until the operator clicks Resume. Cancelling is never offered as a way to pause.

**Why this priority**: The operator wants to free the NAS connection for something more urgent,
or hold back a large download, without losing it. The local stage is fully pausable. On the
Put.io stage, holding the NAS download is the closest real equivalent.

**Independent Test**: Against a mocked Put.io, pause a transfer while Put.io downloads it, and
check that nothing is sent to Put.io and the row shows "On hold". Let Put.io finish it, and check
that its files are queued paused. Resume, and check that they download. Pause a transfer in the
local stage mid-file, and check that the download stops, that another transfer's file starts in
the free slot, and that Resume continues the file from where it stopped.

**Acceptance Scenarios**:

1. **Given** a row in the local stage with one file downloading and three queued, **When** the
   operator clicks Pause, **Then** within 5 seconds no file of it is downloading, all four show as
   paused on the Queue page, and the row shows Paused.
2. **Given** a paused row in the local stage, **When** the operator clicks Resume, **Then** its
   files go back to the queue, and the file that was stopped mid-way continues from its partial
   download instead of starting again.
3. **Given** a row in the Put.io stage, **When** the operator clicks Pause, **Then** no request
   is sent to Put.io, the row keeps showing Put.io's state and progress, and it carries an "On
   hold" badge saying "Put.io keeps downloading; NAS download held".
4. **Given** a held transfer, **When** Put.io finishes it and the sync picks up its files,
   **Then** its files are queued paused and the row shows Paused in the local stage.
5. **Given** a held transfer still on Put.io, **When** the operator clicks Resume, **Then** the
   hold is removed, and its files will be queued normally when Put.io finishes.
6. **Given** a paused or held transfer, **When** the daemon restarts, **Then** it is still paused
   or held.
7. **Given** a Radarr/Sonarr item (feature 011) that is paused, **When** the *arr lists
   torrents, **Then** it is reported as paused, not finished and not failed.

---

### User Story 4 - Jump to the place to act (Priority: P2)

Apart from Pause and Resume, the Active page is for watching, not managing. From any row, the
operator can jump to the page where its other actions live: the Transfers page for a transfer
in the Put.io stage (retry, re-add, remove), or the Queue page, filtered to that transfer's
files, in the local stage (retry, reorder, skip, cancel).

**Why this priority**: Spotting a problem is only useful if fixing it is one click away. Those
actions already exist on those pages, so this view links to them instead of copying them.

**Independent Test**: Open the page with one row in each stage. Follow the link on each row.
Check that it lands on the right page with that transfer or its files shown.

**Acceptance Scenarios**:

1. **Given** a row in the Put.io stage, **When** the operator follows its link, **Then** the
   Transfers page opens with that transfer visible.
2. **Given** a row in the local stage, **When** the operator follows its link, **Then** the
   Queue page opens showing only that transfer's files.

---

### User Story 5 - Pause several, or pause everything (Priority: P3)

The operator ticks several rows and clicks Pause or Resume once, as on the Transfers and Queue
pages since feature 008. A **Pause NAS downloads** switch at the top of the page holds every
download to the NAS, including files that later syncs find, until it is switched off. It does
not touch Put.io, which keeps downloading.

**Why this priority**: Useful at times, such as freeing the home connection for an evening,
but pausing one transfer covers the common case.

**Independent Test**: Tick three rows across both stages and pause them, and check each one
behaves as in User Story 3. Switch on Pause NAS downloads, let a sync find new files, and check
that nothing downloads. Switch it off, and check that everything not paused on its own resumes.

**Acceptance Scenarios**:

1. **Given** three ticked rows, two in the local stage and one in the Put.io stage, **When** the
   operator clicks Pause, **Then** the two are paused and the one is put on hold, and a toast
   reports the result for each.
2. **Given** Pause NAS downloads is on, **When** a sync finds new files, **Then** they are queued
   but none start, and the page shows a banner "NAS downloads paused" with a Resume button.
3. **Given** Pause NAS downloads is on and one transfer was also paused on its own, **When** the
   switch is turned off, **Then** every other transfer resumes, and that one stays paused.
4. **Given** Pause NAS downloads is on, **When** the daemon restarts, **Then** it is still on.

---

### User Story 6 - Recently finished transfers (Priority: P3)

Below the active list, a short **Recently finished** list shows transfers that are done, or
that failed and can no longer be retried (FR-002). It shows when each finished, how long the
whole trip took (from being added to Put.io until the last file reached the NAS), and how long
each stage took.

**Why this priority**: It confirms that something just finished, and shows where time goes:
waiting on Put.io, waiting for the next sync, or downloading to the NAS. History already keeps
the long-term record of local files.

**Independent Test**: Take a transfer through both stages with known times. Check that it
appears in Recently finished with the expected total time and per-stage times, and that it
leaves the list after the retention period.

**Acceptance Scenarios**:

1. **Given** a transfer just became done, **When** the page refreshes, **Then** it shows in
   Recently finished with its finish time, total time and per-stage times.
2. **Given** a finished transfer older than the retention period, **When** the page
   refreshes, **Then** it is no longer listed.
3. **Given** a row in Error on Put.io, **When** the operator removes that transfer on the
   Transfers page, **Then** the row leaves the active list and shows in Recently finished as
   failed with Put.io's error. While it is still in Error on Put.io, it stays active.

### Edge Cases

- **A transfer with several files** (a season pack): it is one row. Local progress is by bytes
  across all its downloadable files. The row says how many files are done.
- **Files blocked or skipped locally** (blocked extensions, folder rules, or Skip on the Queue
  page): they do not count toward the transfer's total. A transfer whose files are all
  blocked never becomes active and goes straight to Recently finished as failed with "nothing to
  download", like feature 011. A transfer whose remaining files are all skipped is done when at
  least one file reached the NAS, with a note such as "1 file skipped", and failed with "nothing
  downloaded" when none did.
- **Files on Put.io that did not come from a transfer** (uploaded directly, or already in the
  account): they still show, as rows that start in the local stage. They are grouped by the
  top-level file or folder the sync is downloading.
- **Seeding**: a transfer that is seeding on Put.io has finished downloading there. It is in
  the local stage, and its Put.io state is shown as a small "seeding" note.
- **Put.io unreachable**: the page keeps the last known Put.io state, marks it as out of date
  with its age, and still shows live local figures.
- **Transfer removed on Put.io** while its files are still downloading locally: the row stays
  in the local stage until its files finish or fail.
- **Some files of a transfer paused on the Queue page**: the row's local state is Paused if no
  file of it is downloading or queued and at least one is paused. The row's Resume then resumes
  them all.
- **Mixed local states**: a row has one local state, picked in this order: Failed, Downloading,
  Queued, Paused, Waiting for sync. The file counts show the full picture.
- **A paused row's figures**: speed shows "—" and time left shows "Paused". Progress stays where
  it stopped.
- **Pause on a "Waiting for sync" row**: it is held, like a row in the Put.io stage, so its
  files arrive paused.
- **Held transfer fails on Put.io**: the row shows the Put.io error. The hold is dropped with it.
- **Pausing a file that is a few seconds from done**: it still stops. Resume finishes it.
- **Resume while the server has removed the partial file**: the file starts again from the
  beginning, and the row says so once.
- **Many active transfers** (hundreds): the page stays usable, with the name filter and state
  filters to narrow it.
- **delete-after-download** is on: Put.io files disappear once downloaded. This must not make a
  row look failed or lose its link to the transfer.

## Requirements *(mandatory)*

### Functional Requirements

**The view**

- **FR-001**: The web UI MUST have an Active page that lists each active transfer once, as a
  single row covering both stages. It sits in the main navigation with the other pages.
- **FR-002**: A transfer is active from the moment it appears on Put.io until every
  downloadable file of it is on the NAS. A row for files with no Put.io transfer is active from
  when the sync finds them until they are all on the NAS. A failed row stays active, counted
  under Error or Failed, while it can still be retried. It leaves the list when it can no
  longer be retried: as failed when its Put.io transfer is removed while in error, or as done
  or failed (see Edge Cases) when its failed files are skipped on the Queue page. A transfer
  with nothing to download is never active and goes straight to Recently finished as failed.
- **FR-003**: Each row MUST show: name, current stage, state in that stage, progress, speed,
  time left, and size. In the local stage it MUST also show files done out of files total.
- **FR-004**: In the Put.io stage, progress, speed and time left MUST be Put.io's figures. In the
  local stage they MUST be for all of the transfer's downloadable files together, measured on
  the NAS.
- **FR-005**: A row MUST move from the Put.io stage to the local stage as soon as Put.io has
  finished downloading the transfer (completed or seeding).
- **FR-006**: A transfer finished on Put.io but not yet picked up by the sync MUST show as
  "Waiting for sync", with when the next sync run is due.
- **FR-007**: A failed row, active or recently finished, MUST show the reason: Put.io's error
  message, the local failure reason, or "nothing to download" with the blocked extensions.
- **FR-008**: A transfer marked stalled by feature 007 MUST show its Stalled badge and reason on
  its row.
- **FR-009**: Where it is known, a row SHOULD show where the transfer came from: Radarr or
  Sonarr (feature 011, by category), the watch folder, or added in the web UI.

**Counts and filters**

- **FR-010**: The page MUST show counts by state for the Put.io stage (Queued, Downloading,
  Stalled, Finishing, Error) and for the local stage (Waiting for sync, Queued,
  Downloading, Paused, Failed). Every state is always shown, including zeros.
- **FR-011**: Each transfer MUST be counted once, in the state of the stage it is in now.
  A paused row in the local stage counts under Paused. A held row in the Put.io stage counts
  under its real Put.io state (it is still downloading there). The number of held rows is shown
  next to the Put.io counts as "N on hold", outside the sum.
- **FR-012**: Clicking a count, or "on hold", MUST filter the list to those rows. The list MUST
  also have a filter by name.

**Pause and resume**

- **FR-013**: Each active row MUST offer Pause, or Resume when it is paused or held.
- **FR-014**: Pausing a row in the local stage MUST pause every not-yet-complete file of that
  transfer. A file that is downloading MUST stop within 5 seconds and keep its partial data.
  The freed download slot MUST go to the next queued file of another transfer.
- **FR-015**: Resuming a row in the local stage MUST put its paused files back in the queue in
  their previous order. A file with partial data MUST continue from it.
- **FR-016**: Put.io offers no pause or resume for transfers, only cancel and retry. Pausing a
  row in the Put.io stage (or Waiting for sync) MUST therefore put it on hold: nothing is sent
  to Put.io, the row MUST say that Put.io keeps downloading, and its files MUST be queued
  paused when the sync picks them up. Resume MUST remove the hold, or resume the files if they
  are already queued.
- **FR-017**: The page MUST NOT cancel or remove a Put.io transfer as part of pausing.
- **FR-018**: Pause and hold MUST survive a daemon restart.
- **FR-019**: The operator MUST be able to tick several rows and pause or resume them at once,
  with a result per row, following the selection pattern of feature 008.
- **FR-020**: The page MUST offer a Pause NAS downloads switch. While on, no file starts
  downloading to the NAS, including files found by later syncs. Downloads in progress stop as in
  FR-014. Turning it off MUST resume everything except transfers paused on their own. The switch
  MUST survive a restart, and the Status and Queue pages MUST show when it is on.
- **FR-020a**: The Queue page's "Pause all" and "Resume all" buttons MUST become this same
  switch: one control, shown on both the Queue and Active pages, with the same state. There is
  no separate one-off "pause what is queued now" action any more.
- **FR-021**: A paused or held Radarr/Sonarr item (feature 011) MUST be reported to the *arr as
  paused, never as finished or failed.
- **FR-022**: Pausing, holding and resuming MUST be logged with the transfer's name, and counted
  in metrics without names.

**Links for other actions**

- **FR-023**: Each row MUST link to the page where its other actions live: the Transfers page in
  the Put.io stage, and the Queue page, showing only that transfer's files, in the local stage.
- **FR-024**: Apart from pause and resume, the Active page MUST NOT add its own retry, cancel or
  remove actions.

**Recently finished**

- **FR-025**: The page MUST list recently finished transfers (done or failed) with finish time,
  total time, and time spent on Put.io, waiting for sync, and downloading to the NAS.
- **FR-026**: Recently finished transfers MUST stay listed for a set time (default 24 hours),
  which can be changed in the configuration. The list MUST survive a daemon restart.

**Freshness and load**

- **FR-027**: The page MUST refresh on its own, at least every 5 seconds while open, like the
  Status and Queue pages.
- **FR-028**: Opening the page, in any number of browser tabs, MUST NOT make the daemon ask
  Put.io for transfers more often than it does for the Transfers page today.
- **FR-029**: When Put.io cannot be reached, the page MUST keep showing the last known Put.io
  state, marked with its age, and keep showing live local figures.

**Existing pages**

- **FR-030**: The Transfers and Queue pages keep all their actions. On the Status page, the
  Queue card stays. The "Active downloads" card, a list of single local files, MUST be replaced
  by an **Active transfers** card that uses the same rows as the Active page:
  - one line per transfer, up to 5, busiest first (downloading before queued, then by speed);
  - each line shows the name, a stage badge ("Put.io" or "NAS"), a progress bar and time left;
  - above the lines, one sentence of counts, for example "3 on Put.io · 2 to the NAS · 1
    failed", with failures in the error colour;
  - a "See all N active transfers" link to the Active page;
  - when nothing is active, the existing empty message.
  The card has no buttons. Pausing is done on the Active page. The Queue page gains Pause and
  Resume for single files, so a file paused there and a file paused through its transfer here
  are the same thing. It also gains Retry for a single failed file, which it lacks today.
- **FR-031**: The daemon MUST expose metrics for active transfers by stage and state. Metric
  labels MUST NOT include transfer names.

### Key Entities

- **Pipeline item**: one transfer (or one group of files with no transfer) followed through both
  stages. It holds the Put.io transfer (when there is one), its name and size, where it came
  from when known, the local files that belong to it, its current stage and state, whether it
  is paused or on hold, and when it entered and left each stage.
- **Pause NAS downloads switch**: one on/off setting for the whole daemon, kept across restarts.
- **Stage**: Put.io (the torrent downloading on Put.io) or local (files downloading to the NAS).
  Each stage has its own set of states, listed in FR-010. The page labels the local stage
  "NAS", and the API calls it `nas`.
- **Recently finished item**: a pipeline item that reached done or failed, with its finish time
  and time per stage, kept for the retention period.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: From the Active page alone, the operator can tell for any active transfer which
  stage it is in and when it should be on the NAS, without opening another page.
- **SC-002**: In the automated tests, a transfer appears as exactly one row through its whole
  trip, and never as done while any of its downloadable files is missing on the NAS.
- **SC-003**: A change of stage or state shows on the open page within 10 seconds of the daemon
  learning about it.
- **SC-004**: The counts always add up to the number of active rows.
- **SC-005**: With 200 active transfers, the page opens and filters in under 2 seconds.
- **SC-006**: With the page open in any number of tabs, Put.io sees no more transfer list
  requests than with one Transfers page open today (one every 5 seconds).
- **SC-007**: After Pause, no data for that transfer reaches the NAS after 5 seconds. After
  Resume, a file that was stopped mid-way does not download its finished part again.
- **SC-008**: In the automated tests, pausing never sends a cancel or remove to Put.io.

## Assumptions

- The link between a Put.io transfer and its local files is the file or folder Put.io creates
  when the transfer finishes. Files the sync downloads from under it belong to that transfer.
- The page uses the same sign-in as the rest of the web UI. Pause and resume are its only
  actions.
- Put.io's API has no pause or resume for transfers (only add, retry, cancel and clean), as
  noted when feature 004 built the Transfers page. If Put.io adds pause later, the Put.io-stage
  Pause can use it instead of a hold.
- Today the local queue only has Pause all and Resume all for the files queued at that moment,
  with no single-file pause, no lasting switch and no Retry for a failed file. This feature adds
  all three.
- The NAS download can continue a partial file (resume is already supported by the
  downloader). Where it cannot, the file starts again.
- Stage 2 ends when the files are on the NAS. Steps after that (hooks, subtitles, translation)
  belong to feature 038, which can add them to this view later.
- Starting a sync as soon as a transfer finishes on Put.io, to close the "Waiting for sync"
  gap, is out of scope. This view only makes the gap visible.
- Feature number 039 is used because `ROADMAP.md` reserves 019 to 038.
