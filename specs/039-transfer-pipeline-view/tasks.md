---

description: "Task list for feature 039: Active Transfers Pipeline View"
---

# Tasks: Active Transfers Pipeline View

**Input**: Design documents from `specs/039-transfer-pipeline-view/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/api.md](contracts/api.md), [quickstart.md](quickstart.md)

**Tests**: included (Constitution III). Write each test first and see it fail. The stop test
(T003) must fail on today's code before T004–T006 start.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an unfinished task)
- **[Story]**: the user story the task serves (US1–US6 in spec.md)

## Phase 1: Setup

- [X] T001 [P] Add `finished_retention_hours` (default 24) to `TransfersConfig` in
  `putio-sync-core/src/config.rs` and to `[transfers]` in `config.example.toml`. The config lint
  refuses a value outside 1–720 with a clear message ("must be from 1 to 720"). Unit test for
  the default, 1, 720, 0 and 721.
- [X] T002 [P] Test then add `putio-sync-telemetry/src/pipeline.rs` (export from `lib.rs`):
  gauge `putio_sync_pipeline_items{stage,state}` (stage `putio`|`nas`, the 10 states), counter
  `putio_sync_pipeline_actions_total{action}` (`pause`|`hold`|`resume`), gauge
  `putio_sync_nas_downloads_paused` (0 or 1, no labels). Setters on `Telemetry`. Add the three
  families to `BASIC_FAMILIES` in `putio-sync-telemetry/tests/contract.rs`, asserting no name or
  id labels, and to the feature 003 metrics contract doc.

---

## Phase 2: Foundational (blocks every story)

### Stop a running download (R7, R8)

- [X] T003 Failing test first: `putio-sync-download/tests/stop_resume.rs`. A local HTTP server
  serves a slow body with range support. Start a download, pause its file, and assert within
  5 s: no more bytes arrive, `.<name>.downloading` is kept, the row is not `completed`, no
  failure is counted. Then resume and assert a range request from the partial size and a
  correct final file. A second case, with one download slot and a file of another transfer
  queued behind it: after the stop, that other file starts (FR-014). Run it on today's code
  and record that it fails.
- [X] T004 Add `DownloadError::Stopped` in `putio-sync-download/src/error.rs` (not retryable,
  not a failure).
- [X] T005 In `putio-sync-download/src/stream.rs`, take a stop signal (`Arc<AtomicBool>` plus
  `tokio::sync::Notify`). Check the flag between chunks, and `select!` on the notify while
  waiting on the rate limiter. Return `DownloadError::Stopped`, flushing and keeping the temp
  file, never renaming it.
- [X] T006 In `putio-sync-download/src/manager.rs`, keep `HashMap<file_id, StopSignal>`:
  register when a file starts, remove when it ends, and add `stop_files(&[file_id])`. On
  `Stopped`: keep the partial, leave the queue row status as the caller set it, record no
  failure in metrics or history, and run no Put.io delete. Resume goes through `check_resume`.
  If a file that stopped with bytes on disk restarts from 0, set the in-memory note "Started
  again: the partial download was gone" until it completes. T003 now passes.

### Database (R9–R12)

- [X] T007 Add the three tables to `putio-sync-db/src/schema.rs` exactly as data-model.md:
  - `pipeline_holds(transfer_id INTEGER PRIMARY KEY, name TEXT NOT NULL, held_at TIMESTAMP
    DEFAULT CURRENT_TIMESTAMP)`;
  - `daemon_flags(key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TIMESTAMP DEFAULT
    CURRENT_TIMESTAMP)`, value `"1"` or `"0"`;
  - `pipeline_finished(key TEXT PRIMARY KEY, name TEXT NOT NULL, size INTEGER NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('done','failed')), reason TEXT, added_at
    TIMESTAMP, putio_done_at TIMESTAMP, queued_at TIMESTAMP, done_at TIMESTAMP NOT NULL)`.
  No change to existing tables. Migration test on an existing database.
- [X] T008 [P] Tests then `putio-sync-db/src/pipeline.rs` (accessors in `lib.rs`):
  - `HoldsRepository`: `add(transfer_id, name)`, `remove`, `list`, `is_held`;
  - `FlagsRepository`: `get_bool(key)` (a missing key means off), `set_bool(key, bool)`
    writing `"1"`/`"0"`;
  - `FinishedRepository`: `upsert` (a later write for the same key replaces the row), `list`
    newest first, `prune_older_than(hours)`;
  - `pipeline_files()`: one query over `download_sources` left-joined to `download_queue` and
    to `downloads` (status `completed`). It reads only files with a queue row, files of the
    transfer ids passed in (the current Put.io list), and files whose
    `downloads.downloaded_at` is within `finished_retention_hours` (R6). Each
    file comes back as **in the queue** (with its queue status), **done** (no queue row, a
    `completed` downloads row) or **skipped** (neither), with sizes, `relative_path`,
    `transfer_id`, `recorded_at` and error. Queue rows with no source row are included as in
    the queue. Test each of the three cases, since a completed file has no queue row (R1).
- [X] T009 [P] Tests then, in `putio-sync-db/src/queue.rs`:
  - `add` takes the initial status (`pending` or `paused`);
  - `pause_files(&[file_id])` changes only `pending` or `downloading` rows to `paused`;
  - `resume_files(&[file_id])` changes `paused` rows back to `pending`, leaving `priority`
    and `id` (queue order) unchanged;
  - `retry_files(&[file_id])` changes `failed` rows to `pending`, keeping queue order (R10);
  - all three return the ids they changed.
  `pause_all` and `resume_all` stay until T045 removes them, so the build stays green.

### Pipeline core (R3, R4)

- [X] T010 Tests (mockito) then `DownloadManager::transfers_snapshot(max_age)` in
  `putio-sync-download/src/manager.rs`: a `tokio::sync::Mutex<Option<(Instant,
  Arc<Vec<Transfer>>)>>`. Two calls within `max_age` make one Put.io request, and concurrent
  callers share one fetch. On a Put.io error it returns the last list with its age, flagged
  stale.
- [X] T011 Create `putio-sync-download/src/pipeline.rs` (export from `lib.rs`) with
  `PipelineItem` (fields as data-model.md), `Stage`, `State`, `FileCounts`, `Origin`, `Counts`,
  `BuildInput` (transfers, active rows, holds, live progress, stall verdicts, last sync start,
  next sync time, `now`) and an empty `build(input) -> Pipeline`. Serde names match
  contracts/api.md (`waiting_for_sync`, `on_hold`, and so on).

**Checkpoint**: running downloads can be stopped and resumed; tables, snapshot and types exist.

---

## Phase 3: User Story 1 — See every active transfer and its stage (P1) 🎯 MVP

**Goal**: one row per transfer, in the Put.io or NAS stage, with progress, speed and time left.

**Independent test**: with the mock UI fixtures, each transfer shows once on `#/active`, in the
right stage and state, and moves from Put.io to NAS when Put.io finishes.

