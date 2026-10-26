# Research: Put.io Empty-Folder Cleanup

## R1. How to decide a folder is empty

- **Decision**: Post-order walk from the root. A folder is *deletable* when it is not
  protected, not shared, old enough, its listing was read in full (no error, no `cursor`
  for a further page), it holds no files, and every sub-folder is deletable. Only the
  top-most deletable folder of each subtree is sent to `POST /files/delete`; Put.io removes
  its (empty) sub-folders with it, and the report lists all of them.
- **Rationale**: one delete call per empty subtree instead of one per folder; nothing that
  holds a file, or was not seen, can be inside a deleted folder.
- **Alternatives**: delete leaf by leaf and re-list parents (more calls, same result);
  trust Put.io's folder `size == 0` (it does not prove there are no zero-byte files or
  unfinished items).

## R2. Pagination

- **Decision**: `GET /files/list` returns up to Put.io's page size (1000) with a `cursor`
  when more exist. A folder with a `cursor` is not empty; sub-folders on the first page are
  still walked; later pages are not fetched.
- **Rationale**: the continue endpoint is not used anywhere in the client yet and its body
  format is unverified against a mock; a folder with over 1000 entries is rare and is
  never deleted.
- **Alternatives**: implement `/files/list/continue` (more surface, untested format).

## R3. Protected folders

- **Decision**: root (0, never a candidate), `watch.parent_folder_id`,
  `sync.remote_parent_id`, entries with `is_shared` (not descended), and the
  `save_parent_id` of every transfer whose status is not `COMPLETED` or `SEEDING`. If the
  transfer list cannot be read, the run fails before deleting anything.
- **Rationale**: user's choice of the built-in set, plus active transfer targets so a
  running transfer never loses its destination.

## R4. Minimum age

- **Decision**: Put.io `created_at` (`YYYY-MM-DDTHH:MM:SS`, UTC) parsed with `chrono`; a
  folder younger than `min_age_hours` or with no readable `created_at` is kept (and so are
  its parents). `now` is a parameter so tests fix it.

## R5. Where the code lives

- **Decision**: walk/delete in `putio-sync-client::cleanup` (Put.io operations, like
  `delete_folder_recursive`); service in `putio-sync-core::cleanup` (needs `Config`, which
  `client` cannot see); schedule in `putio-sync-daemon`; API and CLI are thin callers.
- **Rationale**: Constitution I. The API cannot depend on the daemon, so the shared
  guard/settings object sits in `core`, which both reach.

## R6. Daily schedule

- **Decision**: a loop ticks every 30 s and runs when today's (or yesterday's, across
  midnight) local `run_at` falls in `(previous tick, now]` and that date has not run yet.
  `is_due(prev, now, run_at, last_run_date)` is a pure function over `NaiveDateTime`.
- **Rationale**: no catch-up at startup (the first tick's `prev` is the start time),
  settings changes apply at the next crossing, a DST gap still crosses the time, and a DST
  overlap runs once thanks to `last_run_date`.
- **Alternatives**: sleep until the next run time (must be recomputed on settings changes
  and DST; harder to test); a cron crate (new dependency).

## R7. One run at a time

- **Decision**: `tokio::sync::Mutex<()>` in the service, taken with `try_lock`; busy →
  `CleanupError::AlreadyRunning` → API 409, schedule logs and skips.
- **Scope**: per process. The CLI builds its own service, so a CLI run alongside the
  daemon is not blocked (documented assumption).

## R8. Metrics

- **Decision**: `putio_sync_empty_folder_runs_total{trigger,result}`,
  `putio_sync_empty_folders_deleted_total`, `putio_sync_empty_folder_delete_failures_total`,
  `putio_sync_empty_folder_last_success_timestamp_seconds`. Dry runs are not counted.
  `trigger` ∈ {`schedule`, `manual`}; `result` reuses `SyncResult` (`success`, `failure`);
  a run with any failure is `failure`.
- **Rationale**: matches existing names (`watch_cleanups_total`,
  `last_success_timestamp_seconds`); "empty_folder" avoids confusion with the watch
  cleanup counters.

## R9. API shape

- **Decision**: `POST /putio/cleanup/empty-folders` with a required `{"dry_run": bool}`;
  synchronous; returns the report. A missing `dry_run` is a 400, so nothing deletes by
  accident.
- **Rationale**: runs take seconds; the UI awaits the result to show the count.
