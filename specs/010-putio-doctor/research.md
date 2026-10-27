# Research: Put.io Doctor

## R1. Where the doctor lives

- **Decision**: A new crate, `putio-sync-doctor`, that depends on `core`, `client`, `db`,
  `download` and `telemetry`. `daemon`, `api` and `cli` depend on it.
- **Rationale**: The doctor reads from every lower crate: the Put.io client, the queue and the
  database, the partial-file naming in `download`, and the config in `core`. `api` cannot use
  `daemon` and `core` cannot use `db`, so no existing crate can host the orchestration without
  depending upward. The checks' building blocks stay in the crates that own them (R2), and
  the new crate only runs them, grades them and applies fixes.
- **Alternatives**: `core`: would need `db` and `download`, which depend on `core` (a cycle).
  `daemon`: `api` and `cli`'s `doctor` command could not reach it without the daemon.
  Putting it in `cli`: then the API could not run it.

## R2. Building blocks per owning crate

| Need | Crate | New or reused |
|------|-------|---------------|
| Account info, storage, latency, `Date` and rate-limit headers | `client` | new `probe_account()` returning `AccountInfo` plus response metadata (status, elapsed, `Date`, `X-RateLimit-*`) |
| Flat walk of the whole file tree | `client` | new `walk_files(limit)`; uses the existing `list_files` (Put.io returns a folder's entries in one page) |
| Re-add (cancel + add magnet) | `client` | moved out of `api::handlers::transfers::readd_transfer` into `PutioApiClient::readd_transfer`, so both the API route and the doctor fix use it |
| Retry an errored transfer | `client` | existing `retry_transfer` |
| Empty folders | `core::cleanup::EmptyFolderCleanup` | existing `run(Manual, dry_run)` |
| Stalls | `core::stall::StallTracker` | existing; only the daemon has history (R6) |
| Unknown config keys, risky settings | `core` | new `config::lint(text) -> Vec<ConfigWarning>` |
| DB integrity | `db` | new `Database::quick_check()` (`PRAGMA quick_check`) |
| Failed downloads | `db` | new queries: failed queue rows, `downloads` rows with `status = 'failed'` or `retry_count >= 3` |
| Partial file naming | `download::files` | existing `is_partial_download` / `create_temp_path` |
| Disk usage | `telemetry::gauges::disk_usage` | existing |
| Watch-file checksum | `file-manager` | not used: `doctor` would then depend on `file-manager` for one function. Stuck watch files are matched on `processed_torrents.file_path` instead (R8) |

## R3. Unknown config keys

- **Decision**: Add the `serde_ignored` crate (dtolnay, tiny, no dependencies beyond `serde`)
  to `core`, and deserialize the raw file through it to collect every path serde ignored.
- **Rationale**: This is exact for the real `Config` type, including nested tables, aliases
  and `#[serde(default)]` fields, and needs no hand-kept list of keys.
- **Alternatives**: Diffing the file's keys against `toml::Value::try_from(Config::default())`
  reports keys wrongly because `Option` fields with `skip_serializing_if` are missing from the
  default. `#[serde(deny_unknown_fields)]` would turn a typo into a startup failure, which is
  a behaviour change outside this feature.

## R4. Rate limit and clock skew

- **Decision**: One probe (`GET /account/info`) gives auth, storage, latency, the `Date`
  header (skew) and any `X-RateLimit-Limit` / `X-RateLimit-Remaining` headers. The probe uses a
  client with no retries, so a 429 is seen rather than retried away.
- **Rationale**: Put.io does not document rate-limit headers. When they are missing the check
  is OK with "not reported", and a 429 on the probe is a Warning. One call keeps the doctor
  cheap.
- **Alternatives**: Reading `putio_sync_api_requests_total{outcome="rate_limited"}` from
  telemetry: this only means anything inside the daemon, and the counter never resets.

## R5. Shared tree walk and time limit

- **Decision**: One `walk_files` per run feeds three checks: duplicates, orphaned torrents
  and dead queue rows. Folders are listed with bounded concurrency (4) and an overall run
  deadline (default 5 min). Checks that depend on an unfinished walk report a Problem: "could
  not finish listing Put.io (timed out after N folders)".
- **Rationale**: SC-003 (a few thousand files in under a minute) and FR edge cases.
- **Empty folders** keep using the feature 005 dry run (its own walk) so its rules stay in one
  place.

## R6. Stalls outside the daemon

- **Decision**: The transfers check uses the daemon's `StallTracker` when there is one (daemon
  and API under `daemon`). The CLI has no history, so it reports errored transfers only and
  says in the summary that stall detection needs the daemon.