- [X] T012 [US1] Table tests in `putio-sync-download/src/pipeline.rs` (`#[cfg(test)]`), one per
  row of R5: `IN_QUEUE`/`WAITING`/`PREPARING_DOWNLOAD` → Put.io Queued; `DOWNLOADING` → Put.io
  Downloading, or Stalled with a stall verdict; `COMPLETING` → Finishing; `ERROR` → Error with
  `error_message` as reason; `COMPLETED`/`SEEDING` → NAS stage with `seeding` set for
  `SEEDING`; an unknown status → Put.io Queued with the raw `putio_status`. NAS state order:
  Failed, Downloading, Queued, Paused, then Waiting for sync. R6: finished after the last sync
  start (or no sync yet) → Waiting for sync with `next_sync_at`; finished before it with no
  rows → not active. Files (R1): `files.done` counts done files, skipped files count toward
  neither `total` nor `size`, `done_bytes` = done files' sizes + bytes downloaded in the queue.
  No file left in the queue: at least one done → done (with "N files skipped" when some were
  skipped); every file skipped → failed "nothing downloaded". Failed rows (FR-002, R6): Put.io
  `ERROR` and NAS Failed stay active; `ERROR` with the transfer gone → failed; all files
  blocked → never active. Transfer gone from
  Put.io, or its Put.io files deleted by delete-after-download, while files are still queued →
  the row keeps its `transfer:<id>` key and NAS stage, is not failed, and takes its name from
  the top folder of its files' `relative_path` and its size from its files not skipped. R2: rows
  without `transfer_id` group as `folder:<relative_path>`, root files as `file:<file_id>`.
  Sizes, `done_bytes`, `percent`, speed (sum of running files) and `eta_seconds` (remaining ÷
  speed) in the NAS stage; Put.io figures in the Put.io stage. Origin (`watch`, `manual`,
  `arr:<category>`, `putio`). Sort: failed/error first, then downloading by speed, then name.
  One item per transfer through every stage (SC-002).
