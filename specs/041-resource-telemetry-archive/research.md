# Research: CPU and Memory Telemetry, Kept Long Term

Facts below were checked on 2026-10-27 and 2026-10-28, read-only, on the NAS (Mnemosyne), the
monitoring server (SM4M-64-Redis) and `~/dev/home-network-new`.

## What exists

- **putio-sync**: `putio-sync-telemetry` owns one `prometheus` 0.14 registry
  (`default-features = false`), served by axum on `0.0.0.0:9742/metrics`. Gauges that need
  fresh values (queue, disk) are set by a 15 s refresh task (`gauges.rs`). No process figures
  are exposed. `putio_sync_start_time_seconds` and `putio_sync_build_info` exist.
- **Sync state**: `DownloadManager::sync_downloads` (`putio-sync-download/src/manager.rs`)
  already calls `sync_status.begin()` and `finish()` around each run, then
  `telemetry::sync::record_run`. That is where "sync running" can be set.
- **Monitoring server**: Prometheus (Homebrew, LaunchDaemon or LaunchAgent from role
  `monitoring_prometheus`) on `:9090`, scrape interval 15 s, `--storage.tsdb.retention.time=30d`
  for everything. About 11,400 series, 3.5 GB on disk, 618 GB free. It scrapes itself
  (`job_name: prometheus`). `promtool` and `jq` are installed. Ports used in the repo include
  9090, 9091, 9093, 9100, 9115, 9121, 9187, 9190, 9191 and 9742.
- **Alerts**: a `DiskSpaceLow` rule (below 20% free) already covers `/` on every node_exporter
  host, so the monitoring server's disk is covered. FR-012 needs no new rule.
- **NAS**: no Prometheus, Grafana or container exporter. The container runs
  `putio-sync --config /config/config.toml daemon ...`. `/config` is
  `/mnt/user/appdata/putio-sync/`, and `/downloads` holds the DB (4.6 MB). The host has `curl`,
  `jq`, `sqlite3` and `gzip`, but not `promtool`. The Unraid User Scripts plugin is not
  installed.

## R1. How to read process figures

- **Decision**: a small collector in `putio-sync-telemetry` (`process.rs`) that reads Linux
  `/proc/self` at scrape time:
  - `stat`: user and system CPU ticks;
  - `status`: `VmRSS`, `VmSize`, `Threads`;
  - `io`: `read_bytes`, `write_bytes`;
  - the `fd/` directory: entries counted;
  - `limits`: the soft "Max open files".

  CPU ticks are turned into seconds with `rustix::param::clock_ticks_per_second()`, which needs
  the `param` feature on the existing `rustix` dependency. On other platforms the collector
  registers nothing and logs one warning (FR-005).
- **Rationale**: no new crate is needed. It covers every FR-001 figure, including disk bytes.
  It keeps the `putio_sync_` prefix the constitution asks for. The parsers are pure functions
  over strings, so they are tested with fixture text on any OS.
- **Alternatives considered**:
  - `prometheus` `process` feature (`ProcessCollector`): it pulls in `procfs` and its
    dependencies, has no disk I/O figures, and has no way to warn about missing figures.
    Rejected.
  - `sysinfo` 0.31, already used by `putio-sync-cli` diagnostics: it reports CPU only as a
    percentage between two refreshes (no running counter), and has no open-file count or limit
    on 0.31. Rejected.
  - macOS figures through `getrusage`: these are only seen in development. Rejected under
    Principle VII; FR-005 allows them to be missing.

## R2. When the figures are read

- **Decision**: implement `prometheus::core::Collector` for the process collector and register
  it in the existing registry. `collect()` reads `/proc` and returns fresh families, so every
  scrape gets current values (FR-003). Counters are emitted as counter-type families with the
  read value, not by `inc_by` on a stored counter. A file that fails to read leaves that family
  out of this scrape only.
- **Rationale**: reading `/proc/self` takes well under 1 ms, so it cannot push a scrape past
  1 s. It adds nothing to the download path (SC-003). The 15 s refresh task in `gauges.rs` would
  make values up to 15 s stale and is not needed here.

## R3. "Sync running" figure

- **Decision**: a new gauge `putio_sync_sync_in_progress` (0 or 1). It is set to 1 next to
  `sync_status.begin()` and back to 0 in `record_run`, which every run reaches, success or
  failure.
- **Rationale**: the activity state needs to tell syncing from idle (spec FR-001), and no
  existing figure says a sync is running now.

