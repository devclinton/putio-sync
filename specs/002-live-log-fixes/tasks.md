# Tasks: Fixes from the Live NAS Logs

**Input**: Design documents from `/specs/002-live-log-fixes/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/config-cli-logs.md, quickstart.md

**Tests**: Required. FR-014 and Constitution III say every fix starts with a failing test. Run
the test and confirm it is red before writing the fix.

**Order**: Phases follow the plan's phasing (value per effort), not the spec's story
numbering. Each story phase stands alone and can be committed separately.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Confirm the baseline: `make ci` passes on `002-live-log-fixes` before any change,
  and record the test count in this file's Implementation Notes

## Phase 2: Foundational

None. The stories touch separate crates, and no shared prerequisite blocks them.

---

## Phase 3: User Story 2 - Workers never fail on a busy queue (P1) 🎯 MVP

**Goal**: Concurrent claims never return "database is locked", and each item is claimed
exactly once (FR-004).

**Independent Test**: `cargo test -p putio-sync-db claim`

### Tests

- [X] T002 [US2] Add the test `claim_next_pending_is_safe_under_concurrency` to the test
  module of `putio-sync-db/src/queue.rs`.
  - Use a file-backed `Database::new(tempdir/.putiosync.db)`, not in-memory, with 10 pending
    items.
  - Spawn 8 tokio tasks that start together behind a `tokio::sync::Barrier`. Each loops on
    `claim_next_pending()` until it returns `None`.
  - Assert: no `Err`, 10 claimed `file_id`s in total, and all of them unique.
  - Use `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]`. Confirm the test fails
    with "database is locked".

### Implementation

- [X] T003 [US2] Replace the body of `claim_next_pending` in `putio-sync-db/src/queue.rs` with
  one statement, `UPDATE download_queue SET status='downloading', updated_at=CURRENT_TIMESTAMP
  WHERE file_id = (SELECT file_id FROM download_queue WHERE status='pending' ORDER BY priority
  DESC, created_at ASC LIMIT 1) AND status='pending' RETURNING *`, run via
  `query_as::<_, QueueItem>(...).fetch_optional(self.pool)`. Keep the doc comment and the
  error message prefix "Failed to claim next pending item".
- [X] T004 [US2] In `Database::new` in `putio-sync-db/src/lib.rs`, add the following to
  `SqliteConnectOptions`:
  - `.journal_mode(SqliteJournalMode::Wal)`
  - `.synchronous(SqliteSynchronous::Normal)`
  - `.busy_timeout(Duration::from_secs(10))`

  Leave in-memory test databases unaffected, or confirm WAL is ignored there.
- [X] T005 [US2] Run `cargo test -p putio-sync-db`. T002 must be green, and the existing queue
  tests must still pass.

**Checkpoint**: US2 is done. Commit.

---

## Phase 4: User Story 1 - Syncs don't fail on the first request (P1)

**Goal**: No retry after an idle period, a retry window of about 31 s for real failures, and
the full error cause in logs (FR-001–FR-003).

**Independent Test**: `cargo test -p putio-sync-client`

### Tests

- [X] T006 [P] [US1] Create `putio-sync-client/tests/stale_connection.rs`.
  - Write a raw `tokio::net::TcpListener` HTTP/1.1 server on `127.0.0.1:0`:
    - It answers the first request on a connection with
      `200 {"files":[],"parent":{...},"status":"OK"}` and keeps the connection alive.
    - After that, any further bytes on the same socket get a reset: `set_linger(Some(0))`,
      then drop.
    - New connections are served normally.
  - Send one list request, sleep longer than the client's idle timeout (use the test
    constructor from T009), and send a second.
  - Assert that the second request succeeds, and that the server saw 2 accepted connections.
    Today it reuses the dead socket.
- [X] T007 [P] [US1] In the tests of `putio-sync-client/src/client.rs`, add these mockito
  tests:
  - (a) 503, 503, 503, 503, then 200 must succeed. That's 5 attempts, which today's limit of
    3 can't reach.
  - (b) A 429 with `Retry-After: 1` followed by 200 must succeed, and must wait at least 1 s.
  - (c) After every attempt fails, the error string contains the inner cause and does not
    contain the token value.

  Use a test-only `RetryPolicy` with millisecond delays so the tests run fast.

### Implementation

- [X] T008 [US1] In `putio-sync-client/src/client.rs`, add `error_chain(err: &dyn
  std::error::Error) -> String`. It joins `err` and each `source()` with `": "`. Unit-test
  it with a nested `io::Error`.
- [X] T009 [US1] In `putio-sync-client/src/client.rs`, add a private `fn http_client() ->
  reqwest::Client` and use it in all three API constructors (`from_auth_config_with_base_url`,
  `with_base_url`, and the app-password one).
  - It sets `timeout(30s)`, `pool_idle_timeout(20s)` and `tcp_keepalive(30s)`.
  - Add a `#[doc(hidden)] pub fn with_pool_idle_timeout(self, idle: Duration)` builder (implemented instead of a `with_base_url_and_idle_timeout(token, base_url, idle:
    Duration)` for T006.
  - Apply the same `tcp_keepalive` to the download client in `authenticated_download_request`.
- [X] T010 [US1] In `putio-sync-client/src/client.rs`, replace `MAX_RETRIES` and
  `INITIAL_RETRY_DELAY_MS` with a `RetryPolicy { max_attempts: 6, initial_delay:
  Duration::from_secs(1), max_delay: Duration::from_secs(16) }`.
  - The policy is stored on `PutioApiClient`, and there is a test constructor that overrides
    it.
  - Rewrite `execute_with_retry` so that:
    - connection, timeout and request errors, plus 5xx and 429, are retryable;
    - delays double and get ±20% jitter (no new dependency: derive the jitter from
      `SystemTime` nanos);
    - a 429 with `Retry-After` uses that value instead;
    - each retry logs `warn!(url = %url_without_query_secrets, attempt, max_attempts,
      delay_ms, error = %error_chain(&e), "Put.io request failed, retrying")`;
    - the final failure is returned unchanged in type (`ClientError::HttpError`). The
      download manager logs it with `error_chain`.
- [X] T011 [US1] In `putio-sync-download/src/manager.rs`, change the two `error!` sites in
  `sync_downloads` ("Failed to list files" and "Error syncing") to log `error = %error_chain(&e)`.
  Use a re-export of `error_chain` from `putio-sync-client`.
- [X] T012 [US1] Run `cargo test -p putio-sync-client -p putio-sync-download`. T006 and T007
  must be green, and the existing `test_retry_on_server_error` and
  `test_max_retries_exceeded` tests must be updated to the new policy.

**Checkpoint**: US1 is done. Commit.

---

## Phase 5: User Story 4 - Executables are never downloaded (P1)

**Goal**: A default-on extension blocklist, configurable, warned about once per file
(FR-008–FR-010).

**Independent Test**: `cargo test -p putio-sync-download blocked` and
`cargo test -p putio-sync-core blocked`

### Tests

- [X] T013 [P] [US4] In the tests of `putio-sync-download/src/manager.rs` (or a new
  `putio-sync-download/src/blocklist.rs`), add the table test `is_blocked_matches_final_extension`:
  - `"Show S01E02.exe"` → true
  - `"setup.MSI"` → true
  - `"a.mkv.exe"` → true
  - `"a.exe.mkv"` → false
  - `"README"` → false
  - `".exe"` → false (a hidden file, with no extension)
  - any name with an empty list → false
- [X] T014 [P] [US4] In the tests of `putio-sync-core/src/config.rs`, add:
  - TOML without `blocked_extensions` → it equals the default list
    `["exe","msi","bat","cmd","com","scr","pif","vbs","js","jar","ps1","lnk","apk","dmg","pkg"]`;
  - `blocked_extensions = []` → empty;
  - `blocked_extensions = ["iso"]` → `["iso"]` (it replaces the default, not merges).
- [X] T015 [US4] Add a sync test in `putio-sync-download` with a mockito Put.io.
  - `/files/list?parent_id=0` returns three files: `Show S01E01.mkv`, `Show S01E02.exe`
    and `setup.MSI`.
  - Run `sync_downloads` twice with a temp database.
  - Assert:
    - the queue holds only the `.mkv` file;
    - `stats.files_blocked == 2` on each run;
    - there are no download records for the blocked ids;
    - a captured-log check shows the "Blocked file not downloaded" WARN appearing 2 times in
      total, not 4.

### Implementation

- [X] T016 [US4] Add `blocked_extensions: Vec<String>` to `DownloadConfig` in
  `putio-sync-core/src/config.rs`.
  - Use `#[serde(default = "default_blocked_extensions")]`, so a missing key gives the
    default list from T014 and an explicit `[]` gives empty.
  - Include it in `Default`, and in any builder or merge code in that file.
