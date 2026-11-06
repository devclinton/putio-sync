# Feature Specification: Processing Pipeline and Its View

**Feature Branch**: `038-processing-pipeline`

**Created**: 2026-11-05

**Status**: Draft

**Input**: Roadmap entry 038: "Every item goes through a pipeline of steps, and the UI shows
where it is: Pre-download hooks → Downloading → Pre-processing hooks → Generating subtitles →
Translating (one branch per language, run in parallel) → Post-processing hooks → Notifications.
… Steps that are not configured are skipped … Radarr and Sonarr must not import an item before
its pipeline is done." Asked for as the base that subtitles (025) and translation (026) plug
into.

## Context

- An item today is downloaded, then events go out:
  - event hooks (027) and notifications fire;
  - Radarr/Sonarr see it complete through the Transmission emulation (011), or find it in the
    download folder (watch-folder mode).
- Hooks (027) are fire-and-forget. Nothing waits for them, and nothing can change files
  before Radarr/Sonarr import.
- Feature 039 adds the **Active** page. Its unit is one row per transfer, across two stages
  (Put.io, then local), plus a lasting hold. This feature adds steps after the local stage and
  uses the same unit: a pipeline item is one transfer, or one synced top-level folder or file
  when there is no transfer.
- Subtitles (025) and translation (026) are not built yet. This feature builds the pipeline
  they run in and the steps that exist today (hooks, notifications). They add their own steps
  later.

## Clarifications

Settled without questions (see Assumptions):
- An item is a transfer or a top-level synced entry.
- Pipeline hooks are new hook kinds next to the existing event hooks.
- The hold-back applies to the Transmission and watch-folder modes only.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See where an item is in its pipeline (Priority: P1)

On the Active page (039), each row shows the item's current step ("Downloading 40%",
"Post-processing: tidy-names", "Notifying"). Expanding a row shows a timeline of every step:
waiting, running, done, failed or skipped, with how long each one took and its log lines.
Parallel branches (later, one translation per language) are shown side by side. When the item
is finished, it moves to History with its timeline kept.

**Why this priority**: Without the timeline, every later step (hooks, subtitles, translation) is
invisible. "Why hasn't Sonarr imported it?" has no answer.

**Independent Test**: With one post-processing hook configured, an item's timeline shows
Downloading done, Post-processing running, then done with its duration and output. With nothing
configured, the timeline is just Downloading.

**Acceptance Scenarios**:

1. **Given** no pipeline steps are configured, **When** an item downloads, **Then** its
   timeline shows Downloading, and the steps that are not configured are listed as skipped (or
   hidden behind "show skipped").
2. **Given** a post-processing hook, **When** the item finishes downloading, **Then** the row
   shows "Post-processing: <hook name>" while it runs, and the timeline shows its start,
   duration and the last lines of its output.
3. **Given** an item finished, **When** History is opened, **Then** the item's timeline is
   there, the same as it was on the Active page.
4. **Given** putio-sync restarts while a step is running, **When** it is back, **Then** the
   item shows that step as interrupted and runs it again, and steps that were done are not run
   again.

---

### User Story 2 - Run my own steps before and after the download (Priority: P1)

The operator adds pipeline hooks, each attached to one stage:
- **pre-download**: runs before the local download starts. It can skip the item or let it go
  on.
- **pre-processing**: runs after the download, before subtitles.
- **post-processing**: runs after subtitles and translation, before notifications.

A hook is a program (or URL) that gets the item's details and folder. It can change files in
the item's folder. A non-zero exit (or HTTP error) fails the step. Hooks in the same stage run
in the order listed.

**Why this priority**: These are the steps that can be configured today, and they let users
rename, unpack or tidy files before Radarr/Sonarr import.

**Independent Test**: A pre-processing hook that writes a marker file into the item folder runs
after the download and before the item is reported complete. A failing hook marks the step
failed and holds the item. A pre-download hook that answers "skip" stops the download.

**Acceptance Scenarios**:

1. **Given** a pre-download hook that exits 0, **When** an item is picked up, **Then** the
   download starts after it.
2. **Given** a pre-download hook that exits with the "skip" code (documented), **When** an item
   is picked up, **Then** it is not downloaded, and the timeline says skipped by that hook.
3. **Given** a post-processing hook, **When** it runs, **Then** it gets the item name, the
   category, the item's folder and the list of files, and can change files in that folder.
4. **Given** a hook that fails or runs past its timeout, **When** the item is shown, **Then**
   the step is failed with its output, and the item is not reported complete.
5. **Given** existing event hooks (027), **When** this feature ships, **Then** they behave as
   before: fire-and-forget, never holding an item.

---

### User Story 3 - Radarr and Sonarr import only finished items (Priority: P1)

Radarr/Sonarr never import an item while a step that changes files is still to run.
- **Transmission mode**: the item reports as still downloading, under 100%, with the current
  step as its status message, until the last file-changing step is done.
- **Watch-folder mode**: the item is built in a hidden staging folder on the same disk and is
  moved into place in one rename when its file-changing steps are done.

