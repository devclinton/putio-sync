# Research: Fixes from the Live NAS Logs

Each item is grounded in the code as of `001-local-ci-gate` (fc47c53) and in the NAS logs.

## R1. First request of each sync fails (FR-001–FR-003)

- **Finding**
  - `PutioApiClient` builds one `reqwest::Client` with only `.timeout(30s)`, so reqwest's
    defaults apply. Idle pooled connections are kept for 90 s, and there are no TCP
    keepalives.
  - Syncs run several minutes apart. Some middlebox (Docker bridge NAT, the router, or
    Put.io's load balancer) drops the idle connection without telling the client. The next
    request is written to a dead socket and fails with "error sending request". This
    happens on every sync, at every hour of the day, and always on attempt 1 only.
  - The retry budget is 3 attempts with 100 ms then 200 ms waits, about 0.3 s in total. A
    real blip longer than that loses the sync, which happened twice.
  - The log prints only the top-level `reqwest::Error` `Display`, which hides the hyper or
    io cause.
- **Decision**
  - Set `pool_idle_timeout(Duration::from_secs(20))` and `tcp_keepalive(Duration::from_secs(30))`
    on all API clients. That is the 3 builders in `client.rs` (`from_auth_config_with_base_url`,
    `with_base_url`, and the app-password constructor).
  - Retry policy, centralised in one `RetryPolicy` const:
    - Connection, timeout and request errors, plus HTTP 5xx and 429, are retryable.
    - Waits are 1 s, 2 s, 4 s, 8 s, 16 s: 6 attempts and about 31 s in total.
    - Add ±20% jitter.
    - Honour `Retry-After` on 429.
  - Log each retry and each final failure with the full `source()` chain, rendered by a small
    `error_chain(&dyn Error) -> String` helper in `putio-sync-client`.
  - reqwest never puts the auth header in error text. Keep it that way; the test asserts the
    token doesn't appear in the logged string.
- **Test (red first)**
  - A raw `tokio::net::TcpListener` test server answers one keep-alive request.
  - The server then closes that socket with a reset (`SO_LINGER 0`) when the next bytes
    arrive, which mimics a silent NAT drop.
  - Sending, idling past the idle timeout, and sending again must succeed on the first
    attempt with no retry warning.
  - Today it fails or needs a retry. A shorter test-only idle timeout, through a crate-private
    constructor, keeps the test fast.
  - A mockito test returns 503, 503, then 200, and must succeed. A 429 with `Retry-After: 1`
    must be honoured.
- **Alternatives considered**
  - `pool_max_idle_per_host(0)` disables reuse entirely. It is simpler, but adds a TLS
    handshake to every API call during sync, when the listing makes many calls.
  - Retrying only the first error silently would hide real problems.

## R2. "database is locked" when claiming (FR-004)

- **Finding**: `QueueRepository::claim_next_pending` does `BEGIN` (deferred) → `SELECT … LIMIT 1` → `UPDATE`.
  - Several workers can hold a read lock at the same time. When two of them try to upgrade
    to a write, SQLite returns `SQLITE_BUSY` immediately. To avoid deadlock it doesn't call
    the busy handler for a read→write upgrade, so sqlx's default 5 s `busy_timeout` doesn't
    help.
  - The database also runs in rollback-journal mode, where readers block writers.
- **Decision**
  - Replace the transaction with a single atomic statement:
    ```sql
    UPDATE download_queue SET status='downloading', updated_at=CURRENT_TIMESTAMP
    WHERE file_id = (SELECT file_id FROM download_queue WHERE status='pending'
                     ORDER BY priority DESC, created_at ASC LIMIT 1)
      AND status = 'pending'
    RETURNING *
    ```
    A single write statement takes the write lock up front, so the busy handler applies, and
    two workers can't claim the same row. The bundled SQLite in sqlx 0.8 supports
    `RETURNING`, which needs 3.35 or later.
  - In `Database::new`, set `journal_mode(Wal)`, `synchronous(Normal)` and an explicit
    `busy_timeout(10s)` on `SqliteConnectOptions`.
  - WAL creates `-wal` and `-shm` files next to `/downloads/.putiosync.db`. That's fine on
    the Unraid bind mount, which is local XFS/btrfs rather than NFS.
- **Test (red first)**
  - Use a file-backed temp database with 10 pending items.
  - Spawn 8 tasks that loop on `claim_next_pending` until it returns `None`, all starting
    together behind a `Barrier`.
  - Assert no errors, 10 unique claimed ids, and each claimed once. Today the test sees
    `database is locked`.
- **Alternatives considered**
  - `BEGIN IMMEDIATE` through `pool.begin_with("BEGIN IMMEDIATE")` was rejected: it needs
    sqlx 0.8.3+ and still takes two round trips.
  - A process-wide mutex around claims was rejected because it hides the database issue from
    the API process.

## R3. Already-uploaded watch files never cleaned up (FR-005, FR-006)

- **Finding**
  - `process_torrent_file` returns `Ok(false)` for all of these: "checksum already
    processed", "in-memory processed", "file gone", "not torrent/magnet", and "invalid
    magnet".
  - `scan_directories` runs the post-upload action only on `Ok(true)`.
  - A file that was uploaded once but never removed is therefore skipped on every scan
    forever. That happens after any run where the delete failed, or where the delete flag
    was off at the time.
  - The 41 files on the NAS are valid `magnet:` files, and the database at
    `/downloads/.putiosync.db` exists.
- **Decision**
  - Change the return type to an enum:
    `ProcessOutcome { Uploaded, AlreadyUploaded, Gone, Invalid(String), Unsupported }`.
  - `Uploaded` and `AlreadyUploaded` both run `handle_post_upload_action`.
  - `Invalid` is warned once per path per run.
  - The other outcomes stay at debug.
  - Add a `cleaned_up` counter to `WatchStats`.
  - Post-upload failures are tracked in a `HashSet<PathBuf>` of already-warned paths, so each
    is warned once per daemon run.
- **Test (red first)**
  - Seed `processed_torrents` with the checksum of a magnet file in a tempdir, with delete
    on.
  - One `scan_directories` must remove the file, with `files_processed == 0` and
    `cleaned_up == 1`.
  - The client is a mockito server that fails the test if it receives an add-transfer
    request.
  - Variants cover archive and both-off.

## R4. Watcher log noise (FR-007)

- **Decision**
  - Log the `Scan completed` summary in `watcher.rs:167` at `info!` only when
    `processed + cleaned_up + failed > 0`, and otherwise at `debug!`.
  - Apply the same rule to the duplicate `Watch scan -` line in `daemon.rs`.
  - Also move the per-directory "Found N file(s)" line to debug, since it fires every scan
    for the stuck files.
- **Test**: use a `tracing` capture layer through `tracing-subscriber`'s test writer, which is
  already a dependency. An idle scan must emit no INFO events.

## R5. Settings that lie (FR-011–FR-013)

- **Findings**
  - The sync interval: in `PutioDaemon::start`, `sync_interval = config.watch_interval`.
    `--download-interval` is parsed, compared to 300, warned about, and dropped.
    `DaemonConfig` has no download-interval field.
  - The banner in `main.rs:380-393` prints the raw CLI args with `eprintln!` before the
    config loads, so you get `Watch paths: []` and `Delete after: false`. The structured
    `info!` block in `commands/daemon.rs:118-133` already prints effective values, but it
    omits the delete flags and intervals.
  - The log level: `build_filter` replaces the config filter whenever `RUST_LOG` is set,
    and says nothing.
- **Decisions**
  - Add `download_interval: u64` to `DaemonConfig` (default 300) and use it for the sync
    loop. The worker loop keeps its own cadence (unchanged: `watch_interval`).
  - The CLI flag becomes `Option<u64>`: `None` falls back to the config, with no TOML key
    today. Simplicity: a TOML key isn't needed until someone asks for it.
  - Remove the `--download-interval` warning.
  - Delete the pre-config `eprintln!` banner in `main.rs`, leaving only "Entering daemon
    mode". Extend the `info!` block with the download interval, delete-after-download,
    delete-after-upload, archive dir, and blocked extensions.
  - `build_filter` returns the filter plus a `LevelSource` (`Config` or `Env`).
    `init_logging` logs `Log level: <directive> (from RUST_LOG)` or `(from config)` once,
    after the subscriber is installed.
  - Behaviour stays env-wins, the usual Rust convention. Making it visible is the fix.
- **Tests**
  - A unit test for the `build_filter` source, using the existing env-var test pattern
    serialised with a mutex.
  - A daemon config test that `--download-interval 30` reaches `DaemonConfig.download_interval`.
  - A test that the sync loop uses the download interval: refactor `start`, so the interval
    selection becomes a pure function and gets a unit test.

## R6. Executable guard (FR-008–FR-010)

- **Finding**: `sync_downloads` filters out folders only. `download.blacklist_patterns`
  exists in core config and the settings API but is never applied. Wiring it up is out of
  scope (Simplicity); this guard is separate and on by default.
- **Decision**
  - Add `blocked_extensions: Vec<String>` to `putio_sync_core::config::DownloadConfig`.
    It uses `#[serde(default = "default_blocked_extensions")]`, so a missing key gets the
    default list and an explicit `[]` disables the guard.
  - Mirror the field into `putio_sync_download::DownloadConfig`, and pass it in `daemon.rs:186`
    and in the CLI `download` command.
  - Add a pure `fn is_blocked(name: &str, blocked: &[String]) -> bool` in `putio-sync-download`.
    It compares the last extension, ASCII-lowercased.
  - In `sync_downloads`, check it before the downloaded/queue checks:
    - increment `stats.files_blocked`, a new field;
    - warn once per file id per manager lifetime (an `Arc<Mutex<HashSet<i64>>>`);
    - `continue`.
  - Blocked files never get a download record, so the post-download delete can't reach them.
  - Include `files_blocked` in the "Sync complete" line.
  - Hot reload picks up list changes, because the config sits behind `RwLock`.
  - The settings API (`settings.rs`) gains the field so the future UI can edit it.
- **Tests**
  - `is_blocked` table test:
    - `.EXE` is blocked; `a.exe.mkv` is not; `a.mkv.exe` is.
    - A name with no extension is not blocked.
    - An empty list blocks nothing.
  - A sync test against mockito: the files/list fixture has `.mkv`, `.exe` and `.MSI`.
    Only the `.mkv` lands in the queue, and `files_blocked == 2`.
  - A config test: a missing key gives the default, and `[]` gives empty.

## Dependencies

None new. `mockito`, `tempfile`, `tokio` and `tracing-subscriber` are already in the workspace.
