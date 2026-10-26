---

description: "Task list for feature 003: Telemetry and Prometheus Monitoring"
---

# Tasks: Telemetry and Prometheus Monitoring

**Input**: Design documents from `specs/003-telemetry-prometheus/`

**Prerequisites**:
- [plan.md](plan.md)
- [spec.md](spec.md)
- [research.md](research.md) (R1–R12)
- [data-model.md](data-model.md)
- contracts: [metrics.md](contracts/metrics.md), [config.md](contracts/config.md), [alerts.md](contracts/alerts.md)
- [quickstart.md](quickstart.md)

**Tests**: included. The spec gives an Independent Test for every story. The
constitution requires mock-only tests (Principle III) and a passing `make ci` (Principle
II). Within each story, write the tests first and confirm they fail before implementing.

**Organization**: tasks are grouped by user story. The phases follow spec priority: US1
(P1), US2 (P1), US3 (P2), US4 (P2), US5 (P3).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, and no dependency on an unfinished task)
- **[Story]**: US1–US5 from spec.md
- Paths are relative to the repo root, unless they start with `~/dev/home-network-new/`.

## Ground rules for every task

- Rust edition 2021, so no let-chains. `make ci` = fmt, clippy `-D warnings`, test and a
  release build.
- Metric names, types, labels and buckets come from
  [contracts/metrics.md](contracts/metrics.md). Every label value comes from the closed
  sets in [data-model.md](data-model.md#closed-label-sets), as `&'static str` from an
  enum. Never put a file name, id, path, URL or token in a label.
- Test helpers build a fresh `Registry` through `putio_sync_telemetry::Telemetry::new()`.
  Don't assert on the global, because tests run in parallel.
- `~/dev/home-network-new` already has unrelated uncommitted changes, including in
  `group_vars/monitoring_server/main.yml`. Edits there are additive only. Never revert,
  reformat or commit other changes. Commit nothing in that repo without the user's
  approval.

---

## Phase 1: Setup

**Purpose**: create the new crate and set up dependencies.

- [X] T001 Create the `putio-sync-telemetry` crate:
  - Add `putio-sync-telemetry/Cargo.toml`. Use workspace version, edition, authors and
    license. Dependencies:
    - `prometheus = { version = "0.14", default-features = false }`
    - `tokio = { workspace = true }`
    - `axum = "0.7"`
    - `tracing = { workspace = true }`
    - `rustix = { version = "1", features = ["fs"] }`
  - Dev-deps: `tokio` with `test-util`, `reqwest = { workspace = true }`, `tempfile = "3.8"`.
  - Add an empty `putio-sync-telemetry/src/lib.rs`.
  - Add `"putio-sync-telemetry"` to `members` and `prometheus` to
    `[workspace.dependencies]` in `Cargo.toml`.
  - Run `cargo build -p putio-sync-telemetry` to confirm it resolves. If 0.14 can't be
    resolved, use the newest 0.13 with the same features and note it in
    research.md R1.
- [X] T002 [P] Pass the commit SHA into every image build (R6):
  - In `Dockerfile`, add `ARG PUTIO_SYNC_GIT_SHA=unknown` and
    `ENV PUTIO_SYNC_GIT_SHA=$PUTIO_SYNC_GIT_SHA` in the build stage, before `cargo build`.
    That way `option_env!("PUTIO_SYNC_GIT_SHA")` sees it.
  - In `Makefile`, add `GIT_SHA ?= $(shell git rev-parse --short HEAD 2>/dev/null || echo unknown)`
    and `--build-arg PUTIO_SYNC_GIT_SHA=$(GIT_SHA)` to `docker-build`, `docker-build-cache`,
    `docker-build-local-cache` and `docker-build-multiarch`.
  - In `.github/workflows/docker.yml`, add `build-args: PUTIO_SYNC_GIT_SHA=${{ github.sha }}`
    to the `docker/build-push-action` step. The NAS pulls this image.
  - Without these, `build_info{commit}` is always `unknown`.
- [X] T003 [P] Add `putio-sync-telemetry = { path = "../putio-sync-telemetry" }` to the
  `[dependencies]` of these files:
  - `putio-sync-client/Cargo.toml`
  - `putio-sync-db/Cargo.toml`
  - `putio-sync-download/Cargo.toml`
  - `putio-sync-file-manager/Cargo.toml`
  - `putio-sync-core/Cargo.toml`
  - `putio-sync-cli/Cargo.toml`

---

## Phase 2: Foundational (blocks every story)

**Purpose**: the metric registry, label enums, listener, gauge tick and `[metrics]`
settings that every story builds on.

**⚠️ CRITICAL**: no story work starts until this phase is done.

- [X] T004 [P] Write the contract test `putio-sync-telemetry/tests/contract.rs`. It
  should fail until T005–T007 exist.
  - Build `Telemetry::new()` with `init(workers=2, version, commit)` and record one value
    in every family.
  - Encode the output and assert invariants 1–5 from
    [contracts/metrics.md](contracts/metrics.md#invariants-tested):
    - every family is present at `basic`;
    - detailed-only families and `worker` labels are absent at `basic`;
    - each label value is in its closed set;
    - no label value contains `http`, `/` or the fixture names;
    - the fake token `"tok_SECRET"` passed to the helpers never appears;
    - counters don't decrease.
  - Also check that `last_success_timestamp` and the disk gauges are absent before they
    are first set.
- [X] T005 [P] Write the closed label enums in `putio-sync-telemetry/src/labels.rs`. Each
  has `fn as_str(&self) -> &'static str`:
  - `ApiOp`: ListFiles, GetFile, DeleteFiles, Upload, AddTransfer, ListTransfers,
    GetTransfer, CancelTransfers, RetryTransfer, AccountInfo, Other.
  - `ApiOutcome`: Success, ClientError, ServerError, NetworkError.
  - `RetryReason`: Network, ServerError, RateLimited.
  - `SyncResult`: Success, Failure.
  - `SyncKind`: Found, Queued, Skipped, Blocked, Failed.
  - `QueueState`: Pending, Downloading, Paused, Failed.
  - `FailReason`: Network, Http, Io, Integrity, Other.
  - `DbOp`: Queue, History, Uploads, Migrate, Other.
  - `Detail`: Basic, Detailed. Implement `FromStr`, accepting exactly `basic` and
    `detailed`.

  Then add `ApiOp::from_path(path: &str) -> ApiOp`, matching the Put.io URL path with the
  `/v2` prefix and the query removed:

  | Path | ApiOp |
  |---|---|
  | `/files/list` | ListFiles |
  | `/files/delete` | DeleteFiles |
  | `/files/upload`, or an upload host | Upload |
  | `/files/{n}` | GetFile |
  | `/transfers/add` | AddTransfer |
  | `/transfers/list` | ListTransfers |
  | `/transfers/cancel` | CancelTransfers |
  | `/transfers/retry` | RetryTransfer |
  | `/transfers/{n}` | GetTransfer |
  | `/account/info` | AccountInfo |
  | anything else | Other |

  Add unit tests for each mapping.
- [X] T006 Implement `Telemetry` in `putio-sync-telemetry/src/lib.rs`:
  - It holds a `prometheus::Registry` and every family from the contract, with the names,
    help text and buckets given there.
  - `Telemetry::new()` builds a standalone instance for tests.
  - `pub fn global() -> &'static Telemetry` is a `OnceLock`. Its registry has no metrics
    until `init` is called.
  - `pub fn init(&self, workers: usize, version: &str, commit: &str)` does three things:
    - sets `build_info{version,commit}=1`, `start_time_seconds` and `download_workers`;
    - pre-registers `worker="0".."workers-1"` series, but only when detailed is on;
    - zero-initialises every closed label combination for the counters, and for the
      labelled gauges `last_sync_files{kind}` and `queue_items{state}`, so every family
      appears in the first scrape (contract invariant 1). The only families left absent
      are `last_success_timestamp_seconds` and the disk gauges, as the contract says.
  - `pub fn set_detail(&self, Detail)` / `pub fn detailed(&self) -> bool` use an
    `AtomicBool`.
  - `pub fn encode(&self) -> String` uses `TextEncoder`.
  - Re-export `labels::*`.
- [X] T007 Write the helper modules. Each takes `&Telemetry`, with `global()` wrappers for
  call sites. None of them may allocate a label `String`.
  - `putio-sync-telemetry/src/api.rs`:
    - `record_attempt(op, outcome, elapsed)` observes the overall histogram, plus the
      per-op histogram only when `detailed()`;
    - `record_retry(reason)`.
  - `putio-sync-telemetry/src/sync.rs`:
    - `record_run(result, elapsed, counts: [(SyncKind, usize); 5])` sets the
      `last_sync_*` gauges, and sets `last_success_timestamp` on success;
    - `file_blocked()`.
  - `putio-sync-telemetry/src/download.rs`:
    - `ActiveGuard` increments `downloads_active` on creation and decrements on `Drop`;
    - `bytes(n)`, `completed(elapsed)`, `failed(FailReason)`;
    - `worker_busy(id, bool)` and `worker_inflight(id, bytes)`, which do nothing unless
      `detailed()`.
  - `putio-sync-telemetry/src/watch.rs`: `scan()`, `uploaded()`, `cleaned_up()`,
    `invalid()`, `cleanup_failed()`.
  - `putio-sync-telemetry/src/db.rs`: `error(DbOp)`.
- [X] T008 Implement `putio-sync-telemetry/src/server.rs`:
  - `pub struct MetricsSettings { enabled, bind: SocketAddr, path: String, detail: Detail }`,
    with `Default` = `true`, `0.0.0.0:9742`, `/metrics` and `Basic`.
  - `pub async fn bind(settings) -> io::Result<TcpListener>`.
  - `pub fn serve(listener, path, telemetry: &'static Telemetry) -> JoinHandle<()>`. It
    runs an axum router with only `GET <path>`, which returns `telemetry.encode()` with
    `Content-Type: text/plain; version=0.0.4; charset=utf-8`. Other paths get 404.
  - No lock is held across `.await`.
- [X] T009 Implement the refresh tick in `putio-sync-telemetry/src/gauges.rs`:
  - `pub fn spawn_refresh<F, Fut>(telemetry, download_dir: PathBuf, queue_counts: F) -> JoinHandle<()>`,
    where `F: Fn() -> Fut` and `Fut: Future<Output = Option<Vec<(QueueState, i64)>>>`.
  - It runs once straight away, then every 15 s (R7). It sets `queue_items{state}`.
  - Disk space uses `rustix::fs::statvfs(download_dir)`: free = `f_bavail*f_frsize`,
    total = `f_blocks*f_frsize`.
  - On a statvfs error, it removes both disk gauges and logs `warn!` once. A later success
    re-adds them and resets the warned flag.
  - Unit-test it with a temp dir and a closure that returns fixed counts, using paused
    tokio time.
- [X] T010 [P] Write the listener test `putio-sync-telemetry/tests/server.rs`:
  1. A scrape over `reqwest` returns 200 with the content type, and contains
     `putio_sync_build_info`.
  2. A bad path returns 404.
  3. `bind()` on a port that's already in use returns `Err` and doesn't panic (FR-008).
  4. With 2,000 concurrent `bytes()` calls running in a loop, a scrape returns in under
     1 s (SC-002).
- [X] T011 Add `MetricsConfig` to `putio-sync-core/src/config.rs`:
  - It is a `[metrics]` table: `enabled`=true, `bind`="0.0.0.0:9742", `path`="/metrics",
    `detail`="basic". Use `#[serde(default)]` on `Config`.
  - Add `pub fn resolve_metrics(&self) -> Result<Resolved<MetricsSettings>, CoreError>`.
    It applies the env overrides from [contracts/config.md](contracts/config.md):
    - `PUTIO_SYNC_METRICS_ENABLED` accepts `true`/`false`/`1`/`0`;
    - `PUTIO_SYNC_METRICS_BIND` must parse as `SocketAddr`;
    - `PUTIO_SYNC_METRICS_PATH` must start with `/` and contain no whitespace;
    - `PUTIO_SYNC_METRICS_DETAIL` must be `basic` or `detailed`.
  - Record a `source: Config|Env` for each value, reusing `logging::LevelSource`.
  - Invalid input in TOML or env returns `CoreError::ConfigError("invalid <setting>
    '<value>': expected one of <allowed>")` (FR-011).
  - Add tests, reusing the `ENV_LOCK` pattern from `logging.rs`: defaults; env beats
    config; each invalid case gives the exact message.

**Checkpoint**: `cargo test -p putio-sync-telemetry -p putio-sync-core` passes, and the
contract test is green.

---

## Phase 3: User Story 1 – The daemon reports its health as metrics (Priority: P1) 🎯 MVP

**Goal**: in both `daemon` and `server` modes, a scrape shows real API, sync, download,
watch, DB, queue, disk and build metrics.

**Independent Test**: run against a mock Put.io with a temp watch folder and a temp
download folder. Sync, queue, download and watch-upload one file each. Then scrape and
check each family's value (quickstart §1–2).

### Tests for User Story 1 ⚠️ write first, then confirm they fail

- [X] T012 [P] [US1] Write `putio-sync-client/tests/metrics.rs`. Use a mockito server
  that answers `/v2/files/list` with 500, 500, then 200, and use a fast `RetryPolicy`.
  Assert:
  - `api_requests_total{operation="list_files",outcome="server_error"}==2`;
  - `{outcome="success"}==1`;
  - `api_retries_total{reason="server_error"}==2`;
  - the histogram count is 3.

  Then test a 429 with Retry-After, which gives `reason="rate_limited"`, and a refused
  connection, which gives `outcome="network_error"` and `reason="network"`.
- [X] T013 [P] [US1] Write `putio-sync-download/tests/metrics_sync.rs`, reusing the
  mockito fixtures from `tests/blocked_sync.rs`. Use one 4 KiB file and one `.exe`, then
  run `sync_downloads` twice and process the queue. Assert:
  - `sync_runs_total{result="success"}==2`;
  - `last_sync_files{kind="blocked"}==1`;
  - `files_blocked_total==1`, since the warning is once per file id;
  - `downloads_completed_total==1`;
  - `downloaded_bytes_total==4096`;
  - `downloads_active==0` afterwards.

  Then add a size-mismatch download that gives `downloads_failed_total{reason="integrity"}==1`.
- [X] T014 [P] [US1] Extend `putio-sync-file-manager/tests/watch_cleanup.rs`, or add
  `tests/watch_metrics.rs`. One upload, one already-uploaded cleanup, one invalid file, and
  one cleanup failure on a read-only dir. Each one increments its `watch_*_total` counter
  once, and `watch_scans_total` matches the number of scans.
- [X] T015 [P] [US1] Write `putio-sync-db/tests/metrics.rs`. Close the pool (or use an
  invalid table through a test-only path), then call `queue().add(...)`. Assert
  `db_errors_total{operation="queue"}==1`. Also test the new `queue().counts_by_status()`,
  which returns `pending=2, failed=1` for seeded rows.

### Implementation for User Story 1

- [X] T016 [US1] Instrument `execute_with_retry` in `putio-sync-client/src/client.rs`:
  - Compute `ApiOp::from_path(url.path())` once.
  - For every attempt, time it and call `api::record_attempt(op, outcome, elapsed)`.
    Outcomes:
    - success for 2xx;
    - client_error for every 4xx, including a 429 whether or not it is retried (the retry
      counter's `reason="rate_limited"` carries that detail);
    - server_error for 5xx;
    - network_error for a `reqwest` send error.
  - On each retry, call `api::record_retry(...)` with `Network`, `ServerError` or
    `RateLimited`, next to the existing `warn!`.
  - Instrument the direct upload request in `putio-sync-client/src/files.rs`
    (`upload_file`) the same way, with `ApiOp::Upload`.
- [X] T017 [US1] Instrument `putio-sync-download/src/manager.rs`:
  - Time the whole `sync_downloads` run. Call `sync::record_run(Success|Failure, elapsed,
    counts)`, mapping `found=total_files`, `queued=files_downloaded`,
    `skipped=files_skipped`, `blocked=files_blocked` and `failed=files_failed`. Record
    `Failure` when the run returns `Err`.
  - Call `sync::file_blocked()` only where the "Blocked file not downloaded" warning is
    emitted for the first time for a file id, so it counts once per file id.
- [X] T018 [US1] Instrument the download path in `putio-sync-download/src/stream.rs` and
  `putio-sync-download/src/manager.rs` (`download_file`):
  - Hold an `ActiveGuard` for the whole download.
  - Call `download::bytes(chunk.len())` after each successful `write_all` in
    `stream_download`.
  - Call `download::completed(elapsed)` only after the atomic `tokio::fs::rename`
    succeeds (Principle IV).
  - On error, call `download::failed(reason)`, mapping `DownloadError` as follows:

    | Error | Reason |
    |---|---|
    | `HttpError` with a status | Http |
    | `HttpError` with connect/timeout/body | Network |
    | `IoError` | Io |
    | size/length mismatch | Integrity |
    | otherwise | Other |
- [X] T019 [US1] Instrument `putio-sync-file-manager/src/watcher.rs`. In
  `scan_directories`:
  - call `watch::scan()` once per scan;
  - map `ProcessOutcome::Uploaded` to `uploaded()`;
  - map a successful post-upload cleanup or `AlreadyUploaded` cleanup to `cleaned_up()`;
  - map `Invalid(_)` to `invalid()`, only where `warn_once` fires for the first time;
  - map archive/delete errors to `cleanup_failed()`.
- [X] T020 [US1] Count DB errors in `putio-sync-db`:
  - Add `pub(crate) fn tracked(op: DbOp) -> impl Fn(sqlx::Error) -> DbError` in
    `putio-sync-db/src/error.rs`. It calls `telemetry::db::error(op)` and then builds the
    existing `DbError` value.
  - Replace the `map_err` closures on sqlx calls in each file:

    | File | `DbOp` |
    |---|---|
    | `src/queue.rs` | Queue |
    | `src/history.rs`, `src/downloads.rs` | History |
    | `src/torrents.rs` | Uploads |
    | `src/lib.rs`, `src/schema.rs` | Migrate |
    | `src/stats.rs` | Other |

    Keep the existing error messages.
- [X] T021 [US1] Add `pub async fn counts_by_status(&self) -> DbResult<Vec<(String, i64)>>`
  to `putio-sync-db/src/queue.rs`. It runs
  `SELECT status, COUNT(*) FROM download_queue WHERE status IN
  ('pending','downloading','paused','failed') GROUP BY status`.
- [X] T022 [US1] Write a shared CLI helper `putio-sync-cli/src/telemetry.rs`:
  - Signature:
    ```rust
    pub async fn start(
        config: &Config,
        queue_db: Arc<OnceLock<Arc<Database>>>,
        workers: usize,
        download_dir: PathBuf,
    ) -> Result<Option<SocketAddr>, CoreError>
    ```
  - It doesn't need an open database. `queue_db` is filled whenever the caller opens one,
    which may be later or never.
  - It calls `config.resolve_metrics()` and returns the error on failure (startup fails).
  - When metrics are disabled, it logs `info!(source=?, "Metrics disabled")` and returns
    `Ok(None)`.
  - Otherwise:
    1. `global().set_detail(detail)`.
    2. `global().init(workers, env!("CARGO_PKG_VERSION"), option_env!("PUTIO_SYNC_GIT_SHA").unwrap_or("unknown"))`.
    3. `server::bind`. On `Err`, log one `error!(bind=%addr, error=%e, "Metrics listener failed to start")`
       and continue without the listener (FR-008). On `Ok`, `serve` and
       `info!(bind, path, detail, source, "Metrics enabled")`.
    4. `gauges::spawn_refresh(...)`, with a closure that returns `None` while
       `queue_db.get()` is empty. Once it's set, the closure maps
       `db.queue().counts_by_status()` to `QueueState`. Disk gauges refresh either way.
    5. Return `Ok(Some(listener.local_addr()))` when the listener started, otherwise
       `Ok(None)`. Returning the bound address lets tests use port 0.

  Declare the module in `putio-sync-cli/src/main.rs`.
- [X] T023 [US1] Call `telemetry::start(...)` in both modes, early and unconditionally:
  - Both modes use the configured `config.download.workers` and the download directory.
  - **Daemon mode** (`putio-sync-cli/src/commands/daemon.rs`):
    1. Call it right after `daemon_config` is built, using `download_dir`.
    2. Set `queue_db` once the daemon has opened its database at
       `download_dir/.putiosync.db`.
  - **Server mode** (`putio-sync-cli/src/commands/server.rs`):
    1. Call it before the background download task is spawned, and before `axum::serve`,
       using `config.download.target_directory`.
    2. Pass a clone of `queue_db` into the download task.
    3. Inside that task, set `queue_db` right after `Database::new(...)` succeeds (around
       line 323).
    4. The task returns early when no token or credentials are set. The listener must
       already be running by then (FR-007), and the queue gauges stay at their
       zero-initialised values.
  - Nothing is added to the API router (FR-007).
  - Out of scope, but note it in Implementation Notes: server mode opens `.putiosync.db`
    relative to the working directory, while the daemon uses
    `download_dir/.putiosync.db`. The two modes can therefore report different queue
    contents.
- [X] T024 [US1] Write the end-to-end test `putio-sync-cli/tests/metrics_e2e.rs`, or add
  it to an existing CLI integration test if that harness fits better:
  - Call `telemetry::start` with `PUTIO_SYNC_METRICS_BIND=127.0.0.1:0`, and scrape the
    returned `SocketAddr`.
  - Before `queue_db` is set, a scrape still succeeds and shows
    `putio_sync_queue_items{state="pending"} 0`. This covers server mode with no token.
  - Seed 2 pending rows in a temp DB and set `queue_db`. After the next refresh (paused
    tokio time), `putio_sync_queue_items{state="pending"} 2` is present before any sync
    (FR-004), and `putio_sync_disk_free_bytes` is greater than 0.

**Checkpoint**: US1 is done when `make ci` passes and quickstart §2 shows the expected
families.

---

## Phase 4: User Story 2 – Home Prometheus scrapes the NAS instance (Priority: P1)

**Goal**: the home Prometheus keeps a history of putio-sync, and the operator has a page
explaining how it's monitored.

**Independent Test**: `make -C ~/dev/home-network-new check` passes. After deploy, the
`putio_sync` target is UP.

- [X] T025 [US2] Add the `putio_sync` scrape job from
  [contracts/alerts.md](contracts/alerts.md#scrape-job-group_varsmonitoring_servermainyml-appended-to-prometheus_extra_scrape_configs).
  - File: `~/dev/home-network-new/group_vars/monitoring_server/main.yml`.
  - Append it to the end of the `prometheus_extra_scrape_configs` block, with its
    comment: target `192.168.1.34:9742`, labels `{host: "Mnemosyne", app: "putio-sync"}`.
  - Additive edit only: run `git -C ~/dev/home-network-new diff group_vars/monitoring_server/main.yml`
    before and after, and confirm the only new hunk is this job.
- [X] T026 [P] [US2] Write `~/dev/home-network-new/apps/putio-sync.md`.
  - Use frontmatter in the style of `apps/pull-through-cache.md`:
    - `app: putio-sync`, a summary;
    - `hosts: [Mnemosyne]`;
    - `run_model: unraid-docker-template`, as a deviation;
    - `ports: [9742]`, `exposure: lan`;
    - `metrics: { path: /metrics, port: 9742 }`.
  - Omit `host_group` if the doc pages aren't schema-validated. Otherwise reuse whatever
    `pull-through-cache.md` does, and confirm with `make check`.
  - Sections:
    - what it is;
    - the container must publish 9742 (Unraid template steps);
    - verify (targets page, `curl http://192.168.1.34:9742/metrics | head`);
    - the query cookbook from contracts/metrics.md;
    - alerts and dashboard links;
    - a Divergences note: not Ansible-managed, no fleet manifest (R12).
- [X] T027 [P] [US2] Document metrics in `README.md`, in a new "Metrics" section:
  - the endpoint, defaults and the no-auth warning ("anything that can reach the port can
    read metrics");
  - the full family table (type, labels, meaning), copied from contracts/metrics.md;
  - the questions-to-queries table.
- [X] T028 [US2] Run `make -C ~/dev/home-network-new check`. Record the result in this
  file's Implementation Notes. If a failure is caused by the user's unrelated uncommitted
  work, note it and don't change their files.

**Checkpoint**: the scrape job and docs are ready for the operator to deploy.

---

## Phase 5: User Story 3 – Alerts for the failures we've seen (Priority: P2)

**Goal**: alerts fire for target down, no recent success, queue stuck, sustained retries,
low disk and DB errors.

**Independent Test**: `promtool check rules` and `promtool test rules` pass. Each alert
fires on its synthetic series and stays silent on the healthy one.

- [ ] T029 [P] [US3] Write `specs/003-telemetry-prometheus/alert-tests.yml`, a
  `promtool test rules` file. For each of the six alerts in
  [contracts/alerts.md](contracts/alerts.md), include:
  - one `input_series` that makes it fire, with `alert_rule_test` at a time past its `for`;
  - one healthy series that gives `exp_alerts: []`.

  `rule_files:` points to `/tmp/putio-sync-alerts.yml` (rendered in T031).
- [ ] T030 [US3] Add `- name: putio-sync` with the six rules to
  `~/dev/home-network-new/roles/monitoring_prometheus/templates/alerts.yml.j2`. Take the
  expressions, `for` and severity from contracts/alerts.md, including:
  - the `NoRecentSuccess` `or … unless` clause;
  - `sum by (instance)` for retries.

  Write annotations in that file's style, escaping braces as
  `{{ '{{' }} $labels.host {{ '}}' }}`. Additive edit only.

  `PutioSyncNoRecentSuccess` uses 1800 s, which is 4 × the default `--download-interval`
  of 300 s. To check the NAS value, list only the interval flag from the container's
  command line:

  ```bash
  ssh -o BatchMode=yes root@192.168.1.34 "docker inspect putio-sync --format '{{json .Config.Cmd}}' | grep -o 'download-interval[^,]*'"
  ```

  - If nothing is printed, the default is in use.
  - If the interval is longer than 450 s, set the threshold to 4 × that interval.
  - Record the value used in the rule's description, on `apps/putio-sync.md` and in
    Implementation Notes.
  - Never print the rest of the container config or env, because it holds the token.
- [ ] T031 [US3] Render and test the rules:
  1. Render only the new group to `/tmp/putio-sync-alerts.yml`, for example by extracting
     the `putio-sync` group with python + jinja2 (`{{ '{{' }}` → `{{`).
  2. Run `promtool check rules /tmp/putio-sync-alerts.yml`.
  3. Run `promtool test rules specs/003-telemetry-prometheus/alert-tests.yml`.

  Write the exact commands into quickstart.md §4, replacing the "illustrative" note. If
  `promtool` isn't installed, ask the user before running `brew install prometheus`.

**Checkpoint**: every alert is verified against synthetic data.

---

## Phase 6: User Story 4 – Verbosity is adjustable and visible (Priority: P2)

**Goal**:
- the log format and metrics settings can be overridden by env;
- the log level changes live within 30 s;
- `detailed` metrics work;
- the dead notify watcher is removed.

**Independent Test**: quickstart §3, steps 1–5.

### Tests for User Story 4 ⚠️ write first

- [ ] T032 [P] [US4] Add tests to `putio-sync-core/src/logging.rs`, under `ENV_LOCK`:
  1. `PUTIO_SYNC_LOG_FORMAT=json` overrides `console_format`, with source Env.
  2. An invalid format gives `invalid logging.console_format 'x': expected one of compact,
     pretty, json`.
  3. `apply_level` returns:
     - `Changed{old:"info",new:"debug"}` for a new level;
     - `Unchanged` for the same level;
     - `IgnoredEnv` when `RUST_LOG` is set;
     - `Invalid(_)` for `"loud"`, with the filter left as it was.
- [ ] T033 [P] [US4] Write `putio-sync-core/tests/config_reload.rs`:
  - Write a temp config with `level="info"` and start
    `spawn_reload_poller(path, Duration::from_millis(50), cb)` with a recording callback.
  - Rewrite the file with `level="debug"` and wait. The callback gets the new
    `LoggingConfig` within 500 ms.
  - Write invalid TOML. You get one ERROR and no callback.
  - Delete the file. You get one ERROR, and the poller is still running.
- [ ] T034 [P] [US4] Add a detail test to `putio-sync-telemetry/tests/contract.rs`. At
  `detailed`, with `init(workers=3)`:
  - `putio_sync_worker_state{worker="0"}` through `{worker="2"}` exist, and no
    `worker="3"`;
  - `putio_sync_api_operation_duration_seconds{operation="list_files"}` appears after one
    `record_attempt`.

### Implementation for User Story 4

- [ ] T035 [US4] Make the log filter reloadable in `putio-sync-core/src/logging.rs`:
  - Wrap the `EnvFilter` in `tracing_subscriber::reload::Layer` in every branch of
    `init_logging`, and store the `reload::Handle` in a `static OnceLock`.
  - Add `pub enum LevelChange { Unchanged, Changed{old,new}, IgnoredEnv, Invalid(String) }`.
  - Add `pub fn apply_level(&LoggingConfig) -> LevelChange`. It reuses `build_filter`; if
    `RUST_LOG` is set, it returns `IgnoredEnv`. It keeps the current directive string to
    compare against.
  - Log exactly the lines in [contracts/config.md](contracts/config.md#errors):
    - `info!(old, new, source="Config", "Log level changed")`;
    - `info!("Log level change in config ignored; RUST_LOG is set")`;
    - `error!(error, "Log level not changed")`.
- [ ] T036 [US4] Add the `PUTIO_SYNC_LOG_FORMAT` override to
  `putio-sync-core/src/logging.rs`:
  - It applies before the subscriber is built and accepts `compact`, `pretty`, `json`.
  - Extend the 002 startup line to
    `info!(level, source, format, format_source, "Log level")`.
- [ ] T037 [US4] Add the reload poller to `putio-sync-core/src/config.rs`:
  - `pub fn spawn_reload_poller(path: PathBuf, interval: Duration, on_change: impl Fn(LoggingConfig) + Send + 'static) -> JoinHandle<()>`.
  - It uses `tokio::time::interval` and compares `(modified, len)` from
    `tokio::fs::metadata`. On a change it calls `Config::load(&path)` and passes
    `config.logging` to the callback.
  - Load or metadata errors give `error!` once per distinct stamp, and it carries on.
  - Production uses an interval of 10 s (R8).
- [ ] T038 [US4] Start the poller in both modes, in `putio-sync-cli/src/commands/daemon.rs`
  and `putio-sync-cli/src/commands/server.rs`:
  - Only when a config file path is known.
  - The callback is `|logging| { let _ = putio_sync_core::logging::apply_level(&logging); }`.
- [ ] T039 [US4] Remove the dead notify watcher:
  - delete `putio-sync-daemon/src/config_watcher.rs`;
  - remove its `mod`/`pub use` in `putio-sync-daemon/src/lib.rs`;
  - remove the `config_watcher` field, init block and start block, and the
    `enable_hot_reload`/`config_path` fields if nothing else uses them, in
    `putio-sync-daemon/src/daemon.rs`;
  - remove `notify` from `putio-sync-daemon/Cargo.toml`;
  - fix callers in `putio-sync-cli/src/commands/daemon.rs`.

  Check with `grep -rn "enable_hot_reload\|ConfigWatcher\|notify" putio-sync-*/src putio-sync-*/Cargo.toml`
  that nothing is left.
- [ ] T040 [US4] Record per-worker metrics for detailed mode:
  - In `putio-sync-download/src/manager.rs` and `putio-sync-download/src/stream.rs`, pass
    the worker id to the download path. It is already in `WorkerStats.worker_id`.
  - Call `download::worker_busy(id, true/false)` around each job, and
    `download::worker_inflight(id, bytes_so_far)` with each chunk.
  - Both are no-ops at `basic` (T007).
- [ ] T041 [P] [US4] Document the settings in `config.example.toml` and `README.md`:
  - add the `[metrics]` block from contracts/config.md, with comments;
  - add a "Verbosity" table covering every env var, its allowed values, what's live versus
    what needs a restart, and the startup lines;
  - remove any README mention of hot reload that the removed watcher implied (FR-017).

**Checkpoint**: quickstart §3 passes by hand, and `make ci` is green.

---

## Phase 7: User Story 5 – A dashboard for a quick look (Priority: P3)

**Goal**: one Grafana dashboard answers "is it healthy, and what did it do today?"

**Independent Test**: the dashboard is provisioned by the Grafana role and `make check`
passes. After deploy, the panels load with no errors.

- [ ] T042 [US5] Write `~/dev/home-network-new/roles/monitoring_grafana/templates/putio-sync-dashboard.json.j2`:
  - Model it on `ollama-queue-dashboard.json.j2`: the same datasource reference, uid style
    and `{% raw %}`/escaping conventions as that file.
  - Panels:
    - target up;
    - age of the last successful sync (`time() - putio_sync_last_success_timestamp_seconds`);
    - sync outcomes rate;
    - `queue_items` by state;
    - download throughput (`rate(putio_sync_downloaded_bytes_total[5m])`) and completions;
    - API request, error and retry rates, and p95 latency;
    - watch uploads and cleanups;
    - blocked files;
    - disk free %.
- [ ] T043 [US5] Add a copy task for `putio-sync-dashboard.json.j2` to
  `~/dev/home-network-new/roles/monitoring_grafana/tasks/main.yml`, next to the
  `ollama-queue-dashboard.json.j2` task (around line 206), using the same module, dest
  pattern and notify. Then re-run `make -C ~/dev/home-network-new check` and validate the
  JSON:
  1. Render it with python jinja2 to `/tmp/putio-sync-dashboard.json`.
  2. Run `python3 -m json.tool /tmp/putio-sync-dashboard.json`.

---

## Phase 8: Polish & Cross-Cutting Concerns

- [ ] T044 [P] Check throughput overhead (SC-002). Add an `#[ignore]` benchmark-style test
  to `putio-sync-download/tests/metrics_sync.rs` that streams a 64 MiB synthetic file
  twice, once with telemetry initialised and once without. Run it once with
  `cargo test -- --ignored`. Record the throughput delta (≤2%) in Implementation Notes.
- [ ] T045 Run `make ci` from the repo root. Fix anything it reports. Record the baseline
  and final test counts in Implementation Notes (002 ended at 333).
- [ ] T046 Run quickstart §1–§5 and record each outcome. §6 is the operator's
  post-deploy step; list it as pending.
- [ ] T047 [P] Update `specs/003-telemetry-prometheus/checklists/requirements.md` notes.
  Two of them are out of date after clarify:
  - "metrics are unauthenticated by default" is now "always unauthenticated, no auth
    option";
  - the live-reload note should now say it was kept at P2, as clarified.
- [ ] T048 Add an "Implementation Notes" section at the end of this file covering:
  - baseline and final test counts;
  - deviations from plan or research;
  - the home-network-new files changed, all uncommitted;
  - the operator steps: publish port 9742 in the Unraid template, pull the new image, and
    deploy the monitoring stack.

---

## Dependencies & Execution Order

### Phase dependencies

- **Setup (T001–T003)**: no dependencies. T002 and T003 can run in parallel with each
  other once T001 exists, because T003 needs the crate path.
- **Foundational (T004–T011)**: needs Setup, and blocks every story.
  - T005 comes before T006, and T006 before T007–T009.
  - T004 and T010 are tests: write them early, and they go green after T006–T008.
  - T011 is independent of the telemetry tasks and only needs core.
- **US1 (T012–T024)**: needs Foundational.
- **US2 (T025–T028)**:
  - T025 and T026 need only the metric names from the contract, so they can start in
    parallel with US1.
  - T028's meaning depends on US1 being deployable.
- **US3 (T029–T031)**: needs only the contract, so it can run in parallel with US1 and US2.
  Deploying it is useful only after US1.
- **US4 (T032–T041)**:
  - Needs Foundational. T040 needs T018, because it edits the same download path.
  - T038 and T023 both edit `daemon.rs` and `server.rs`, so do them one after the other.
- **US5 (T042–T043)**: needs only the contract.
- **Polish (T044–T048)**: after the chosen stories are done.

### Same-file sequencing (not [P])

| File | Tasks, in order |
|---|---|
| `putio-sync-download/src/manager.rs` | T017 → T018 → T040 |
| `putio-sync-download/src/stream.rs` | T018 → T040 |
| `putio-sync-cli/src/commands/daemon.rs`, `server.rs` | T023 → T038 → T039 |
| `putio-sync-core/src/config.rs` | T011 → T037 |
| `putio-sync-core/src/logging.rs` | T032 → T035 → T036 |
| `~/dev/home-network-new/group_vars/monitoring_server/main.yml` | T025 only |
| `putio-sync-telemetry/tests/contract.rs` | T004 → T034 |

## Parallel Examples

```text
# Foundational, after T006:
T007 helpers   |  T008 server.rs  |  T009 gauges.rs  |  T011 core MetricsConfig

# US1 tests, all different crates:
T012 client/tests/metrics.rs  |  T013 download/tests/metrics_sync.rs  |  T014 file-manager tests  |  T015 db/tests/metrics.rs

# US1 implementation, different crates:
T016 client  |  T019 file-manager  |  T020+T021 db      (T017→T018 download stays sequential)

# Other repo, in parallel with US1:
T025 scrape job  |  T026 apps/putio-sync.md  |  T029 alert-tests.yml  |  T042 dashboard
```

## Implementation Strategy

### MVP (US1 only)

1. Setup and Foundational (T001–T011).
2. US1 (T012–T024).
3. **Stop and validate**: `make ci`, then quickstart §2. The daemon serves metrics locally.
   That alone replaces the SSH-and-grep sessions from feature 002.

### Incremental delivery

1. Add US2 (scrape job and app page). Once the operator deploys and publishes port 9742,
   history starts collecting.
2. Add US3 (alerts). Silent failures become alerts.
3. Add US4 (live verbosity, detailed metrics, removing the dead watcher).
4. Add US5 (dashboard).
5. Polish (T044–T048).

Each step leaves `make ci` green and can ship on its own.

## Implementation Notes

MVP (T001–T024), recorded during implementation. T048 extends this section.

- `make ci` passes with 364 tests (002 ended at 333).
- Contract invariant 4 now checks label values for `://` instead of `http`, because
  `reason="http"` is a valid download-failure label.
- `tracked(op, context)` in `putio-sync-db/src/error.rs` takes a context string as well as
  the `DbOp`, so every DB error message keeps its old wording.
- `Resolved<T>` has a single `source`: `env` if any `PUTIO_SYNC_METRICS_*` override applied,
  otherwise `config`.
- The `metrics.bind` error says `expected one of ip:port`.
- `putio-sync-cli/src/telemetry.rs` is declared in `lib.rs`, not `main.rs`, because the
  `daemon` and `server` commands live in the library and the e2e test imports it.
- `process_queue` now builds download URLs from the client's base URL instead of a
  hard-coded `https://api.put.io`. Production behaviour is unchanged; it makes the path
  testable.
- Server mode opens `.putiosync.db` relative to the working directory, while the daemon
  uses `download_dir/.putiosync.db`, so the two modes can report different queue
  contents (out of scope, T023).
- The e2e test seeds its database on real time and then pauses the clock, because sqlx's
  pool timeouts fire under a paused clock.
- Tests that share the global registry serialise on a `tokio::sync::Mutex` (clippy rejects
  a std guard held across `.await`).

US2 (T025–T028):

- `make -C ~/dev/home-network-new check` passes ("All offline checks passed"), and the
  rendered scrape configs parse with 13 jobs, `putio_sync` last. The check prints an
  `action` string warning and a `49agents.py` usage error; both predate this change.
- The scrape job follows `main.yml`'s block style (multi-line `targets`/`labels`)
  rather than the inline form in contracts/alerts.md. Same values.
- The `main.yml` diff was compared before and after: the only new hunk is the
  `putio_sync` job. The user's other uncommitted edits in home-network-new are untouched,
  and nothing there is committed.
- `apps/putio-sync.md` keeps `host_group: nas_devices` (the real inventory group for
  Mnemosyne; app pages aren't schema-validated, only fleet manifests are).
- README "Metrics" includes a short `[metrics]` settings block; T041 still owns the
  full settings docs and `config.example.toml`.