- [X] T017 [US4] Add `pub fn is_blocked(name: &str, blocked: &[String]) -> bool` in
  `putio-sync-download`. Use `Path::new(name).extension()`, ASCII-lowercased, and compare to
  the lowercased list. Export it from the crate root.
- [X] T018 [US4] In `putio-sync-download/src/manager.rs`:
  - add `blocked_extensions: Vec<String>` to the download crate's `DownloadConfig` (default:
    the same list; pass it from core);
  - add `files_blocked: usize` to `SyncStats`;
  - add `blocked_warned: Arc<Mutex<HashSet<i64>>>` to `DownloadManager`.

  In `sync_downloads`, before the "already downloaded" check, when `is_blocked(&file.name,
  &config.blocked_extensions)` is true:
  - increment `files_blocked`;
  - warn once per id with `warn!(file_id = file.id, name = %file.name, extension = %ext,
    "Blocked file not downloaded")`;
  - `continue`.

  Add `Blocked: {}` to both "Sync complete" lines.
- [X] T019 [US4] Wire the field through:
  - `putio-sync-daemon/src/daemon.rs:186`: `blocked_extensions:
    config.download.blocked_extensions.clone()`. The daemon receives the core config;
    follow the existing fields.
  - `putio-sync-cli/src/commands/download.rs:103`.
  - Every other `putio_sync_download::DownloadConfig { .. }` literal that the compiler flags.
