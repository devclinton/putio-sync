# Research: Active Transfers Pipeline View

Decisions for [spec.md](spec.md), from reading the code on 2026-10-28. Each has a decision, the
reason, and what else was considered.

## R1 Linking a Put.io transfer to its local files

**Decision**: Use `download_sources.transfer_id` (feature 006). When the sync queues a file, it
already looks up the transfer whose file or folder holds it (`matching_transfer` in
`putio-sync-download/src/manager.rs`) and records `file_id → transfer_id`. A pipeline item's
files are the `download_sources` rows with that transfer id. Each file is in one of three
places:
- **in the queue**: it has a `download_queue` row (pending, downloading, paused or failed);
- **done**: it has a `downloads` row with status `completed`. The downloader deletes a file's
  queue row as soon as it completes (`manager.rs`, after the download succeeds), so the queue
  alone cannot say which files are done;
- **skipped**: neither of the above. The operator used Skip on the Queue page, which deletes
  the row.

`files.done` counts done files. The total leaves out skipped files, as it leaves out blocked
ones (spec edge cases). `done_bytes` adds the size of done files to the bytes already
downloaded of files in the queue.

**Rationale**: The link exists and is written on every enqueue, including the *arr path
(`enqueue_tree_report`). No new lookup against Put.io is needed.

**Alternatives**: Walking Put.io folders from `transfer.file_id` on each request (one Put.io
call per transfer, too slow and too many calls). Matching by name (unreliable).

## R2 Rows for files with no transfer

**Decision**: Queue rows with no `transfer_id` are grouped by their `relative_path` (the folder
the file is written into). A file at the sync root is its own row. The row key is
`folder:<relative_path>` or `file:<file_id>`. These rows start in the local stage.

**Rationale**: Matches the spec's "grouped by the top-level file or folder the sync is
downloading" with data the queue row already has.

**Alternatives**: One row per file (a folder upload would flood the page). Grouping by the
first path segment (puts unrelated downloads, such as everything under `sonarr/`, in one row).

## R3 Not asking Put.io more often (FR-028, SC-006)

**Decision**: Add a shared transfers snapshot to `DownloadManager`. `transfers_snapshot(max_age)`
returns the last Put.io transfer list if it is younger than `max_age`, and otherwise fetches it
once. Concurrent callers wait on the same fetch (a `tokio::sync::Mutex` around the cached
list). The Active page, the Status card and the daemon's pipeline loop all read it with a
10-second maximum age. The Transfers page keeps its direct call, as the baseline the spec
measures against.

**Rationale**: Any number of tabs, plus the Status page polling every 3 seconds, then costs at
most one Put.io call every 10 seconds. That is fewer calls than one open Transfers tab makes
today (one every 5 seconds).

**Alternatives**: Caching in the API crate only (the daemon loop could not share it). Moving
the Transfers page onto the snapshot too (a good follow-up, but it changes a shipped page's
freshness, so it is out of scope).

## R4 Where the pipeline logic lives

**Decision**: A new module `putio-sync-download/src/pipeline.rs` with a pure function
`build(input) -> Pipeline`. Its input is the transfer list, the queue rows joined with their
sources, the holds, live progress, stall verdicts, the sync's next run time and `now`. Its
output is the items and counts. A thin async `DownloadManager::pipeline()` gathers the inputs.

**Rationale**: Principle I. The download crate owns the queue and the enqueue path that holds
must change, and it already depends on `client` and `db`. The *arr crate depends on
`download`, so it can use the same pause state without a cycle. A pure function makes every
state rule testable without Put.io (Principle III).

**Alternatives**: `core` (cannot see client or db types). `api` (the daemon loop and the *arr
view also need it). A new crate (Principle VII: not needed).

## R5 Mapping states

**Decision**:

| Put.io status | Stage | State |
|---|---|---|
| `IN_QUEUE`, `WAITING`, `PREPARING_DOWNLOAD` | Put.io | Queued |
| `DOWNLOADING`, not stalled | Put.io | Downloading |
| `DOWNLOADING`, stalled (feature 007's `StallTracker`) | Put.io | Stalled |
| `COMPLETING` | Put.io | Finishing |
| `ERROR` | Put.io | Error |
| `COMPLETED`, `SEEDING` | NAS | from its files (below); "seeding" shown as a note |
| anything else | Put.io | Queued, with Put.io's raw status shown |

In the NAS stage, the state comes from the transfer's files, in the spec's order: Failed if
any failed; else Downloading if any is downloading; else Queued if any is pending; else Paused
if any is paused; else Waiting for sync when the transfer has no source files yet. When none
of its files is left in the queue, the item is done if at least one file is done, with the
note "N files skipped" when some were skipped. If every file was skipped, it failed with "nothing
downloaded".

**Spec fix made during planning**: the spec listed Seeding as a Put.io count, but also said a
seeding transfer is in the NAS stage. Counting once (FR-011) makes both impossible, so Seeding
was dropped from the Put.io counts (US2, FR-010). It shows as a note on the row.

## R6 "Waiting for sync", and old transfers that are not active

**Decision**: A finished transfer with no queue rows and no source rows is:
- **Waiting for sync** if it finished after the start of the last completed sync run, or no sync
  has run yet. The row shows `next_due_at` from `SyncStatus`.
- **Not active** otherwise. The sync has already seen it and queued nothing (all blocked,
  skipped or already on disk). If a rule decision (feature 009) blocked any of its files, and it
  finished within `finished_retention_hours`, it is recorded once in Recently finished as failed
  with "nothing to download" and the blocked extensions. It is never active, because there is
  nothing to retry. Otherwise it is not shown.

An item with none of its files left in the queue is done (R1, R5), and is never shown as
active again.

**When a row is written to Recently finished.** Put.io keeps finished transfers until they are
cleared, and `download_sources` keeps every file ever queued. "The first time the daemon sees
it done" would therefore record every old transfer on the first run, and record it again after
each prune. Instead the loop writes a row only on a transition:
- it keeps, in memory, the keys that were active on its previous tick. An item that was active
  then and has now left (done, or failed for good) is written, with `done_at` = now, or the last
  `downloads.downloaded_at` for a done item;
- after a restart, the first tick has no previous keys. It writes only done items whose last
  `downloads.downloaded_at` is within `finished_retention_hours` and that have no row yet. A
  failed-for-good transition that happened while the daemon was down is not recorded;
- the blocked "nothing to download" case is written with `done_at` = `transfer.finished_at`,
  only when that is within the retention period and no row exists.

So a pruned row is never written again: its item is not active, and its time is outside the
retention period.

**Bounding the file query.** `pipeline_files()` reads only: files with a queue row, files of
transfers in the current Put.io list, and files whose `downloads.downloaded_at` is within the
retention period. Its cost then follows the active work, not the whole download history.

A failed row stays active while it can be retried: Put.io `ERROR` (Retry on the Transfers page)
or failed queue rows (Retry on the Queue page, R10). It leaves the active list when it can no
longer be retried:
- the transfer is gone from Put.io while in `ERROR`: recorded as failed with Put.io's error;
- the operator skipped its failed files: the item is then done or failed by the rule in R5
  (spec FR-002).

**Rows whose transfer is gone, or whose Put.io files were deleted**: the link lives in
`download_sources`, not on Put.io, so a row keeps its `transfer:<id>` key and its NAS stage when
the transfer is removed from Put.io or when delete-after-download removes its Put.io files.
Neither makes it failed. When the transfer is no longer in the list, the row's name is the top
folder of its files' `relative_path` (or the file name), and its size is the total of its
files that are not skipped.

**Rationale**: Put.io keeps finished transfers in its list until cleared. Without this rule,
every old transfer would show as waiting. `finished_at` and the sync's start time are both
already known.

## R7 Stopping a running download (FR-014, SC-007)

**Finding**: Today nothing can stop a running download. `pause_all` and `cancel_download` only
change the queue row's status, and the running file carries on to the end (see the comment on
`DownloadManager::cancel_download`).

**Decision**: When `DownloadManager` starts a file, it registers a stop signal for it (an
`Arc<AtomicBool>` plus a `tokio::sync::Notify`, in a map keyed by `file_id`). The
chunk loop in `stream.rs` checks the flag between chunks, and also while waiting on the rate
limiter, using `select!` on the notify. A stop ends the download with a new
`DownloadError::Stopped`. For that error, the manager:
- keeps the `.<name>.downloading` partial file;
- leaves the queue row as the caller set it (paused, or pending under the switch);
- does not count a failure in metrics or history.

**Rationale**: Chunks are about 128 KB, so a stop takes effect in well under 5 seconds even on
a slow line. No new dependency is needed (`tokio-util`'s `CancellationToken` was considered;
the workspace does not use `tokio-util`, and an atomic flag plus `Notify` covers it).

**Alternatives**: Aborting the task (`JoinHandle::abort`). This could leave the writer
half-flushed and skips the cleanup paths.

## R8 Resuming from the partial file

**Decision**: Reuse the existing resume path. `check_resume` checks the partial file's size
against the remote size before a range request (Principle IV). If a resumed file starts from 0
although it had stopped with bytes on disk, the item carries the note "Started again: the
partial download was gone" until that file completes. The note is kept in memory.

## R9 Holds for transfers still on Put.io

**Decision**: A new table `pipeline_holds(transfer_id PRIMARY KEY, name, held_at)`. In the
enqueue path, the manager now looks up the transfer *before* inserting the queue row. If that
transfer is held, the row is inserted with status `paused` instead of `pending`, so no worker
can claim it in between. Resume on a held item deletes the hold and resumes any of its rows
that are already queued. The daemon loop drops holds whose transfer has failed or is gone.

**Rationale**: Put.io has no transfer pause (the `client` crate exposes add, retry, cancel and
clean only; `putio-sync-api/src/routes/putio/transfers.rs` says the same). Pausing a transfer
never sends anything to Put.io (FR-017, SC-008).

## R10 Pausing one transfer or one file