- [X] T013 [US1] Implement item building in `build()` in `putio-sync-download/src/pipeline.rs`
  until T012 passes.
- [X] T014 [US1] `DownloadManager::pipeline()` in `putio-sync-download/src/manager.rs`: gather
  `BuildInput` from `transfers_snapshot(10 s)`, `pipeline_files()`, holds, live progress, the
  feature 007 stall tracker, origins (`arr_items.category` by `transfer_id`, then
  `transfer_origins`, then `download_sources.origin`), the sync status (last start,
  `next_due_at`) and the restart notes, then call `build`. Set `putio_as_of` and `putio_stale`
  from the snapshot.
- [X] T015 [US1] Tests first in `putio-sync-api/tests/active.rs`: `GET /active` returns the
  contracts/api.md shape; auth is required; no database → 503 with the usual error body; Put.io
  answering 503 → `putio_stale: true` with the last list and NAS figures still current.
- [X] T016 [US1] `putio-sync-api/src/routes/active.rs`: `GET /active` calling
  `DownloadManager::pipeline()`; register under the authenticated router in
  `putio-sync-api/src/lib.rs`. T015 passes.
- [X] T017 [US1] `putio-sync-api/ui/pages/active.js`: table with name and its origin label
  (Radarr/Sonarr category, watch folder or web UI) when known, stage badge, state, size,
  progress bar, files ("4 of 12 files"), speed, time left; Stalled badge and reason; Waiting
  for sync with the next sync time; "seeding" note; failure reason; the out-of-date marker with
  the Put.io figures' age when `putio_stale`. When nothing is active, "Nothing is downloading
  right now" with a link to the Transfers page. Refresh every 5 s while open. Cards at phone
  width (375 px) like the other pages. Add the "Active" nav link after Status in
  `putio-sync-api/ui/index.html` and the `#/active` route in `putio-sync-api/ui/app.js`.
- [X] T018 [P] [US1] `putio-sync-api/examples/mock_ui.rs`: fixtures for every stage and state,
  the season pack (4 of 12 files, seeding), a stalled transfer, a Put.io Error transfer, a
  waiting-for-sync transfer, a folder row with no transfer, a Radarr-origin row, and
  `MOCK_PUTIO_DOWN=1` making the mocked transfer list answer 503. `MOCK_ACTIVE_COUNT=200`
  (new, like `MOCK_PUTIO_DOWN`) adds that many generated active transfers, for SC-005.

**Checkpoint**: the Active page lists every active transfer once.

---

## Phase 4: User Story 2 — Counts by state for each stage (P1)

**Goal**: two rows of counts that add up to the number of rows, and a Status page summary.

**Independent test**: on the mock UI the counts show every state including zeros, add up to
the rows, and a click filters the list.

- [X] T019 [US2] Tests in `putio-sync-download/src/pipeline.rs`: every state present with zeros;
  each item counted once, in its current stage (FR-011); `putio` plus `nas` equals the number
  of items (SC-004); `on_hold` counted outside the sum.
- [X] T020 [US2] Build `Counts` in `build()` and add `counts` to `GET /active` in
  `putio-sync-api/src/routes/active.rs`; extend `putio-sync-api/tests/active.rs`.
- [X] T021 [US2] Count chips in `putio-sync-api/ui/pages/active.js`: one per state, a click
  filters, "Show all" clears; zero counts are shown as 0 and dimmed; "N on hold" chip; a name
  filter. Filtering is client-side.
- [X] T022 [US2] Tests then the `active` section in `putio-sync-api/src/routes/system/status.rs`:
  `{total, putio, nas, failed, top}`, `top` at most 5 items, downloading before queued then by
  speed; `failed` counts `error` and `failed` and is also in `putio` or `nas`. `downloads`
  stays in the response.
- [X] T023 [US2] `putio-sync-api/ui/pages/status.js`: the Active transfers card (counts
  sentence, up to 5 lines with stage badges, "See all" link to `#/active`) replaces the Active
  downloads card.
