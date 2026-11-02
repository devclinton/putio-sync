# Implementation Plan: CPU and Memory Telemetry, Kept Long Term

**Branch**: `041-resource-telemetry-archive` | **Date**: 2026-10-28 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/041-resource-telemetry-archive/spec.md`

## Summary

putio-sync adds its own process figures (CPU time, memory, threads, open files, disk I/O) and
a "sync running" gauge to the existing metrics. They are read from `/proc/self` at scrape time
by a collector in `putio-sync-telemetry`.

On the monitoring server, the main Prometheus keeps its 30 days. Recording rules work out
5-minute points (average, peak, activity state) for putio-sync only, and `remote_write` sends
them to a second, small Prometheus with a one-year retention. The main Prometheus
`remote_read`s that copy, so Grafana sees one series covering a year from one data source.

A new resources dashboard shows CPU and memory next to what putio-sync was doing, and a "use
by activity" section. Its source is in this repo, with a copy provisioned in
`~/dev/home-network`.

A new crate, `putio-sync-archive`, behind `putio-sync archive {metrics,db,summary}`, does
three things:
- saves the thinned points to daily OpenMetrics files;
- copies the database with `VACUUM INTO`;
- writes daily and monthly usage summaries (CSV and text).

A daily Unraid User Scripts entry runs it through `docker exec`. Decisions are in
[research.md](research.md).

## Technical Context

**Language/Version**: Rust 1.96 (`rust-toolchain.toml`), edition 2021

**Primary Dependencies**:
- Existing: tokio, axum 0.7, reqwest 0.12, sqlx (SQLite), chrono, serde_json, clap,
  `prometheus` 0.14.
- Changed: `rustix` gains the `param` feature (clock ticks), already locked.
- New: `flate2` (gzip, default pure-Rust backend) in `putio-sync-archive` only (R8).
- Monitoring side: Prometheus and promtool, already installed by Homebrew on the monitoring
  server.

**Storage**:
- putio-sync stores nothing new; process figures stay in memory.
- A new long-term Prometheus TSDB on the monitoring server (a few MB a year).
- Archive files on the NAS under `/mnt/user/appdata/putio-sync/archive`, which is `/config/archive`
  in the container.

**Testing**:
- `cargo test`, with fixture `/proc` text for the parsers. A scrape test on Linux CI checks the
  families are present and asserts they are absent with one warning elsewhere.
- mockito plays Prometheus for the archive and summary commands, with a temp SQLite.
- `promtool test rules` covers the recording and alert rules. It is a documented manual step,
  as in feature 003.
- `make check` in home-network.

**Target Platform**: Linux container on Unraid (production). macOS for development, where
process figures are absent.

**Project Type**: Cargo workspace (one CLI binary), plus Ansible and Jinja changes in
`~/dev/home-network`.

**Performance Goals**:
- A scrape still takes under 1 s; reading `/proc` costs well under 1 ms.
- Downloads slow by no more than 2% (SC-003).
- The archive and summary for one day run in seconds.

**Constraints**:
- Fixed labels, no file names or paths (Principle V, SC-007).
- Archive commands never change the live DB (FR-015).
- Temp file then rename for every output (FR-016).
- Home-network edits are additive.

**Scale/Scope**:
- 9 new metric families.
- About 25 recorded series.
- About 105 k samples per series a year.
- Daily archive files of about 100 KB gzipped.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Status |
|---|---|---|
| I. Crate Boundaries | New code in the owning crate; no upward deps | ✅ Process figures and the sync gauge go in `putio-sync-telemetry`, which owns metrics. Archive, summary and DB copy go in a new `putio-sync-archive` crate. It depends on `core` (config) and `db` (history queries); nothing depends on it except `cli`. A new daily-totals query goes in `putio-sync-db`. `cli` only adds the subcommand wiring. |
| II. Local-First Verification | `make ci` is the gate | ✅ Parsers, collector, archive, DB copy and summary are all tested under `cargo test`. promtool and home-network `make check` are documented extra steps, as in 003. |
| III. Tests Without Put.io | mocks only | ✅ No Put.io calls at all. Prometheus is mocked with mockito, and the DB is a temp SQLite. |
| IV. Download Integrity | unchanged | ✅ The download path is untouched apart from one gauge set per sync. |
| V. Observable by Default | prefix, units, bounded labels | ✅ All new families use `putio_sync_` with base units, and recorded series use `putio_sync:<name>:<agg>5m`. The activity state is a value, not a label. |
| VI. Secrets Stay Secret | no tokens in output | ✅ Archive commands read no tokens and print none. The DB copy holds no tokens (they live in config). A redaction test covers the new metric output and summary files. |
| VII. Simplicity | justify deps and knobs | ✅ `flate2` is justified in R8. No new config section: archive options are CLI flags and env vars only. No in-daemon scheduler (R7). The one new crate is justified below. |

**Post-design re-check**: passes. The only structural addition is `putio-sync-archive`. No
existing crate owns "export and summarize history": `db` must not make HTTP calls to
Prometheus, `telemetry` sits below `db`, and `cli` must stay glue.

## Project Structure

### Documentation (this feature)

```text
specs/041-resource-telemetry-archive/
├── spec.md
├── plan.md                 # this file
├── research.md             # R1–R12
├── data-model.md           # figures, recorded series, activity state, archive files, summary rows
├── quickstart.md           # validation guide
├── contracts/
│   ├── metrics.md          # new metric families + invariants
│   ├── long-term.md        # recording rules, remote write/read, long-term instance, alerts
│   └── archive-cli.md      # `putio-sync archive` commands, files, exit codes
├── alert-tests.yml         # promtool tests (created in tasks)
├── checklists/requirements.md
└── tasks.md                # /speckit-tasks
```

### Source Code (repository root)

```text
putio-sync-telemetry/
├── Cargo.toml              # rustix: + "param"
└── src/
    ├── process.rs          # NEW: /proc parsers + Collector (Linux); warn-once elsewhere
    ├── sync.rs             # + sync_started(); record_run also clears the in-progress gauge
    └── lib.rs              # register collector; new sync_in_progress gauge