**Decision**: New `QueueRepository` methods `pause_files(&[file_id])` and
`resume_files(&[file_id])`. They change only `pending` or `downloading` rows to `paused`, and
`paused` rows back to `pending`. Pausing a transfer pauses its files (from R1), then signals
the stop for any of them that is running. Resume keeps the queue order, because order comes
from `priority` and `id`, which are left unchanged. The Queue page gets per-file Pause and
Resume using the same methods (FR-030).

**Retry on the Queue page**: today a failed queue row stays failed. The page has no Retry, and
a later sync's enqueue keeps the existing status (`ON CONFLICT … DO UPDATE` leaves `status`
alone). A new `retry_files(&[file_id])` changes `failed` rows to `pending`, and the Queue page
gets a per-file Retry. This is what lets a NAS Failed row be retried (FR-002). The Active page
still has no Retry of its own (FR-024); its link opens the Queue page.

## R11 The Pause NAS downloads switch (FR-020, FR-020a)

**Decision**: A new key/value table, `daemon_flags(key PRIMARY KEY, value, updated_at)`, with
the key `nas_downloads_paused`. While it is on:
- `process_queue` claims nothing;
- turning it on signals a stop for every running file and puts those rows back to `pending`,
  not `paused`;
- rows paused on their own stay `paused`.

Turning it off lets workers claim again. Everything except the individually paused rows then
resumes, without touching any row. `POST /local/queue/pause` and `/resume` now switch it on and
off (FR-020a). `QueueRepository::pause_all` and `resume_all` are removed (Principle VII).

**Upgrade note**: rows paused by the old Pause all stay paused after the upgrade, as
individually paused rows. The changelog line says to resume them once.

**Alternatives**: A config file key (a settings save rewrites the file, and the switch is
runtime state, not configuration). Keeping the switch in memory (FR-020 requires it to survive
a restart).

## R12 Recently finished and stage times

**Decision**: A new table `pipeline_finished(key PRIMARY KEY, name, size, outcome, reason,
added_at, putio_done_at, queued_at, done_at)`. The daemon's pipeline loop writes a row when an
item becomes done, or when a failed item leaves the active list (R6). Its times come from:
- `added_at`: `transfer.created_at`;
- `putio_done_at`: `transfer.finished_at`;
- `queued_at`: the earliest `download_sources.recorded_at` of the item;
- `done_at`: the last file's completion time from `downloads.downloaded_at`; for a failed item,
  when it left the active list.

Rows older than `[transfers] finished_retention_hours` (default 24) are deleted by the same loop.
When rows are written, and why old transfers are not, is in R6.

## R13 The daemon pipeline loop

**Decision**: A loop in `putio-sync-daemon` every 10 seconds. It builds the pipeline from the
snapshot, records transitions (R12), clears stale holds (R9), and sets the gauges (R15). It
lets the snapshot refresh from Put.io at most once every 60 seconds; the shorter 10-second age
applies only while the page or Status card is reading it. The `server` engine has no daemon
loop: the API builds the pipeline on demand, and Recently finished stays empty there.

**Rationale**: Metrics and Recently finished must work with no browser open. Most transitions
(NAS done, or a NAS row leaving after its files were skipped) are local and need no Put.io
call.

## R14 Radarr/Sonarr items that are paused (FR-021)

**Decision**: `putio-sync-arr/src/view.rs` `LocalProgress` gains `paused: bool`. It is true when
the item's transfer is held, or every unfinished file of the item is paused, or the switch is
on. A paused item is reported as Transmission status 0 (stopped) with `leftUntilDone > 0` and
`isFinished = false`. Radarr and Sonarr show that as Paused, not completed or failed.

## R15 Metrics and logs (FR-022, FR-031)

**Decision**:
- `putio_sync_pipeline_items{stage, state}`, a gauge with bounded labels (2 stages, 10 states).
- `putio_sync_pipeline_actions_total{action}`, where action is `pause`, `hold` or `resume`.
- `putio_sync_nas_downloads_paused`, a gauge that is 0 or 1.

No names or ids appear in labels. Logs carry `transfer_id`, `name` and `files` as structured
fields.

## R16 Web UI

**Decision**:
- **New page `ui/pages/active.js`**, in the nav after Status.
- **Active page rows**: the row selection from `select.js` (feature 008). Pause/Resume per row
  and in the selection bar. Results come back per item and show in one toast.
- **Active page counts and filters**: a count chip per state that filters on click, plus a name
  filter.
- **Queue page**: gains a `?transfer=<id>` filter and per-file Pause/Resume. Its Pause all and
  Resume all become one switch button.
- **Status page**: the Active downloads card becomes the Active transfers card, built from a
  new `active` section of `GET /status`. The switch shows as a banner on all three pages.
- **Mock UI**: `examples/mock_ui.rs` gains fixtures for every stage and state, a held item, a
  paused item and the switch.

**Rationale**: The same patterns as the existing pages. The ES modules need no build step
(feature 004).

## R17 Performance (SC-005)

**Decision**: One SQL query joins `download_queue` with `download_sources` for active rows, and
`build` runs in memory. With 200 transfers and a few thousand files this is milliseconds. The
page filters on the client. No pagination.