- [X] T020 [US4] Expose `blocked_extensions: Vec<String>` in `putio-sync-api/src/routes/system/settings.rs`
  in both the GET response and the PUT body, next to `blacklist_patterns`. Update any
  settings test fixtures.
- [X] T021 [US4] Document the setting in `config.example.toml`, under `[download]`, with the
  comment from contracts/config-cli-logs.md.

**Checkpoint**: US4 is done. Commit.

---

## Phase 6: User Story 3 - Already-uploaded watch files get cleaned up (P1)

**Goal**: Leftover files that were already uploaded get the post-upload action; failures and
invalid files are warned once (FR-005, FR-006).

**Independent Test**: `cargo test -p putio-sync-file-manager already_uploaded`

### Tests

- [X] T022 [US3] Add `already_uploaded_file_is_deleted_not_reuploaded` to the tests of
  `putio-sync-file-manager/src/watcher.rs`.
  - Use a tempdir watch folder containing `x.magnet` with text `magnet:?xt=urn:btih:abc`, and
    a temp `Database` with `processed_torrents` seeded with
    `calculate_string_checksum("magnet:?xt=urn:btih:abc")`.
  - Point a mockito client at an add-transfer route that has `.expect(0)`.
  - Run `scan_directories(.., archive_dir=None, delete_after_upload=true, Some(&db))`.
  - Assert:
    - the file is gone;
    - `stats.files_processed == 0` and `stats.cleaned_up == 1`;
    - the mock is verified (it received no calls).
- [X] T023 [US3] Add variants of T022:
  - (a) with `archive_dir=Some(tmp/archive)`, the file is moved there;
  - (b) with delete off and no archive, the file stays, and there is no upload;
  - (c) a read-only watch dir, so the delete fails, scanned twice: one WARN in total
    (captured logs).

### Implementation