putio-sync-download/src/manager.rs   # call telemetry::sync::started() beside sync_status.begin()

putio-sync-db/src/history.rs         # + daily_totals(from, to) for the summary

putio-sync-archive/                  # NEW crate
├── Cargo.toml              # core, db, reqwest, serde_json, chrono, tokio, flate2
├── src/
│   ├── lib.rs
│   ├── prom.rs             # range-selector fetch from Prometheus (R9)
│   ├── openmetrics.rs      # write OpenMetrics text
│   ├── metrics.rs          # `archive metrics`: days not yet saved → .om.gz
│   ├── db.rs               # `archive db`: VACUUM INTO → .sqlite.gz
│   ├── summary.rs          # `archive summary`: daily/monthly rows, CSV + text
│   ├── files.rs            # temp+rename, manifest.jsonl, keep-days pruning
│   └── error.rs
└── tests/                  # mockito Prometheus + temp SQLite

putio-sync-cli/src/cli.rs            # + `archive` subcommand wiring

packaging/grafana/putio-sync-resources.json   # NEW dashboard (source of truth)
scripts/archive/unraid-user-script.sh         # NEW daily wrapper (docker exec)
scripts/check-grafana-dashboard.sh            # NEW: compare with the home-network copy
README.md, config.example.toml (comment only), CHANGELOG.md

~/dev/home-network/ (second repo)
├── roles/monitoring_prometheus/
│   ├── defaults/main.yml                    # long-term instance vars (port 9092, 1y, data dir)
│   ├── tasks/main.yml                       # second LaunchDaemon/Agent, rules file, dirs
│   └── templates/
│       ├── prometheus.yml.j2                # + rule_files, remote_write, remote_read, scrape of :9092
│       ├── putio-sync-recording.yml.j2      # NEW recording rules
│       └── alerts.yml.j2                    # + 6 putio-sync alerts
├── roles/monitoring_grafana/templates/putio-sync-resources-dashboard.json.j2   # NEW copy
├── roles/monitoring_grafana/tasks/main.yml  # provision it
└── apps/putio-sync.md                       # retention, disk estimate, job checks, archive, restore
```

**Structure Decision**: workspace crates as above, plus the second repo. The new crate follows
the existing `putio-sync-<concern>` layout.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| New crate `putio-sync-archive` (not a violation, recorded for review) | Needs both DB history and HTTP to Prometheus, and must be testable under `make ci` | Putting it in `cli` breaks Principle I (logic in glue). Putting it in `db` adds HTTP to the persistence layer. Shell scripts can't be tested in `make ci` (R7). |
| Second Prometheus instance on the monitoring server | The user chose 30 days for everything plus a one-year putio-sync-only copy | Raising retention for everything was declined. Federation loses points on outages, and VictoriaMetrics is new software (R4). |