**Why this priority**: Importing early means Radarr/Sonarr move the video before the subtitles
or the hook's changes exist. That is the main reason the pipeline is needed.

**Independent Test**:
- Transmission mode: with a slow post-processing hook, the item's percent stays below 100
  while the hook runs, the status message names the step, and it reaches 100% only after the
  hook.
- Watch-folder mode: the category folder never shows the item until it appears whole.

**Acceptance Scenarios**:

1. **Given** Transmission mode and a post-processing hook, **When** the download finishes,
   **Then** Radarr's view of the item stays under 100% with "Post-processing: <hook>" until the
   hook is done.
2. **Given** watch-folder mode, **When** the item is being downloaded and processed, **Then**
   nothing of it is in the category folder, and after the last file-changing step the whole
   folder appears at once.
3. **Given** no file-changing steps are configured, **When** an item downloads, **Then** it is
   reported complete exactly as today, with no extra delay.
4. **Given** the item is held (039), **When** the hold is released, **Then** the pipeline
   continues from the step it was at.

---

### User Story 4 - Retry a failed step, and don't wait forever on an optional one (Priority: P2)

A failed step has a Retry button that runs that step again, then the steps after it. Each step
is either required or optional:
- a required step holds the item until it succeeds or the operator skips it;
- an optional step is retried a set number of times, then the item goes on without it.

When an item goes on without a step, the failure stays on the item, a notification is sent,
and the item counts as "completed with warnings".

**Why this priority**: Without this, one flaky hook (or later, one failed translation) blocks an
import forever.

**Independent Test**: An optional hook that always fails is tried the set number of times, then
the item completes with a warning and a notification. A required one waits. Retry after fixing
the hook makes it pass, and the item completes.

**Acceptance Scenarios**:

1. **Given** a failed step, **When** Retry is pressed, **Then** that step runs again, and the
   steps after it follow if it succeeds.
2. **Given** a failed required step, **When** Skip is pressed (with confirmation), **Then** the
   item goes on to the next step, and the skip is recorded on the timeline.
3. **Given** an optional step that fails 3 times (the default), **When** the last retry
   fails, **Then** the item goes on, shows "completed with warnings", and a notification names
   the failed step.
4. **Given** a step is retried, **When** the timeline is shown, **Then** each attempt is listed
   with its result and output.

---

### User Story 5 - Steps can be turned off per category (Priority: P3)

The operator turns pipeline steps on or off per category (for example, no subtitle steps for
`radarr` because Bazarr does them). Turned-off steps show as skipped, with the reason.

**Why this priority**: Needed once subtitles (025) exist. This feature gives hooks a category
filter, and the per-category switch is ready for 025 and 026 to use.

**Acceptance Scenarios**:

1. **Given** a post-processing hook limited to category `sonarr`, **When** a `radarr` item
   finishes, **Then** the hook is skipped with "not for this category".

### Edge Cases

- The same pipeline applies to Radarr/Sonarr items, watch-folder items and plain sync.
- A hook removes the video or moves files out of the item folder: the next step sees the
  folder as it is; Radarr/Sonarr import what is left. This is the hook's responsibility, and
  it is logged.