- **Rationale**: A single snapshot cannot tell a stalled transfer from a slow one (feature 007
  R-decision).

## R7. Single flight and the latest report

- **Decision**: `Doctor` holds a `tokio::sync::Mutex<()>` run guard and the latest report.
  A run request that finds a run in progress waits on the guard, then returns the report that
  finished while it waited instead of starting another.
- **Rationale**: FR edge case "two runs requested at once". This is simpler than a broadcast
  channel.

## R8. Stuck watch files

- **Decision**: With watch enabled, list `*.torrent` / `*.magnet` files directly in each watch
  path (not recursive, like the watcher) whose modification time is more than 1 h ago and whose
  path is not in `processed_torrents.file_path`.
- **Rationale**: The watcher deletes or archives a file after upload when configured to;
  otherwise it records the file in `processed_torrents`. A file in neither place older than an
  hour was never uploaded.

## R9. Orphaned torrents and duplicates

- **Decision**: Orphaned torrent: a file in the walk with extension `torrent` whose name stem
  (case-insensitive) matches no transfer name in the current transfer list. Duplicate: files
  (not folders) of at least 1 MiB grouped by `(name, size)` with more than one member.
- **Rationale**: Put.io does not keep a `.torrent` it turned into a transfer, so one left in the
  account usually failed to start. Both checks give a Warning and offer no fix.

## R10. Dead queue rows

- **Decision**: Queue rows whose `file_id` is not in the walk are confirmed one by one with
  `get_file_details`. A 404 makes the row dead. Any other error leaves the row out and adds a
  note. If the walk did not finish, the check reports Skipped.
- **Rationale**: Avoids false positives from shared or unreadable folders. The download manager
  already treats a failed lookup as "no longer exists" (manager.rs), which explains the log
  lines.

## R11. Orphan partial files

- **Decision**: Walk the download directory recursively (skipping the DB and hidden folders
  other than partial files) for `is_partial_download` files. A partial is an orphan when no
  queue row with status pending, downloading or paused maps to it via
  `create_temp_path(target/relative_path/filename)`, and its mtime is more than 1 h ago.
- **Fix**: delete the listed files. Before each delete, check again that the file still
  exists, still has the same size and mtime, and is still not claimed.

## R12. Fix protocol

- **Decision**: Two calls. `preview(fix_id)` re-runs that check's detection and returns the
  items with a `preview_id` (a hash of the fix id and sorted item ids). `apply(fix_id, items,
  confirm)` acts only on `items`, re-validating each one. Destructive fixes require
  `confirm: true` and a non-empty `items` from a preview, or they are refused (400). The
  empty-folder fix does a fresh dry run on apply and refuses (409, "the list changed, preview
  again") if it would remove a folder that was not in `items`, then runs the real cleanup.
- **Rationale**: FR-051/052. The cleanup service decides for itself what to delete, so the
  subset check is how the "only previewed items" rule holds there.

## R13. Metrics

- **Decision**: In `telemetry`, add a new `doctor` module:
  - `putio_sync_doctor_check_status{check}`: 0, 1, 2 or -1;
  - `putio_sync_doctor_check_items{check}`;
  - `putio_sync_doctor_last_run_timestamp_seconds`;
  - `putio_sync_doctor_last_run_duration_seconds`.

  `check` takes one of the fixed ids in data-model.md, which is about 20 values.
- **Rationale**: Principle V. The label set stays bounded, with no item names.

## R14. Schedule

- **Decision**: `[doctor] enabled = true`, `interval_minutes = 60`. The daemon runs the first
  check 2 minutes after start (so it does not compete with the startup sync), then every
  interval. The values are read again at each tick.
- **Rationale**: Hourly is cheap (one walk plus a few calls) and is quick enough for alerting.

## R15. UI budget

- **Decision**: `pages/doctor.js` stays at 9 KB or less, plus about 1.5 KB of CSS. On main the
  UI is 124 KB against the 150 KB limit, and other sessions are adding pages, so the doctor
  page uses the existing `h()`, toast and confirm helpers from `app.js` and adds no new
  component.
