# Research: Telemetry and Prometheus Monitoring

Decisions for feature 003. Each one lists what was chosen, why, and what else was considered.

## R1. Metrics library

- **Decision**: `prometheus` crate 0.14 with `default-features = false` (no protobuf).
  - Metrics are registered in one `Registry` owned by a new `putio-sync-telemetry` crate.
  - Output uses `TextEncoder`.
- **Rationale**:
  - It is mature, widely used, and has typed counters, gauges and histograms with labels.
  - The text encoder is exactly the exposition format FR-005 asks for.
  - Without protobuf it pulls in only a few small crates (`fnv`, `parking_lot`, `memchr`,
    `thiserror`, `lazy_static`).
  - An explicit registry means tests can build a fresh one and assert on output, with no
    global state leaking between tests.
- **Alternatives considered**:
  - `metrics` + `metrics-exporter-prometheus`:
    - The facade is nice, and it is a no-op without a recorder.
    - But the exporter brings its own hyper server and upkeep thread.
    - Histograms default to summaries unless configured.
    - Asserting in tests needs a test recorder.
    - More moving parts for one daemon.
  - `prometheus-client` (OpenMetrics): a good API, but its OpenMetrics output (`# EOF`, `_created`
    series) is a different contract from the classic text format. Prometheus accepts both,
    but alert and test tooling is simpler with the classic one.
  - Hand-writing the text format: less code to pull in, but histogram buckets and escaping
    rules would have to be written and tested by hand. That conflicts with Principle VII,
    "smallest change", in the other direction.

## R2. Where metrics live (crate boundaries)

- **Decision**: add a new bottom-layer crate, `putio-sync-telemetry`. It depends only on
  `prometheus`, `tokio`, `axum` and `tracing`. It owns:
  - every metric family (names, help text, label enums) as typed statics behind accessors;
  - the `detailed` flag;
  - the HTTP listener (`serve(settings)`).

  The engine crates (`client`, `db`, `download`, `file-manager`) depend on it and call
  small helpers, for example `telemetry::api::record(op, outcome, elapsed)`.
- **Rationale**:
  - Put.io calls happen in `client`, the lowest crate; `core` depends on `client`. The
    metric definitions therefore can't live in `core`, `daemon` or `api` without an upward
    dependency (Principle I).
  - One crate holds the whole metric contract, so the documentation and the bounded-label
    enums sit in one place, and FR-003 can be tested in one test.
  - Instrumenting the engine crates rather than the daemon loop means daemon mode and
    server mode report the same metrics automatically, with no duplicated code (FR-007,
    clarification Q5).
- **Alternatives considered**:
  - Put metrics in `core`: rejected, `client` can't depend on `core`.
  - Use the facade macros in each crate: rejected with R1.
  - Pass a metrics handle through every constructor: rejected, because it touches every
    public API for no gain over a crate-level registry.

## R3. HTTP listener

- **Decision**: `telemetry::serve(&MetricsSettings)` binds a `tokio::net::TcpListener` and
  serves one axum `Router` with a single GET route (the configured path, default
  `/metrics`). It runs in its own tokio task.
  - A bind failure returns an error. The caller logs it once at ERROR and carries on
    (FR-008).
  - Any other path returns 404.
  - The handler encodes from the registry with no locks held across `.await`. Reading
    metrics is lock-free or short-lock, so the response is never delayed by downloads
    (SC-002).
- **Rationale**: axum 0.7 is already in the workspace lock file (used by `putio-sync-api`),
  so this adds no new crate to the build. The same function serves both modes
  (clarification Q5).
- **Alternatives considered**:
  - Raw hyper 1: more boilerplate for the same result.
  - Mounting on the API router: rejected by clarification Q5.

## R4. Port and default bind

- **Decision**: the default bind is `0.0.0.0:9742` (clarification Q4) and the default path
  is `/metrics`.
- **Rationale**:
  - Port 9742 isn't used anywhere in `~/dev/home-network`.
  - It isn't listening on nas01 today. The ports in use there are 22, 80, 111, 139,
    443, 445, 2049, 3702, 5355, 7878, 8080, 8181, 8191, 8989, 9191, 9696 and 32400+.
  - It isn't a well-known Prometheus exporter port that could clash later. 9464, for
    example, is the OpenTelemetry default.
- **Alternatives considered**: 9464 (the OTel exporter default, kept free for a future
  OTel collector); 9100–9199 (crowded with node and other exporters).

## R5. Disk-space gauges

- **Decision**: use `rustix::fs::statvfs` on the download directory. `rustix` 1.1 is
  already in the lock file; enable its `fs` feature.
  - free = `f_bavail * f_frsize`;
  - total = `f_blocks * f_frsize`.
  - The values are refreshed on the gauge refresh tick (R7).
  - On error, the series are removed and one warning is logged; the next success clears
    the warned state (spec edge case).