## R4. Where the thinned year lives

- **Decision**: a second, small Prometheus on the monitoring server ("long-term instance"):
  - same Homebrew binary, own data dir, port `9092`;
  - `--storage.tsdb.retention.time=1y`, `--web.enable-remote-write-receiver`;
  - no scrape jobs.

  The main Prometheus works out the 5-minute points with recording rules (group interval 5m),
  and `remote_write`s only those series (`write_relabel_configs` keeping
  `putio_sync:.+:5m`) to the long-term instance. Main Prometheus also gets a `remote_read` of the
  long-term instance with `read_recent: false`.
- **Rationale**:
  - **Spec fit**: it keeps the 30-day retention for everything else, as clarified. It adds no
    new software, only a second copy of a program the repo already installs.
  - **Seamless Grafana**: through `remote_read`, a query to the main Prometheus for a
    `putio_sync:*:5m` series returns the local 30 days and the older remote data as one
    series. Grafana keeps one data source and the dashboard needs no switch (FR-010a).
  - **Survives outages**: `remote_write` retries from the main WAL (about 2 h) while the
    long-term instance is down.
  - **Size**: about 25 series at one point per 5 minutes is about 2.6 M samples a year, a few
    MB on disk (SC-002b).
- **Alternatives considered**:
  - **Federation** (the long-term instance scrapes `/federate` every 5 min): pull at a
    5-minute interval sits on Prometheus's 5-minute staleness edge, and an outage loses points
    with no retry. Rejected.
  - **VictoriaMetrics**: it would be new software to install, run and back up. Automatic
    downsampling is an enterprise feature, so recording rules would be needed anyway. Rejected.
  - **Raising retention for everything**: rejected by the user (clarification Q1).

## R5. Showing raw detail for recent ranges and the thinned copy for older ones

- **Decision**: long-range panels use `raw_expr or on(instance) thinned_expr`. Vector matching
  ignores the metric name, so where a raw sample exists (the last 30 days) it wins, and older
  points come from the thinned series. Thinned series are wrapped in
  `last_over_time(...[10m])`, so 5-minute samples never fall between query steps.
- **Rationale**: this meets FR-010a with one data source and no variables. It is checked in
  quickstart step 6, since it relies on label sets matching exactly. The recording rules keep
  `job`, `instance`, `host` and `app`.

## R6. Activity state

- **Decision**: one recorded series, `putio_sync:activity_state:5m`, whose value is the state:
  - `N ≥ 1`: downloading, when `round(avg_over_time(downloads_active[5m])) ≥ 1`, that is, at
    least half the window had downloads running. `N` is the rounded average.
  - `-1`: syncing only, when not downloading and `avg_over_time(sync_in_progress[5m]) ≥ 0.5`.
  - `0`: idle, otherwise.

  Per-state CPU and memory are worked out later (dashboard and summary) by pairing each
  5-minute point with that point's state.
- **Rationale**: "mostly spent in" follows the spec wording. Downloading wins over syncing,
  because downloads are what cost resources. Putting the state in the value instead of a label
  keeps label sets fixed (Principle V). N is the number of downloads actually running, so a
  change in worker count doesn't shift its meaning (spec edge case).

## R7. Archive and summary: scripts or subcommands

- **Decision**:
  - **Engine**: a new crate `putio-sync-archive`, reached through a CLI subcommand
    `putio-sync archive {metrics,db,summary}`.
  - **Wrappers**: thin shell scripts in `scripts/archive/` for scheduling.
  - **On the NAS**: an Unraid User Scripts entry runs
    `docker exec putio-sync putio-sync --config /config/config.toml archive ...` daily. The
    binary is already in the container, so nothing is installed there except the User Scripts
    plugin.
- **Rationale**:
  - **Testing**: the summary needs date bucketing, joins with download history and
    restart-safe totals. In Rust that is tested under `make ci` with mockito and a temp SQLite
    (Principles II and III).
  - **Reuse**: it reuses the `putio-sync-db` history queries from feature 006 instead of
    writing the SQL again.
  - **Consistency**: it gets a consistent DB copy through `VACUUM INTO` on the same driver.
  - **No host tools**: nothing depends on host tools such as `jq` or `sqlite3`, whose
    versions differ between the NAS and macOS.