- A hook writes outside the item folder: allowed (it is the operator's program), but the
  staging rename only moves the item folder.
- The staging folder and the target are on different filesystems: refuse watch-folder staging
  with a clear error in Settings, rather than copying in a non-atomic way.
- A pipeline step is running when the operator deletes the item (on the Local files page or in
  the Queue): the step is stopped and the item is ended as cancelled.
- Many items finish at once: the steps of different items run with a set concurrency (default 2
  items at once). Branches of one item run in parallel up to that limit.
- A single-file item with no folder: the hook gets the file's path, and the staging folder holds
  just that file.
- An item failed in the download stage: the steps after it do not run, and the timeline shows
  them as not reached.
- The configuration changes while an item is mid-pipeline: steps not started yet use the new
  configuration; the running step finishes with the old one.

## Requirements *(mandatory)*

### Functional Requirements

**Pipeline model**

- **FR-001**: Every item MUST go through this ordered pipeline: Pre-download → Downloading →
  Pre-processing → Subtitles → Translating → Post-processing → Notifications. Each stage holds
  zero or more steps.
- **FR-002**: A step MUST have a state: waiting, running, done, failed, skipped (with a reason)
  or interrupted. It MUST record start, end, the attempts made, and the last 200 lines (or
  64 KB) of its output.
- **FR-003**: Stages with no steps configured MUST be skipped instantly. With nothing
  configured, an item MUST behave as today: no extra wait, and the same events and reports.
- **FR-004**: A stage MUST be able to hold parallel branches (for translation, one per
  language). Branches MUST run at the same time, and the stage is done when all are done or
  given up.
- **FR-005**: The pipeline state MUST survive restarts: done steps are not run again, and a
  step running at shutdown is marked interrupted and run again.
- **FR-006**: The pipeline MUST be the same for items from Radarr/Sonarr, the watch folder and
  plain sync.
- **FR-007**: Steps of different items MUST run with a configurable concurrency (default 2).

**Pipeline hooks**

- **FR-008**: The operator MUST be able to add hooks to the pre-download, pre-processing and
  post-processing stages. Each hook has a name, a command or URL, a timeout, a category filter,
  and a required/optional choice.
- **FR-009**: A pipeline hook MUST receive the item: name, category, source (arr, watch or
  sync), item folder, file list with sizes, and Put.io IDs. Commands get it as environment
  variables plus a JSON file; URL hooks get it as the POST body.
- **FR-010**: Exit 0 (HTTP 2xx) MUST mean success. A documented exit code (HTTP status) MUST
  mean "skip this item", for pre-download only. Anything else, or a timeout, MUST mean failed.
- **FR-011**: Hooks in one stage MUST run one after another, in the order listed.
- **FR-012**: Existing event hooks (027) MUST keep their current behavior.

**Hold-back for Radarr/Sonarr**

- **FR-013**: A step MUST declare whether it changes files. Pipeline hooks do, and so will
  subtitles and translation. Notifications do not.
- **FR-014**: In Transmission mode, an item MUST report less than 100% done until its last
  file-changing step is done, and MUST report the current step as its status message.
- **FR-015**: In watch-folder mode, an item MUST be downloaded and processed in a hidden staging
  folder on the same filesystem as the category folder, and MUST be moved into place in one
  rename when its file-changing steps are done.
- **FR-016**: With no file-changing step configured, items MUST be reported and placed exactly
  as today.
- **FR-017**: A hold (039) MUST pause the pipeline between steps; release continues it.

**Failure, retry, give-up**

- **FR-018**: A failed step MUST be retryable on its own from the UI and the API. After it
  succeeds, the remaining steps run.
- **FR-019**: A required failed step MUST hold the item until it is retried successfully or
  skipped by the operator.
- **FR-020**: An optional step MUST be retried automatically up to a set number of times
  (default 3, with backoff). After that, the item MUST go on and be marked completed with
  warnings.
- **FR-021**: An item completed with warnings MUST send a notification naming the failed steps,
  and MUST show the warning in History.

**View**

- **FR-022**: The Active page row MUST show the current step and, where the step reports it,
  its progress.
- **FR-023**: Expanding a row MUST show the timeline: every stage, its steps and branches side
  by side, state, duration, attempts, and output.
- **FR-024**: History MUST keep each finished item's timeline for as long as History keeps the
  item.
- **FR-025**: Skipped steps MUST show their reason: not configured, not for this category, or
  skipped by the operator or by a hook.

**Per-category**

- **FR-026**: Pipeline hooks MUST support a category filter. The pipeline MUST let later step
  kinds (subtitles, translation) be turned off per category.

**Observability**

- **FR-027**: The daemon MUST expose metrics for step runs by stage, kind and result, step
  duration, and items waiting at each stage. Labels are bounded: no item names.
- **FR-028**: Each step start and end MUST be logged with the item, stage, step name, attempt,
  result and duration as structured fields. Hook output MUST go through the existing secret
  redaction.

### Key Entities

- **Pipeline item**: one transfer, or a top-level synced entry; source, category, folder,
  current stage, overall state (running, held, done, done with warnings, failed, cancelled).
- **Stage**: an ordered position in the pipeline. It holds steps and, for translation,
  branches.
- **Step**: one unit of work (a hook run; later a subtitle fetch or one translation): kind,
  name, required or optional, changes files or not, state, attempts.
- **Attempt**: one run of a step, with start, end, result and an output tail.
- **Pipeline hook**: the configured hook, with its stage, order, category filter and
  required/optional choice.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With no steps configured, the time from the download ending to Radarr/Sonarr
  seeing it complete is the same as before this feature (within 1 s).
- **SC-002**: In tests covering both modes, Radarr/Sonarr never see an item complete while a
  file-changing step is waiting or running.
- **SC-003**: An operator can find out why an item has not been imported from the Active page
  alone, in under 30 seconds.
- **SC-004**: A failing optional step never keeps an item from completing for longer than its
  retries take (default under 15 minutes).
- **SC-005**: After a restart in the middle of a pipeline, no done step runs a second time.
- **SC-006**: Timelines are kept in History for every finished item, for as long as History
  keeps the item.

## Assumptions

- An item is the 039 unit:
  - one Put.io transfer, with all its files;
  - or, for plain sync without a transfer, one top-level folder or file.

  Steps run once per item, not once per file.
- Pipeline hooks are a new hook kind next to the 027 event hooks. They reuse the same command
  and URL runner, timeout limits, header and env masking, and the Settings page. Event hooks
  are not changed.
- The skip exit code for pre-download hooks is 10, and a URL hook skips with HTTP 204 plus
  `{"action":"skip"}`. Both are documented in EVENTS.md.
- Subtitles (025) and translation (026) are out of scope. Their stages exist, but are skipped
  as "not configured" until those features add steps.
- Notifications become the last stage. The existing notification and event behavior moves into
  it unchanged. A completed-with-warnings item adds one notification.
- Timelines are stored with download history. Their retention follows History's.
- The default retry policy for optional steps is 3 tries, with 1, 5 and 10 minute backoff.