- **Rationale**: it is safe Rust, already compiled, and accurate for bind mounts inside the
  container (it measures the filesystem behind the path, not the container root).
- **Alternatives considered**:
  - `sysinfo::Disks`: you have to match the path to the longest mount prefix, which isn't
    reliable for Unraid's `shfs` bind mounts.
  - `libc::statvfs`: needs `unsafe`.
  - `fs4`: a new crate for one call.

## R6. Process and build info

- **Decision**:
  - `putio_sync_start_time_seconds`: a gauge set once at startup, in Unix seconds.
  - `putio_sync_build_info{version,commit}`: a gauge that is always 1.
    - `version` = `CARGO_PKG_VERSION`.
    - `commit` = `option_env!("PUTIO_SYNC_GIT_SHA")`, or `"unknown"` when it isn't set.
  - The Dockerfile passes `PUTIO_SYNC_GIT_SHA` as a build arg.
- **Rationale**: these follow standard Prometheus conventions without the `process`
  feature. That feature needs `procfs`, works only on Linux, and adds a dependency for
  CPU and memory numbers the spec doesn't ask for.
- **Alternatives considered**: the `prometheus` `process` feature, deferred until CPU and
  memory are actually needed.

## R7. Queue-depth gauges and "within one sync interval"

- **Decision**: the telemetry listener task also runs a refresh tick every 15 s. Each tick:
  - calls an existing-style DB aggregate (`SELECT status, COUNT(*) … GROUP BY status`);
  - sets `putio_sync_queue_items{state}` for the states pending, downloading, paused and
    failed;
  - refreshes the disk gauges.

  The tick also runs once at startup, before the first scrape can happen (FR-004).

  The tick is given a closure, so `telemetry` stays below `db`. The CLI wires
  `db.queue().counts()` into it.
- **Rationale**: the gauges are always fresh (≤15 s, well under the 300 s sync interval),
  are correct straight after a restart, and need no bookkeeping on every status change.
- **Alternatives considered**:
  - Increment and decrement on each status change: easy to drift after a crash or restart.
  - Query on every scrape: an async DB call inside the handler could make the 1 s budget
    depend on SQLite lock waits.

## R8. Live log-level reload (clarification Q1)

- **Decision**:
  1. `core::logging::init_logging` wraps its `EnvFilter` in
     `tracing_subscriber::reload::Layer` and stores the `reload::Handle` in a `OnceLock`.
  2. A new `core::logging::apply_level(&LoggingConfig) -> LevelChange` rebuilds the filter
     using the same `build_filter` precedence and swaps it in. It returns `Unchanged`,
     `Changed{old,new}`, `IgnoredEnv` or `Invalid(err)`.
  3. A new `core::config::spawn_reload_poller(path, interval=10s, on_change)` polls the
     config file's mtime and length with `tokio::time::interval`. On a change it re-parses
     the file and calls `apply_level`.
     - A parse error is logged once at ERROR and keeps the current level.
     - The poll runs every 10 s, so a change takes effect within 30 s (FR-010, SC-005).
  4. The CLI starts the poller in both `daemon` and `server` modes when a config path is
     known.
  5. Remove the unused notify-based `putio-sync-daemon::ConfigWatcher`, its
     `enable_hot_reload` plumbing and the `notify` dependency (Principle VII, dead code).
- **Rationale**:
  - The existing watcher never applied anything: its callback only logs "reload would be
    triggered here".
  - It blocks a tokio worker thread on a std `mpsc::recv`.
  - inotify events on Unraid's FUSE `shfs` user shares aren't reliable for edits made from
    the host or over SMB.
  - Polling the mtime every 10 s is simple, portable, testable with a temp file, and costs
    nothing.
- **Alternatives considered**:
  - Keep `notify` and fix the callback: there are still reliability problems on `shfs`,
    plus the blocking thread.
  - SIGHUP: rejected by clarification Q1 (no signal or command).
  - An admin HTTP endpoint: clarification Q1 option C, not chosen.

## R9. Settings and environment overrides

- **Decision**:
  - A new `[metrics]` table in the TOML config holds `enabled` (true), `bind`
    ("0.0.0.0:9742"), `path` ("/metrics") and `detail` ("basic").
  - The existing `[logging]` table keeps `level`, `module_filters` and `console_format`.
  - Environment overrides, applied after the file loads:

    | Setting | Env var | Notes |
    |---|---|---|
    | log level / directives | `RUST_LOG` | already the override (feature 002); kept for compatibility |
    | log format | `PUTIO_SYNC_LOG_FORMAT` | `compact`, `pretty`, `json` |
    | metrics enabled | `PUTIO_SYNC_METRICS_ENABLED` | `true`/`false`/`1`/`0` |
    | metrics bind | `PUTIO_SYNC_METRICS_BIND` | `host:port` |
    | metrics path | `PUTIO_SYNC_METRICS_PATH` | must start with `/` |
    | metrics detail | `PUTIO_SYNC_METRICS_DETAIL` | `basic`, `detailed` |

  - For each setting, the effective value and its source (`Config`/`Env`) are logged in one
    startup line, extending the 002 "Log level" line.
  - Invalid values fail startup with a message naming the setting and its allowed values
    (FR-011).