- **Alternatives considered**:
  - Pure shell with `curl`, `jq` and `sqlite3`: these work on the NAS today, but the summary
    logic would be untestable in `make ci`. Rejected.
  - Scheduling inside the daemon (an `[archive]` section): it adds config knobs and a scheduler
    for a once-a-day job that cron does well. Rejected under Principle VII; it could come later.

## R8. Archive formats

- **Decision**:
  - **Metrics**: one file per UTC day, `metrics/putio-sync-metrics-YYYY-MM-DD.om.gz`, in
    OpenMetrics text with `# EOF`. Restore is
    `promtool tsdb create-blocks-from openmetrics <file> <out>`, then the blocks are moved into
    the long-term data dir (overlapping blocks are allowed by default since Prometheus 2.39).
  - **Database**: `db/putio-sync-db-YYYY-MM-DD.sqlite.gz`, a gzipped `VACUUM INTO` copy.
  - **Summary**: `summary/usage-daily.csv`, `summary/usage-monthly.csv` and
    `summary/usage-latest-month.txt`, rewritten whole on every run.
  - **Manifest**: `manifest.jsonl` in the archive dir records each saved file (day, saved_at,
    points, partial).
- **Rationale**:
  - OpenMetrics is plain text that promtool loads straight back (FR-014), and it can be read
    without putio-sync.
  - `VACUUM INTO` gives a consistent snapshot while the daemon writes, and never changes the
    source.
  - CSV opens in a spreadsheet (spec US4).
- **Compression**: new dependency `flate2` (default `miniz_oxide` backend, pure Rust). Nothing
  in the workspace does gzip today. Justified under Principle VII.

## R9. Fetching exact samples

- **Decision**: archive and summary fetch with an instant query of a range selector at the end
  of the period, for example `{__name__=~"putio_sync:.+:5m",instance="..."}[1d]` at
  `YYYY-MM-DDT24:00Z`, against the main Prometheus. Through `remote_read`, that covers a full
  year. It returns stored samples with their own timestamps.
- **Rationale**: `query_range` re-samples on its own step grid with lookback, which can drop or
  repeat 5-minute samples. A range selector returns exactly what is stored.

## R10. Filling gaps (spec US3 scenario 5)

- **Decision**: a documented step on the monitoring server:
  `promtool tsdb create-blocks-from rules --start --end --url http://127.0.0.1:9090 putio-sync-recording.yml`.
  It evaluates the same recording rules over the raw data still in the main Prometheus, and the
  output blocks are moved into the long-term data dir.
- **Rationale**: this is what promtool's rules backfill exists for, so no custom code is
  needed. Only points still in the 30-day raw data can be filled. Older gaps stay gaps and show
  as partial days in the manifest.

## R11. Dashboard source of truth

- **Decision**: the source is `packaging/grafana/putio-sync-resources.json` in this repo,
  importable anywhere through a `${DS_PROMETHEUS}` input. `~/dev/home-network-new` gets
  `roles/monitoring_grafana/templates/putio-sync-resources-dashboard.json.j2`, a copy with the
  data source set to the provisioned uid. The two are compared with
  `scripts/check-grafana-dashboard.sh` (jq diff ignoring data source fields), run by hand
  before deploy.
- **Rationale**: FR-009 asks for one named source, and the generic copy belongs to the
  project. The home-network convention is a Jinja template per dashboard.

## R12. Alerts

- **Decision**: add these rules to the existing `putio-sync` group in `alerts.yml.j2`, with
  promtool tests in `specs/041-resource-telemetry-archive/alert-tests.yml`, in the same style
  as feature 003's:
  - `PutioSyncMemoryHigh`: resident memory above 1 GiB for 30 min.
  - `PutioSyncMemoryGrowing`: `predict_linear(resident[24h], 7d) > 1 GiB`, with no restart in
    the last 24 h, for 1 h.
  - `PutioSyncCpuHigh`: `rate(cpu[5m]) > 0.9` for 30 min.
  - `PutioSyncOpenFilesHigh`: open files above 80% of the limit for 10 min.
  - `PutioSyncLongTermBehind`: remote write to the long-term instance is more than 2 h behind
    (`prometheus_remote_storage_queue_highest_sent_timestamp_seconds`).
  - `PutioSyncLongTermDown`: the long-term instance's `up` is 0 for 2 h (FR-011a, with the
    one above).
- **Rationale**: these are the US5 thresholds. Remote-write metrics show a stalled copy even
  though `remote_read` would otherwise hide it behind the local 30 days.