- [X] T024 [US3] In `putio-sync-file-manager/src/watcher.rs`, add `enum ProcessOutcome {
  Uploaded, AlreadyUploaded, Gone, Invalid(String), Unsupported }`. Change
  `process_torrent_file` to return `Result<ProcessOutcome>`:
  - a checksum match and an in-memory match → `AlreadyUploaded`;
  - a missing file → `Gone`;
  - a non-torrent or non-magnet file → `Unsupported`;
  - bad magnet content or a read error → `Invalid(reason)`.

  Update every caller, including `initial_scan` near line 861 and any daemon or API callers
  the compiler flags.
- [X] T025 [US3] In `scan_directories`:
  - `Uploaded` and `AlreadyUploaded` both call `handle_post_upload_action`.
  - `AlreadyUploaded` increments `cleaned_up` only when an action actually ran (delete or
    archive configured), and otherwise `files_skipped`.
  - Add `pub cleaned_up: usize` to `WatchStats`, and include it in `total()`.
  - Add `warned: Mutex<HashSet<PathBuf>>` to the watcher. Post-upload failures and `Invalid`
    outcomes `warn!(path = %p.display(), error/reason = ..)` only the first time for each
    path.
  - Log the cleanup as `info!(path = %..., "Removed already-uploaded watch file")`, or
    "Archived …" for an archive.

**Checkpoint**: US3 is done. Commit.

---

## Phase 7: User Story 5 - Logs and settings tell the truth (P2)

**Goal**: Quiet idle scans, a working `--download-interval`, a truthful banner, and a visible
log-level source (FR-007, FR-011–FR-013).

**Independent Test**: `cargo test -p putio-sync-file-manager idle_scan -p putio-sync-core log_level_source -p putio-sync-daemon download_interval`

### Tests

- [X] T026 [P] [US5] In the tests of `putio-sync-file-manager/src/watcher.rs`, add
  `idle_scan_logs_nothing_at_info`. Install a capture subscriber for the test, with
  `tracing_subscriber::fmt().with_writer(...)` and a shared buffer. Scan a folder where every
  file is `AlreadyUploaded` with no action configured. Assert that the output has no INFO
  lines from `putio_sync_file_manager`.
- [X] T027 [P] [US5] In the tests of `putio-sync-core/src/logging.rs`, add
  `log_level_source_reports_env_override`. With `RUST_LOG=info` set and the config at
  `debug`, `build_filter` returns `LevelSource::Env`. With the variable unset, it returns
  `LevelSource::Config`. Serialise with the existing env-var test mutex if there is one;
  otherwise add a `static ENV_LOCK: Mutex<()>`.
- [X] T028 [P] [US5] In the tests of `putio-sync-daemon/src/daemon.rs`, add
  `sync_loop_uses_download_interval`. Extract `fn loop_intervals(config: &DaemonConfig) ->
  (u64 /*sync*/, u64 /*worker*/)`, then assert that `download_interval: 30` and
  `watch_interval: 60` give `(30, 60)`.

### Implementation

- [X] T029 [US5] In `putio-sync-file-manager/src/watcher.rs`:
  - log the "Scan completed" summary at `info!` only when `files_processed + cleaned_up +
    files_failed > 0`, and otherwise at `debug!`;
  - move the per-directory "Found N .torrent/.magnet file(s)" line to `debug!`;
  - apply the same rule to the "Watch scan -" line in `putio-sync-daemon/src/daemon.rs:435`.
