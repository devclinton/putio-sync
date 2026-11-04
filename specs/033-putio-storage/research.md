# Research: Put.io Storage Management

## R1 Where the service lives

- **Decision**: new crate `putio-sync-storage` (client + core + db + telemetry).
- **Rationale**: eligibility needs the `downloads` and queue tables and the local
  filesystem. `core` (where the cleanup service lives) must not depend on `db`. The API,
  CLI and daemon all need the service.
- **Alternatives**: in `api` (CLI and daemon can't use it); in `doctor` (wrong concern);
  a trait seam in `core` over the database (more code than a crate).

## R2 Finding candidates

- **Decision**: reuse `PutioApiClient::walk_files(4, deadline)` from the doctor (010), with
  a 120 s deadline. `WalkedFile` gains `is_shared` (serde-skipped so the doctor's JSON is
  unchanged). Ancestors come from `parent_id` links in the walk.
- **Rationale**: one tested walker; it already stops at a deadline and records listing
  errors. Files on pages the walk does not read, or under folders it could not list, are
  simply not candidates, which is safe.
- **Alternatives**: start from the `downloads` table and fetch each file's details (one
  call per record, and no way to see shared ancestors or the date added without more
  calls); the empty-folder walk (it deletes as it goes).

## R3 Eligibility

- **Decision**: a walked non-folder file is eligible when: no ancestor (or itself) is
  shared; no ancestor is the save folder of an unfinished transfer (status not COMPLETED or
  SEEDING, as in empty-folder cleanup); it is not pending, downloading or paused in the
  queue; and some `downloads` row with `status = 'completed'`, the same `file_id` and
  `file_size` equal to the Put.io size has a `local_path` whose file exists with that size.
  "Unfinished transfer" covers its save folder and its own file id, but never the root:
  a transfer saving to the root would otherwise block every file in the account.
- **Rationale**: matches the spec's definition; the size check catches a re-uploaded or
  changed file with the same id and a local file that was replaced or truncated.

## R4 Order and stopping

- **Decision**: sort by Put.io `created_at` ascending (missing dates last), then id. Needed
  bytes = target free − free now, where target free = `ceil(size × target% / 100)`. Take
  files in order until the running total ≥ needed; cap at 1,000 files. When free ≥ target,
  the plan is empty with `needed_bytes = 0`.
- **Rationale**: "oldest first until a target is free" exactly; the cap bounds one run.

## R5 Plan and confirm

- **Decision**: the service keeps the latest plan in memory: a v4 UUID, the time, and each
  planned file with its ancestors and local path. `free(plan_id, file_ids)` refuses an
  unknown or older-than-30-minutes plan (409, "preview again"), ignores ids not in the
  plan, and for each file: fetches its details (gone → skipped "no longer on Put.io";
  size changed → skipped), re-checks the local copy and the queue, re-reads the transfer
  list once and skips files under unfinished transfers. Deletes in batches of 100; a failed
  batch marks its files failed. The plan is dropped after use.
- **Rationale**: the server never trusts paths or sizes from the browser and never deletes
  files the user did not see (FR-008).
- **Alternatives**: re-plan on confirm (could pick different files); send the whole plan
  back from the browser (would need re-validation of everything anyway).

## R6 Automatic run and gauges

- **Decision**: `putio-sync-daemon/src/schedule.rs` gets `run_storage(service)`: a 30 s
  ticker reusing `due_date`; every 15 minutes it reads account info and sets the gauges;
  at `run_at`, if `auto_free` is on and free < warn level, it plans and frees all planned
  files with trigger `schedule`. Reads settings every tick.
- **Rationale**: same scheduling semantics as the empty-folder cleanup (no catch-up).

## R7 Plan id

- **Decision**: `uuid` v4 (already in the lock file through `putio-sync-api`).

## R8 Levels and the doctor

- **Decision**: `[storage] warn_free_percent = 10`, `target_free_percent = 20`,
  `auto_free = false`, `run_at = "02:30"` (before the default 03:00 empty-folder cleanup,
  which then removes folders the run left empty). Validation: 1 ≤ warn < target ≤ 90,
  `run_at` HH:MM. The doctor's storage check: problem when free < 2%, warning when free <
  warn level, suggestion "Use Free up space on the Put.io page (or `putio-sync files
  free-space`) to delete old files that are already downloaded."
- **Rationale**: percentages work for every Put.io plan; the old fixed 90%/98% used map to
  the new 10%/2% free defaults.

## R9 Recording

- **Decision**: a run that deleted ≥ 1 file inserts a `storage_runs` row and one
  `storage_run_files` row per deleted file in one transaction, and one `download_history`
  row with action `deleted` per file (name from the path's last part). Recent runs: latest
  20 with their files.

## R10 UI

- **Decision**: a new module `ui/storage.js` exports `storageCard()` used at the top of the
  Put.io page and `storageNotice()` used by the Status page. Preview opens in the existing
  `confirm` dialog with the file list; confirm posts the plan. The Settings page gets a
  `storage` group in `GROUPS` (fields generated from the GET body like the others), plus
  help strings in the assistant.
- **Rationale**: keeps `putio.js` focused; reuses existing dialogs and forms.

## R11 CLI

- **Decision**: `putio-sync files free-space [--dry-run] [-y]` (next to `clean-empty`):
  prints the plan, asks unless `-y`, then frees it. Requires the database (exits with an
  error without one).
