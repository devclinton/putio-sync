---

description: "Task list for feature 041: CPU and Memory Telemetry, Kept Long Term"
---

# Tasks: CPU and Memory Telemetry, Kept Long Term

**Input**: Design documents from `specs/041-resource-telemetry-archive/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: included (Constitution II and III). Write each test first and watch it fail.

**Two repos**: tasks marked **(HN)** are in `~/dev/home-network-new`. Do not deploy and do not
touch the NAS. Deploying is the operator's step in quickstart §4 and §6.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [ ] T001 *(Deferred to US4: nothing before T021 uses the crate, and an empty crate would add `flate2`
  unused.)* Create crate `putio-sync-archive`:
  - `Cargo.toml` deps: putio-sync-core, putio-sync-db, reqwest, serde, serde_json, chrono,
    tokio, tracing, thiserror, uuid, `flate2 = "1"` (default backend); dev deps: mockito,
    tempfile;
  - stub `src/lib.rs` and `src/error.rs` (`ArchiveError` with one variant per exit code in
    contracts/archive-cli.md: Args=1, Prometheus=2, Database=3, Folder=4);
  - add it to workspace `members` in `Cargo.toml`, and to the Dockerfile so
    `scripts/check-dockerfile-members.sh` passes.
- [X] T002 [P] Turn on the `param` feature for `rustix` in `putio-sync-telemetry/Cargo.toml`.
  It is already locked, so `Cargo.lock` changes only in features.

## Phase 2: Foundational (blocks every story)

- [X] T003 Test, then add the `putio_sync_sync_in_progress` gauge in
  `putio-sync-telemetry/src/lib.rs` (register it in `register_all`) and `src/sync.rs`:
  - add `pub fn started()`, which sets the gauge to 1;
  - `record_run` sets it to 0 on every result;
  - the gauge is 0 at startup.

  Tests cover invariant 4 of contracts/metrics.md: 1 during a run, 0 after success, 0 after
  failure.
- [X] T004 Call `putio_sync_telemetry::sync::started()` beside
  `self.sync_status.write().await.begin()` in `putio-sync-download/src/manager.rs` (around
  line 1143). Add a test in that crate's existing sync tests: the gauge reads 1 while a mocked
  sync is held open.

**Checkpoint**: the activity data that US2–US4 rely on exists.

## Phase 3: User Story 1 — putio-sync reports its own CPU and memory use (P1) 🎯 MVP

**Goal**: 8 `putio_sync_process_*` families on `/metrics` at `basic` and `detailed`, in daemon
and server mode.

**Independent test**: quickstart §2.

- [X] T005 [P] [US1] Add fixture files under `putio-sync-telemetry/tests/fixtures/proc/`:
  - `stat`, `status`, `limits` (one with a number, one with `unlimited`), `io`, and malformed
    versions of each;
  - shaped like the real NAS reads in research R1.
- [X] T006 [US1] Test, then write pure parsers in `putio-sync-telemetry/src/process.rs`. Each
  takes `&str` and returns `Option<…>`, never panics, and returns `None` on malformed text:
  - `parse_stat_cpu_ticks`: fields 14+15, reading fields from after the last `)`;
  - `parse_status`: `VmRSS` and `VmSize` in kB × 1024, and `Threads`;
  - `parse_limits_nofile`: the soft value, or `None` for `unlimited`;
  - `parse_io`: `read_bytes` and `write_bytes`.
- [X] T007 [US1] Test, then add `ProcessCollector`, which implements
  `prometheus::core::Collector`, in `putio-sync-telemetry/src/process.rs`:
  - it reads `/proc/self/*` on each `collect()` and caches nothing;
  - CPU = ticks ÷ `rustix::param::clock_ticks_per_second()`;
  - open files = entries in `/proc/self/fd`;
  - names, types and units exactly as the table in contracts/metrics.md, with no labels;
  - a figure that fails on one scrape is left out of that scrape only.
- [X] T008 [US1] *(The warning test is in its own binary,
  `putio-sync-telemetry/tests/process_warning.rs`, because tracing caches callsite interest
  across threads.)* Handle missing figures in `putio-sync-telemetry/src/process.rs`:
  - with `#[cfg(not(target_os = "linux"))]`, the collector yields no families;
  - log one WARN on the first collect, in this form:
    `Process metrics unavailable missing="cpu,open_fds,..." os=<os>`;
  - log nothing when every figure works;
  - guard it with `std::sync::Once`.
- [X] T009 [US1] Register `ProcessCollector` in `register_all` in
  `putio-sync-telemetry/src/lib.rs` at both levels, then add scrape tests in
  `putio-sync-telemetry/tests/`:
  - on Linux, all 8 families are present; CPU, memory and threads are above 0; CPU goes up
    across a busy loop (invariant 2);
  - elsewhere, no families and one WARN across 3 scrapes (invariant 3);
  - no new family has labels, and the existing redaction test still passes (invariant 5).
- [X] T010 [US1] *(Already covered: `scrape_is_fast_while_recording` in
  `putio-sync-telemetry/tests/server.rs` builds `Telemetry::new()`, which now registers the
  collector. Daemon and server mode both use `global()`.)* Extend the existing scrape-latency test from feature 003 to register the
  collector and stay under 1 s during a synthetic download (invariant 6). Check daemon and
  server mode share one registry (FR-006).
- [X] T011 [P] [US1] In the `README.md` metrics section, list the 9 new families with type and
  meaning (FR-021), and say they are Linux only.

**Checkpoint**: MVP. A Docker build scraped by hand shows the figures.

## Phase 4: User Story 3 — A thinned year of putio-sync history (P1)

Done before US2 because the dashboard reads these series.

**Goal**: 5-minute points stored for 1 year, readable from the main Prometheus.

**Independent test**: quickstart §3 and §4.

- [ ] T012 [US3] (HN) Write `roles/monitoring_prometheus/templates/putio-sync-recording.yml.j2`
  with group `putio-sync-5m`, `interval: 5m`, and every rule in
  contracts/long-term.md#recording-rules in that order. Fill in the rules shown there as "and
  :max5m". The activity-state rule must be exactly the 3-part `or` expression in the contract.
- [ ] T013 [US3] (HN) Add the long-term instance:
  - `defaults/main.yml`: `prometheus_longterm_listen: 127.0.0.1:9092`,
    `prometheus_longterm_retention_time: "1y"`, and a data dir of
    `{{ prometheus_data_dir }}-longterm`;
  - `tasks/main.yml`: data dir, config file (`global:` only), and a second
    LaunchDaemon/LaunchAgent copied from the main one with the flags in the contract table.
- [ ] T014 [US3] (HN) In `roles/monitoring_prometheus/templates/prometheus.yml.j2` add:
  - `rule_files` entry;
  - `remote_write` with the keep regex `putio_sync:.+:[a-z]*5m`;
  - `remote_read` with `read_recent: false` and `required_matchers: {app: putio-sync}`;
  - scrape job `prometheus_longterm` for `127.0.0.1:9092`.

  Everything is copied from contracts/long-term.md.
- [ ] T015 [US3] Write promtool tests in `specs/041-resource-telemetry-archive/alert-tests.yml`
  for the recording rules, using made-up 15 s input:
  - avg and max values;
  - `rate` across a counter reset;
  - activity state is 2 when `downloads_active` averages 2;
  - activity state is -1 when syncing ≥ 0.5;
  - activity state is 0 when idle;
  - downloading wins over syncing.

  Run `promtool check rules` and `promtool test rules` against the rendered template.
- [ ] T016 [US3] (HN) Run `make check` in home-network-new. Fix any lint issues in the new
  templates.

## Phase 5: User Story 2 — CPU and memory over months in Grafana (P1)

**Goal**: a `putio-sync resources` dashboard, seamless across 30 days and 1 year, with a "use
by activity" section.

**Independent test**: quickstart §5 and home-network `make check`.

- [ ] T017 [US2] Write `packaging/grafana/putio-sync-resources.json`:
  - uid `putio-sync-resources`, the same `host` variable as the `putio-sync` dashboard, and a
    link back to it;
  - rows and panels exactly as the table in contracts/long-term.md#dashboard. Over-time panels
    use `raw or on(instance) last_over_time(thinned[10m])`, and every query includes
    `app="putio-sync"`;
  - restart and version annotations;
  - "use by activity" table and the per-MB/s panel, read from the thinned series only;
  - `maxDataPoints` and `$__rate_interval` set, so a 1-year range stays quick (FR-008).
- [ ] T018 [US2] Write `scripts/check-grafana-dashboard.sh`:
  - it checks the JSON is valid (`jq`);
  - it checks every panel query names `app="putio-sync"`;
  - if the path in `HOME_NETWORK_DIR` exists, it compares the file with the home-network copy,
    ignoring Jinja raw markers;
  - it is wired into `make ci` (or `scripts/ci-local.sh`) with the compare skipped when that
    dir is missing.
- [ ] T019 [US2] (HN) Add the dashboard copy and its provisioning:
  - `roles/monitoring_grafana/templates/putio-sync-resources-dashboard.json.j2` holds the
    copy, wrapped so Jinja leaves `{{` alone;
  - `roles/monitoring_grafana/tasks/main.yml` provisions it beside the existing putio-sync
    dashboard;
  - the existing `putio-sync` dashboard gets a link to the new one.
- [ ] T020 [US2] (HN) Update `apps/putio-sync.md` with the bullets in
  contracts/long-term.md#app-page (FR-011): retention, disk estimate, checks, recovery and
  archive pointers.

## Phase 6: User Story 4 — Archive and summary scripts (P2)

**Goal**: `putio-sync archive {metrics,db,summary}` per contracts/archive-cli.md, plus the
Unraid wrapper.

**Independent test**: `cargo test -p putio-sync-archive`, then quickstart §6 and §7.

- [ ] T021 [P] [US4] Test, then write `putio-sync-archive/src/files.rs`:
  - atomic write: `<name>.tmp` in the same dir, fsync, rename;
  - sweep leftover `*.tmp` at the start of each run;
  - `manifest.jsonl` append and read, with line shapes as in data-model.md;
  - `--keep-days` pruning that deletes only `putio-sync-metrics-*.om.gz` and
    `putio-sync-db-*.sqlite.gz` older than N days, plus their manifest lines;
  - `--require-manifest` check;
  - dir create-if-missing.

  Use tempfile tests, including a "foreign file untouched" case.
- [ ] T022 [P] [US4] Test, then write `putio-sync-archive/src/prom.rs`:
  - a range-selector instant query for the selector and time in contracts/archive-cli.md
    `archive metrics` (R9);
  - keep samples in `[day, day+1)`;
  - `--instance` auto-detect, and an error if there are several;
  - map errors to the Prometheus variant.

  Use mockito tests for: normal, empty, HTTP 500, unreachable, and two instances.
- [ ] T023 [P] [US4] Test, then write `putio-sync-archive/src/openmetrics.rs`:
  - one `# TYPE <name> gauge` per family;
  - samples sorted by name, then labels, then time;
  - timestamps in seconds;
  - end with `# EOF`.

  A golden-file test feeds the output through `promtool tsdb create-blocks-from openmetrics`.
  Run that by hand, noted in the test's doc comment.
- [ ] T024 [US4] Test, then write `putio-sync-archive/src/metrics.rs` (`archive metrics`):
  - it covers every finished UTC day from `--since` (default 366 days back) to yesterday that
    is not in the manifest;
  - it writes `metrics/putio-sync-metrics-<day>.om.gz` with gzip;
  - the manifest line has `points` = sample count of `putio_sync:up:min5m`, `expected` = 288,
    and `partial` = `points < expected`;
  - days with no samples print `no data`, nothing is written, and a later run retries them;
  - output is `saved <day> points=N/288[ partial]`.

  Tests: a second run does nothing; a partial day; a gap day; a Prometheus outage gives exit
  2 and changes nothing.
- [ ] T025 [P] [US4] Test, then add `daily_totals(from, to)` to
  `putio-sync-db/src/history.rs`. It returns, per UTC day: `downloads`, `bytes`,
  `download_seconds` and `avg_speed_bytes_per_s`, from download history rows. Use temp SQLite
  tests that cross a UTC midnight.
- [ ] T026 [US4] Test, then write `putio-sync-archive/src/db.rs` (`archive db`):
  - `VACUUM INTO '<dir>/db/.tmp-<uuid>.sqlite'`;
  - gzip to `db/putio-sync-db-<today>.sqlite.gz` through temp and rename, then delete the temp
    copy;
  - the manifest line includes `rows` (download history count);
  - `already saved <today>` and exit 0 if the file already exists;
  - the live DB is opened read-only and never written (FR-015).

  Tests: run it while another connection writes; the gunzipped copy opens and its row count
  matches.
- [ ] T027 [US4] Test, then write `putio-sync-archive/src/summary.rs` (`archive summary`):
  - write `summary/usage-daily.csv`, `summary/usage-monthly.csv` and
    `summary/usage-latest-month.txt` with the columns in data-model.md#usage-summary-row;
  - state seconds = points per state × 300;
  - `downloading_<N>_s` columns run up to the largest N in range;
  - averages weight each point equally;
  - `cpu_per_mbps` skips points with 0 bytes;
  - `restarts` and `versions` follow the data-model rules;
  - `resource_coverage` = points ÷ expected;
  - resource columns are "Blank, never 0, when there is no data";
  - today's row is partial;
  - output is byte-identical for the same inputs (FR-015a).

  Tests: a golden CSV from a mockito Prometheus plus temp SQLite; `--prometheus` omitted; two
  runs compared with `cmp`.
- [ ] T028 [US4] Add the `archive` subcommand in `putio-sync-cli/src/cli.rs`, with a new
  `putio-sync-cli/src/commands/archive.rs` registered in `commands/mod.rs`:
  - flags and env vars exactly as in the contracts/archive-cli.md tables;
  - the default dir is `<config dir>/archive`;
  - the DB path comes from config;
  - exit codes are 0–4;
  - stderr gets one line, `archive <cmd>: <what failed>: <cause>`.

  Add a test that no token appears in any output or file (FR-018), reusing the redaction
  helper.
- [ ] T029 [P] [US4] Write `scripts/archive/unraid-user-script.sh` exactly as in
  contracts/archive-cli.md#unraid-wrapper, then run `shellcheck` on it.
- [ ] T030 [P] [US4] Document the archive commands in `README.md` (FR-022): what each saves,
  where, the schedule on Unraid (plugin required), keep-days, and restore with promtool. Also
  add a comment-only pointer in `config.example.toml`.

## Phase 7: User Story 5 — Alerts when putio-sync uses too much (P3)

**Goal**: 6 alerts in the `putio-sync` group.

**Independent test**: `promtool test rules` firing and silent cases, and home-network
`make check`.

- [ ] T031 [US5] (HN) Add the 6 alerts in contracts/long-term.md#alerts to the `putio-sync`
  group in `roles/monitoring_prometheus/templates/alerts.yml.j2`. Use exact expressions, `for`
  values and severities, with a summary and description that match the existing alerts. Do not
  add a disk alert; `DiskSpaceLow` already covers FR-012.
- [ ] T032 [US5] Extend `specs/041-resource-telemetry-archive/alert-tests.yml` with a firing and
  a silent case for each alert. Include `MemoryGrowing` staying silent across a restart, and
  `LongTermDown` staying silent at 1 h. Run `promtool test rules`.

## Phase 8: Polish

- [ ] T033 Add a `CHANGELOG.md` line under Unreleased covering the process figures, the
  resources dashboard and `putio-sync archive`. Run `scripts/check-changelog.sh`.
- [ ] T034 Run `make ci` and quickstart §1–§3 locally. Record any manual-only steps (§4–§7) as
  unchecked items for the operator in the PR description.

## Dependencies

- Setup (T001–T002) → Foundational (T003–T004) → stories.
- **US1** (T005–T011) needs only T002 and T003.
- **US3** (T012–T016) needs US1 families to exist (it can be written in parallel, but is tested
  after US1).
- **US2** (T017–T020) needs US3's series names.
- **US4**:
  - T021–T023 and T025 can start right after Setup;
  - T024 needs T021–T023;
  - T026 needs T021;
  - T027 needs T022 and T025;
  - T028 needs T024, T026 and T027.
- **US5** (T031–T032) needs US1 families and T013–T014 (the `prometheus_longterm` job).
- Polish comes last.

## Parallel examples

- After Setup: T005, T011, T021, T022, T023 and T025 touch separate files.
- Two writers: one does the putio-sync US1 tasks; one does the home-network T012–T014, T031.
- US4: T029 and T030 alongside T024–T027.

## Implementation strategy

1. **MVP**: Phases 1–3. The figures show on `/metrics`; you can ship and scrape them today, and
   the main Prometheus keeps 30 days.
2. US3 then US2: the year-long copy and the dashboard (home-network PR).
3. US4: archive and summaries (putio-sync PR, plus operator scheduling).
4. US5: alerts.

Each step is its own PR and is useful by itself.