- [X] T024 [US2] Pipeline loop in `putio-sync-daemon/src/daemon.rs`: every 10 s build the
  pipeline and set `putio_sync_pipeline_items`. The loop reads `transfers_snapshot(60 s)`, the
  API reads `transfers_snapshot(10 s)`, so Put.io is asked at most every 60 s with no page
  open and every 10 s with pages open. Tests with mockito: with no viewer, ticks make one
  Put.io call per 60 s; with three simulated tabs polling `GET /active` every 5 s, at most one
  call per 10 s, never more than one Transfers page's one per 5 s (FR-028, SC-006). The
  `server` engine has no loop.

**Checkpoint**: US1 and US2 together are the MVP.

---

## Phase 5: User Story 3 — Pause and resume one transfer, at either stage (P2)

**Goal**: Pause stops a NAS-stage transfer within 5 s and keeps its partials; on a Put.io-stage
transfer it holds the NAS download. Nothing is sent to Put.io.

**Independent test**: pause a NAS-stage row (speed "—" within 5 s, Resume continues from the
partial) and a Put.io-stage row ("On hold"; its files arrive paused).

- [ ] T025 [P] [US3] Tests first in `putio-sync-download/tests/hold.rs` (mockito Put.io, with
  `expect(0)` on cancel, remove and clean): holding a Put.io-stage transfer writes a
  `pipeline_holds` row and sends nothing to Put.io (SC-008); when it finishes and syncs, its
  files are inserted `paused`; resume deletes the hold and resumes its queued rows; a hold
  survives reopening the database (FR-018).
- [ ] T026 [US3] Enqueue path in `putio-sync-download/src/manager.rs`: look up the transfer
  *before* inserting the queue row; if held, `add` with status `paused`.
- [ ] T027 [US3] `DownloadManager::pause_items(&[key])` and `resume_items(&[key])` in
  `putio-sync-download/src/manager.rs`. Results per key in order: pause → `paused` (NAS stage:
  `pause_files` then `stop_files`), `held` (Put.io stage or waiting for sync), `already`,
  `not_found`; resume → `resumed`, `already`, `not_found`. `folder:` and `file:` items are never
  held. Unknown keys are per-item `not_found`, not a request error. Log with `transfer_id`,
  `name`, `files` fields; count `putio_sync_pipeline_actions_total`. T025 passes.
- [ ] T028 [US3] Tests then `POST /active/pause` and `POST /active/resume` in
  `putio-sync-api/src/routes/active.rs`: body `{ "keys": [...] }`; 400 when `keys` is empty or
  has more than 500 entries; 200 with `results` in key order; auth required.
- [ ] T029 [US3] Tests then `POST /local/queue/{file_id}/pause` and `/resume` in
  `putio-sync-api/src/routes/local/queue.rs` (a running file stops and keeps its partial;
  resume keeps its place).
- [ ] T030 [US3] Stale holds in the pipeline loop in `putio-sync-daemon/src/daemon.rs`: delete
  holds whose transfer is in `ERROR` or gone from Put.io. Test.
- [ ] T031 [P] [US3] `putio-sync-arr/src/view.rs` and `service.rs`: `LocalProgress.paused` is
  true when the item's transfer is held or every unfinished file is paused; report status 0,
  `isFinished: false`, `leftUntilDone > 0`. Test in the arr view tests (FR-021).
- [ ] T032 [US3] UI: Pause/Resume button per row in `putio-sync-api/ui/pages/active.js`; held
  rows show the "On hold" badge with "Put.io keeps downloading; NAS download held"; paused rows
  show speed "—" and time left "Paused". Per-row Pause/Resume on
  `putio-sync-api/ui/pages/queue.js`. Add held and paused fixtures to
  `putio-sync-api/examples/mock_ui.rs`.

**Checkpoint**: single-transfer pause works at both stages.

---

## Phase 6: User Story 4 — Jump to the place to act (P2)

**Goal**: each row links to the Transfers page (Put.io stage) or to the Queue filtered to its
files (NAS stage).

**Independent test**: on the mock UI a Put.io-stage row opens Transfers, a NAS-stage row opens
Queue showing only its files.

- [ ] T033 [US4] Fill `links` in `build()` (`#/transfers?id=<id>`, `#/queue?transfer=<id>`;
  folder and file rows link to the queue filtered by their files) with a test in
  `putio-sync-download/src/pipeline.rs`.