- [X] T030 [US5] In `putio-sync-daemon/src/daemon.rs`:
  - add `pub download_interval: u64` to `DaemonConfig` (doc: "Put.io sync interval in
    seconds", default `300`);
  - add `loop_intervals`, and use it in `start()` in place of `sync_interval =
    config.watch_interval`.
- [X] T031 [US5] In `putio-sync-cli`:
  - pass `download_interval` from the CLI into `DaemonConfig` in `commands/daemon.rs`, and
    delete the `if download_interval != 300 { warn!(...) }` block;
  - in `main.rs` `Commands::Daemon`, delete the pre-config `eprintln!` lines (download/watch
    interval, Watch paths, Filter, Skip existing/Delete after, Watch only/Download only),
    keeping only "Entering daemon mode";
  - in `commands/daemon.rs`, extend the `info!("Daemon configuration:")` block with the
    download interval, `delete_after_download`, `delete_after_upload`, the archive dir, and
    the blocked extensions.
- [X] T032 [US5] In `putio-sync-core/src/logging.rs`:
  - add `pub enum LevelSource { Config, Env }`;
  - make `build_filter` return `(EnvFilter, LevelSource, String /*effective directive*/)`;
  - after the subscriber is installed in the init function, emit one
    `info!(level = %directive, source = ?source, "Log level")`;
  - update the existing logging tests for the new return type.

**Checkpoint**: US5 is done. Commit.

---

## Phase 8: Polish

- [X] T033 [P] Update `README.md`: describe the executable guard and `blocked_extensions`, and
  note that `--download-interval` now works in daemon mode.
- [X] T034 [P] Check that no new log line contains secrets. Grep the diff for `token` inside
  `warn!`/`error!`/`info!` and confirm that only field names, never values, appear.
- [X] T035 Run `make ci` end-to-end, then quickstart.md §1-2. Record the results in
  Implementation Notes below.

## Dependencies

- T001 comes before everything else.
- The US2, US1, US4 and US3 phases touch different crates (db, client, download/core,
  file-manager), so they can run in parallel. Two overlaps:
  - T011 (US1) and T018 (US4) both edit `putio-sync-download/src/manager.rs`. Do them in
    sequence.
  - T024 changes `process_torrent_file`'s signature, which the daemon may call. Finish it
    before T029/T030 touch `daemon.rs`.
- US5 depends on US3 (it uses `cleaned_up` in the idle rule) and on US4 (the banner lists the
  blocked extensions).
- T035 depends on all of the above.

## Parallel Examples

- **US1**: T006 and T007 can be written in parallel (separate files).
- **US4**: T013 and T014 can be written in parallel.
- **US5**: T026, T027 and T028 are in three separate crates, so they can run in parallel.
- **Across stories**: US2 and US1 can be implemented at the same time.

## Implementation Strategy

- **MVP**: US2 (T002–T005), the smallest change. It removes every ERROR line in the NAS logs
  except the 2 lost syncs.
- Then US1, which removes 12.5k warnings and the lost syncs.
- Then US4, which closes the security hole.
- Then US3, which clears the 41 stuck files.
- Then US5.
- Commit after each checkpoint. Deploying to the NAS is out of scope; see quickstart.md §4
  for post-deploy checks.

## Implementation Notes

- **Baseline**: `make ci` passed with 314 tests on `001-local-ci-gate` (fc47c53).
- **Result (2026-10-25)**: `make ci` exit 0. 333 passed, 0 failed, 1 ignored. Quickstart §2
  targeted runs are all green. §3 (local daemon smoke test) and §4 (NAS) were not run;
  §4 needs a deploy.
- **Deviations from the tasks**:
  - T006/T009: the test hook is a builder, `with_pool_idle_timeout(idle)`, instead of a
    `with_base_url_and_idle_timeout` constructor. The test also asserts that the server saw
    no write on the stale socket. The fixture needed `"status":"OK"`.
  - T011: `list_all_files` used to flatten client errors to a string
    (`DownloadError::ApiError(e.to_string())`), which lost the cause. It now wraps them as
    `DownloadError::ClientError`, so `error_chain` shows the full cause.
  - T017: added `blocked_extension()`, which returns the matched extension for the WARN;
    `is_blocked` wraps it. The default list is duplicated in `putio-sync-download`, because
    that crate doesn't depend on core. A comment says to keep the two in sync.
  - T025: a failed cleanup of an already-uploaded file counts as `files_failed`, and the WARN
    is logged once per path. Post-upload logs are now structured
    (`Deleted uploaded watch file`, `Removed already-uploaded watch file`, `Archived …`).
  - T026: logs are captured with a custom `Layer`, not with `fmt().with_writer`.
  - T031: daemon mode never used `watch.archive_directory`. It now warns at startup when it
    is set, instead of listing it in the config block.
  - T032: the line is `Log level level=<directive> source=Env|Config`. quickstart §3 shows
    an older wording.