- **Rationale**:
  - The spec says "`text` or `json`". The existing config already has `compact`, `pretty`
    and `json`, and both text styles are worth keeping. The spec's FR-009 wording was
    updated to match rather than adding a new enum.
  - `RUST_LOG` stays the level override so existing NAS and dev setups keep working.
- **Alternatives considered**:
  - A generic `PUTIO_SYNC__SECTION__KEY` overlay (figment-style): more general than needed,
    and it adds a dependency.
  - `PUTIO_SYNC_LOG_LEVEL` alongside `RUST_LOG`: two overrides for one setting would be
    confusing.

## R10. Metric detail levels (clarification Q3)

- **Decision**:
  - `telemetry::set_detail(Detail)` is set once at startup and read by the helpers through
    an `AtomicBool`.
  - At `basic`, the helpers observe only `putio_sync_api_request_duration_seconds` (no
    `operation` label).
  - At `detailed`, they also observe
    `putio_sync_api_operation_duration_seconds{operation}` and update
    `putio_sync_worker_state{worker}` and `putio_sync_worker_inflight_bytes{worker}`.
  - Worker labels are `"0"..workers-1`. They are pre-registered at startup, so the series
    count is fixed.
- **Rationale**: one switch, decided at startup, with a fixed number of series.
  Live-switching detail isn't required (only the log level is live, per FR-010).

## R11. Instrumentation points

| Metric area | Where | Hook |
|---|---|---|
| API requests, outcome, latency, retries | `putio-sync-client::client::execute_with_retry` | every attempt records the outcome; every retry records its reason (network / server_error / rate_limited) |
| API operation label | each public client method passes a static `ApiOp` into `execute_with_retry` | closed enum |
| Sync runs, durations, per-sync counts | `putio-sync-download::DownloadManager::sync_downloads` | wraps the whole run; uses `SyncStats` |
| Downloads completed/failed, bytes, duration, active | the download worker in `putio-sync-download` | bytes are added per chunk written (so throughput = `rate()`); a guard type decrements the active count on drop |
| Blocked files | the same place `files_blocked` is incremented (feature 002) | counter |
| Watch scans/uploads/cleanups/invalid/failures | `putio-sync-file-manager::watcher` | maps `ProcessOutcome` and cleanup errors to counters |
| DB errors | `putio-sync-db`, where `DbError` is built from an sqlx error | `operation` = a closed enum of queue/history/upload calls |
| Queue depth, disk | telemetry refresh tick (R7) | gauges |

The download "outcome" labels for failures are a closed set: `network`, `http`, `io`,
`integrity`, `other`. They are mapped from `DownloadError` variants.

## R12. Home-network integration

- **Decision**: in `~/dev/home-network`, change these four places by hand:
  1. Add a `putio_sync` job to `prometheus_extra_scrape_configs` in
     `group_vars/monitoring_server/main.yml`, with target `192.168.1.50:9742` and labels
     `{host: "nas01", app: "putio-sync"}`, following the `registry_cache` and
     `atranslate` jobs.
  2. Add a `putio-sync` group to `roles/monitoring_prometheus/templates/alerts.yml.j2`,
     with the six rules from US3.
  3. Add `roles/monitoring_grafana/templates/putio-sync-dashboard.json.j2`, plus its copy
     task next to `ollama-queue-dashboard` in `roles/monitoring_grafana/tasks/main.yml`.
  4. Add `apps/putio-sync.md` as a doc page, using frontmatter in the style of
     `apps/pull-through-cache.md`.

  The `fleet/` app manifest isn't added.
- **Rationale**:
  - `fleet.py validate` manifests are for apps provisioned by an Ansible role with a repo
    pin and a `host_group` in the inventory.
  - putio-sync runs from an Unraid Docker template the operator manages, like the
    pull-through cache's compose project. That one is also documented only as an
    `apps/*.md` page.
  - Keeping all four edits in existing files means `make check` covers them.
- **Constraints**:
  - That repo has unrelated uncommitted changes, including the same
    `group_vars/monitoring_server/main.yml` (the registry_cache job). Edits must be
    additive, without touching or committing the other changes.
  - Nothing is committed there without the user's go-ahead.
- **Alert test**: the spec asks for synthetic tests (SC-004), and the repo has no
  `promtool` tests today. Rules are checked with `promtool check rules` and a
  `promtool test rules` file, both run against a temp render of the template. They are run
  manually or from the putio-sync quickstart, not added to that repo's `make check`. That
  avoids making `promtool` a new hard requirement there.