- [ ] T034 [US4] Tests then, in `putio-sync-api/src/routes/local/queue.rs`:
  `GET /local/queue?transfer={id}` (only that transfer's files, via `download_sources`), and
  `POST /local/queue/{file_id}/retry` (failed → `pending` through `retry_files`, keeping its
  place; 409 if the file is not failed; auth required).
- [ ] T035 [P] [US4] `putio-sync-api/ui/pages/queue.js`: honour `?transfer=`, show "Showing
  files of <name>" with a "Show all" link; a Retry button on failed rows.
  `putio-sync-api/ui/pages/transfers.js`: when `?id=` is present, scroll to and highlight that
  row.
- [ ] T036 [US4] Link column in `putio-sync-api/ui/pages/active.js`. The page adds no retry,
  cancel or remove action of its own (FR-024).

---

## Phase 7: User Story 5 — Pause several, or pause everything (P3)

**Goal**: bulk pause/resume of ticked rows, and one lasting Pause NAS downloads switch that
replaces the Queue page's Pause all/Resume all.

**Independent test**: tick three rows and Pause (one toast lists each result); turn the switch
on (banner on Active, Status and Queue; nothing starts; restart keeps it on), then off
(everything resumes except individually paused files).

- [ ] T037 [P] [US5] Tests first in `putio-sync-download/tests/switch.rs`: switch on → running
  files stop and go back to `pending` (not `paused`); `process_queue` claims nothing; files
  queued by a sync stay queued; individually paused rows stay `paused`. Switch off → workers
  claim again, without touching any row. The switch survives reopening the database.
- [ ] T038 [US5] `DownloadManager::set_nas_paused(bool)` and `nas_paused()` in
  `putio-sync-download/src/manager.rs`: persisted in `daemon_flags` key `nas_downloads_paused`
  (`"1"`/`"0"`, missing means off), loaded at start; `process_queue` checks it before claiming.
  T037 passes.
- [ ] T039 [US5] `putio-sync-api/src/routes/local/queue.rs`: `POST /local/queue/pause` turns
  the switch on and returns `{ "nas_paused": true }`; `POST /local/queue/resume` turns it off
  and returns `{ "nas_paused": false }`; neither rewrites row statuses. `GET /local/queue`,
  `GET /active` and `GET /status` gain `nas_paused`. Update existing queue route tests.
- [ ] T040 [US5] Switch in the arr view (`putio-sync-arr/src/view.rs`): any item is paused while
  the switch is on. Test.
- [ ] T041 [US5] Set `putio_sync_nas_downloads_paused` in the pipeline loop in
  `putio-sync-daemon/src/daemon.rs`.
- [ ] T042 [US5] Bulk selection on `putio-sync-api/ui/pages/active.js` with `select.js`
  (feature 008): Pause and Resume in the selection bar send one request; one toast lists each
  key's result.
- [ ] T043 [US5] The switch on the Active page, and the banner "NAS downloads paused" with a
  Resume button on Active, Status and Queue (shared helper in `putio-sync-api/ui/app.js`).
- [ ] T044 [US5] `putio-sync-api/ui/pages/queue.js`: one switch button replaces Pause all and
  Resume all. Add a switch fixture to `putio-sync-api/examples/mock_ui.rs`.
- [ ] T045 [US5] Remove `QueueRepository::pause_all` and `resume_all` from
  `putio-sync-db/src/queue.rs` and every remaining caller (CLI, API, tests). Confirm that
  `grep -rn "pause_all\|resume_all" --include='*.rs' .` finds none.

---

## Phase 8: User Story 6 — Recently finished transfers (P3)

**Goal**: a Recently finished list with outcome, finish time and time spent in each stage.

**Independent test**: on the mock UI a finished fixture shows in Recently finished with stage
times; a failed one shows its reason; rows older than the retention are gone.

- [ ] T046 [US6] Tests in `putio-sync-daemon/src/daemon.rs` (or `putio-sync-daemon/tests/`): the
  first transition to done writes one `pipeline_finished` row with `outcome` `done`; a failed
  item that can still be retried (Put.io `ERROR`, NAS Failed) writes nothing and stays active;
  it writes one `failed` row (`reason` set) when its transfer is gone from Put.io while in
  `ERROR`; when its failed files are skipped, it writes `done` with "N files skipped" if at
  least one file is done, else `failed` with "nothing downloaded" (FR-002, R5, R6); a later
  write for the same key replaces it; rows older than `finished_retention_hours` are pruned.
  First run and prune (R6): with old transfers on Put.io whose files were all done days ago,
  the first tick writes nothing; a done item whose last download finished within the retention
  period is written on the first tick after a restart; a pruned row is not written again on
  later ticks;
  rows survive reopening the database (FR-026); a finished transfer whose files were all
  blocked by a rule (feature 009), and that finished within the retention period, is recorded
  once as `failed` with "nothing to download" and the blocked extensions.
- [ ] T047 [US6] Record transitions and prune in the pipeline loop in
  `putio-sync-daemon/src/daemon.rs`. Keep the previous tick's active keys in memory and write
  only items that were active then and are not now, plus the restart and blocked cases in R6.
  Columns: `added_at` = `transfer.created_at`, `putio_done_at` =
  `transfer.finished_at`, `queued_at` = earliest `download_sources.recorded_at`, `done_at` =
  last `downloads.downloaded_at` (for a failed item, when it left the active list; for the
  blocked case, `transfer.finished_at`). T046
  passes.
- [ ] T048 [US6] `recently_finished` in `GET /active` (`putio-sync-api/src/routes/active.rs`):
  `total_seconds`, `putio_seconds` (`putio_done_at - added_at`), `waiting_seconds`
  (`queued_at - putio_done_at`), `nas_seconds` (`done_at - queued_at`); a time is null when
  either end is NULL. Empty under the `server` engine. Extend `putio-sync-api/tests/active.rs`.
- [ ] T049 [US6] Recently finished section in `putio-sync-api/ui/pages/active.js` and finished
  fixtures (done and failed) in `putio-sync-api/examples/mock_ui.rs`.

---

## Phase 9: Polish & cross-cutting

- [ ] T050 [P] Performance test in `putio-sync-api/tests/active.rs`: 200 transfers and 5,000
  queue rows → `GET /active` under 200 ms.
- [ ] T051 [P] `README.md`: the Active page, pause and hold (Put.io keeps downloading), the
  Pause NAS downloads switch, and `finished_retention_hours`.
- [ ] T052 [P] `CHANGELOG.md` entry, with the upgrade note: rows paused by the old Pause all
  stay paused after the upgrade, so resume them once from the Queue page.
- [ ] T053 Run `make ci` and fix anything it reports.
- [ ] T054 Walk through [quickstart.md](quickstart.md) sections 2 and 3 on the mock UI (all 15
  checks, then `MOCK_PUTIO_DOWN=1`).

---

## Dependencies

```text
Setup (T001–T002) ─┐
                   ├─> Foundational (T003–T011) ─> US1 (T012–T018) ─> US2 (T019–T024) ─┬─> US3 (T025–T032) ─> US5 (T037–T045)
                   │                                                                   ├─> US4 (T033–T036)
                   │                                                                   └─> US6 (T046–T049)
                   └─────────────────────────────────────────────────────────────────────────────────────> Polish (T050–T054)
```

- T003 comes before T004–T006 and must fail first.
- `pause_all`/`resume_all` are removed only in T045, after T039 has moved their routes onto the
  switch.
- US1 needs T010, T011 and T008 (`pipeline_files`). US2 builds on US1's `build()`.
- US3 needs the stop signal (T006) and `pause_files`/`resume_files` (T009).
- US5 reuses US3's per-file pause (individually paused rows) and the arr view change (T031).
- US4 and US6 only need US2's loop and `build()`; they can run beside US3.
- Polish comes last.

## Parallel examples

- **Setup**: T001 and T002 together.
- **Foundational**: T008 and T009 together after T007; T004 can be written while T003's server
  fixture is built.
- **US1**: T018 (mock fixtures) beside T012–T016.
- **US3**: T025 (hold test) and T031 (arr view) together.
- **US4 / US6 / US3**: once US2 is done, the three phases touch different code and can run in
  parallel, except that each edits `ui/pages/active.js`; merge those edits one at a time.
- **Polish**: T050, T051, T052 together.

## Implementation strategy

1. **MVP = US1 + US2** (T001–T024): the read-only Active page with counts and the Status card.
   Stop and check it on the mock UI.
2. **US3** adds pausing; with the stop signal from Phase 2 this also fixes today's pause.
3. **US4** links, then **US5** bulk and the switch, then **US6** Recently finished.
4. Each story ends with `make ci` green and its independent test passing.
