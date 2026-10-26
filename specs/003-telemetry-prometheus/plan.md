# Implementation Plan: Telemetry and Prometheus Monitoring

**Branch**: `003-telemetry-prometheus` | **Date**: 2026-10-25 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/003-telemetry-prometheus/spec.md`

## Summary

The daemon and server modes will publish Prometheus metrics from one listener on
`0.0.0.0:9742/metrics`. The metrics cover:
- Put.io API calls, retries and latency;
- sync runs;
- queue depth and downloads;
- blocked files and watch-folder activity;
- DB errors, disk space and build info.

All metric definitions live in a new bottom-layer crate, `putio-sync-telemetry`
(`prometheus` 0.14 plus axum). The engine crates (`client`, `db`, `download`,
`file-manager`) call small helpers in it, so both modes report the same metrics without
duplicated code.

Verbosity:
- New `[metrics]` settings get `PUTIO_SYNC_*` env overrides.
- `RUST_LOG` stays the log-level override.
- The log level is reloaded live by a 10 s mtime poller. This replaces the unused
  notify-based watcher.

In `~/dev/home-network`, add four things:
- a scrape job;
- six alert rules;
- a Grafana dashboard;
- an `apps/putio-sync.md` page.

Decisions are in [research.md](research.md).

## Technical Context

**Language/Version**: Rust 1.96 (`rust-toolchain.toml`), edition 2021 (no let-chains)

**Primary Dependencies**:
- Existing: tokio, axum 0.7, reqwest 0.12, sqlx (SQLite), tracing and tracing-subscriber.
- New: `prometheus` 0.14 (`default-features = false`).
- Newly enabled for direct use: `rustix` with the `fs` feature. It is already in the lock
  file.
- Removed: `notify` from `putio-sync-daemon`.

**Storage**: none new. Metrics are kept in memory, and settings live in the existing TOML
file.

**Testing**:
- `cargo test`, with mockito for Put.io and a local TCP or axum listener for scrape tests.
- tracing-subscriber test layers for log assertions.
- `promtool test rules` for alerts. This is a manual step, not part of `make ci`.

**Target Platform**: Linux container on Unraid (nas01), plus macOS for development.

**Project Type**: Cargo workspace. It builds one CLI binary with `daemon` and `server`
modes, plus Ansible/Jinja config in a second repo.

**Performance Goals**:
- A scrape takes under 1 s during downloads.
- Throughput drops by no more than 2% with metrics on (SC-002).
- Helpers do atomic increments only; no allocation per call on the hot path except label
  lookup, and the label values are `&'static str`.

**Constraints**:
- Bounded labels (Principle V).
- No secrets in output (Principle VI).
- A listener failure doesn't stop the daemon (FR-008).
- Edits in home-network must be additive, alongside that repo's unrelated
  uncommitted work.

**Scale/Scope**:
- 1 instance and about 25 metric families.
- At most around 150 series at `basic`, plus about 3 per worker at `detailed`.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Status |
|---|---|---|
| I. Crate Boundaries | New code goes in the crate that owns it; no upward deps; shared types in `core` | ⚠️ Exception recorded in [Complexity Tracking](#complexity-tracking). The `telemetry` crate sits below `client`, so nothing depends upward. But it holds the label types shared by six crates (the rule says `core`) and serves the metrics HTTP endpoint (the rule gives HTTP to `api`). Settings parsing and the reload poller do go in `core` (config and logging). Instrumentation goes in each engine crate, and the CLI only wires things together. |
| II. Local-First Verification | `make ci` is the gate | ✅ All new tests run under `cargo test`. promtool and home-network `make check` are documented extra steps, not remote-only. |
| III. Tests Without Put.io | mock HTTP only | ✅ mockito, synthetic payloads and temp configs. |
| IV. Download Integrity | unchanged | ✅ Instrumentation only. "Completed" is counted after the existing verify-and-rename step. |
| V. Observable by Default | Prometheus metrics for the listed areas; naming; bounded labels; configurable enable/bind/path | ✅ This feature implements exactly that, with every listed area covered (see [contracts/metrics.md](contracts/metrics.md)). |
| VI. Secrets Stay Secret | no tokens in metrics or logs; metrics MAY be unauthenticated | ✅ Unauthenticated, per clarification Q2, which the constitution allows. A redaction test runs over the full output. |
| VII. Simplicity | justify every dependency; remove dead code | ✅ `prometheus` is justified in R1. `rustix` and `axum` are already locked. The unused `ConfigWatcher` and `notify` are removed (R8). The new config knobs are only the ones FR-009 requires. |

**Post-design re-check**: passes, with one recorded exception to Principle I. The new
crate (`putio-sync-telemetry`) is the only structural addition. No existing crate can host
metrics for `client` without an upward dependency. The justification is in Complexity
Tracking.

## Project Structure

### Documentation (this feature)

```text
specs/003-telemetry-prometheus/
├── spec.md
├── plan.md              # this file
├── research.md          # R1–R12 decisions
├── data-model.md        # settings, reload state, closed label sets
├── quickstart.md        # validation guide
├── contracts/
│   ├── metrics.md       # endpoint + metric families + invariants
│   ├── config.md        # TOML/env settings, startup lines, errors
│   └── alerts.md        # scrape job + alert rules (home-network)
├── alert-tests.yml      # promtool rule tests (created in tasks)
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
putio-sync-telemetry/            # NEW crate (bottom layer)
├── Cargo.toml                   # prometheus (no default features), tokio, axum, tracing, rustix(fs)
└── src/
    ├── lib.rs                   # Registry, set_detail(), init(workers, build info)
    ├── labels.rs                # closed enums: ApiOp, ApiOutcome, RetryReason, SyncKind, QueueState, FailReason, DbOp
    ├── api.rs / sync.rs / download.rs / watch.rs / db.rs   # helper fns per area
    ├── gauges.rs                # refresh tick: queue counts (via closure) + statvfs disk
    └── server.rs                # serve(&MetricsSettings) -> axum on TcpListener

putio-sync-client/src/client.rs          # execute_with_retry records attempts/outcomes/retries; methods pass ApiOp
putio-sync-db/src/{lib.rs,queue.rs}      # db error counter; queue counts-by-status for the gauge tick
putio-sync-download/src/manager.rs       # sync run timing/result, per-sync counts, downloads, bytes, active guard, blocked
putio-sync-file-manager/src/watcher.rs   # scans, uploads, cleanups, invalid, cleanup failures
putio-sync-core/src/config.rs            # MetricsConfig + env overlay + validation; spawn_reload_poller
putio-sync-core/src/logging.rs           # reload::Layer handle, apply_level(), PUTIO_SYNC_LOG_FORMAT
putio-sync-daemon/src/{config_watcher.rs,daemon.rs,lib.rs}, Cargo.toml   # remove ConfigWatcher + notify
putio-sync-cli/src/commands/{daemon.rs,server.rs}  # start metrics listener + gauge tick + reload poller
Cargo.toml                               # add workspace member + prometheus workspace dep
Dockerfile                               # ARG/ENV PUTIO_SYNC_GIT_SHA for build_info
config.example.toml, README.md           # [metrics] section, env table, metric list, query cookbook

~/dev/home-network/ (separate repo, additive edits, not committed without approval)
├── group_vars/monitoring_server/main.yml                       # putio_sync scrape job
├── roles/monitoring_prometheus/templates/alerts.yml.j2         # putio-sync alert group
├── roles/monitoring_grafana/templates/putio-sync-dashboard.json.j2   # P3
├── roles/monitoring_grafana/tasks/main.yml                     # provision the dashboard
└── apps/putio-sync.md                                          # endpoint, port, verify, queries
```

**Structure Decision**: this is the existing workspace layout plus one new crate,
`putio-sync-telemetry`. Tests go in each crate's `tests/` directory, matching feature 002
(for example, `putio-sync-client/tests/metrics.rs`).

## Delivery Order (maps to user stories)

1. **US1 (P1)**:
   - telemetry crate and listener;
   - instrumentation in client, download, file-manager and db;
   - gauge tick;
   - `[metrics]` settings and their env overrides (the part of US4 that US1 needs);
   - CLI wiring in both modes.
2. **US2 (P1)**:
   - scrape job and `apps/putio-sync.md`;
   - README metric list and query cookbook.
3. **US4 (P2)**: log-format env override, live level reload, removal of `ConfigWatcher`,
   detailed metrics.
4. **US3 (P2)**: alert group and `alert-tests.yml`.
5. **US5 (P3)**: Grafana dashboard.

## Complexity Tracking

| Violation | Why needed | Simpler alternative rejected because |
|---|---|---|
| Principle I: shared label types live in the new `putio-sync-telemetry` crate, not in `core` | `client`, `db`, `download` and `file-manager` all record metrics, so the types must sit below every one of them. `core` depends on `client`, so `client` can't depend on `core` without a cycle. | **Types in `core`:** creates a `client` → `core` → `client` cycle. **Metrics per crate with no shared crate:** duplicates the registry, breaks the closed label sets and gives no single place to encode the output. |
| Principle I: the metrics HTTP listener is in `putio-sync-telemetry`, not `api` | Daemon mode doesn't use the `api` crate. FR-007 needs the same listener in both modes, with nothing on the API port. The listener is one route that returns the encoded registry. | **Listener in `api`:** daemon mode would pull in the whole API crate for one route, and the metrics would sit next to the authenticated routes that FR-007 keeps apart. **Listener in `cli`:** it's two-mode glue and would be duplicated in each command. |

If this pattern recurs, amend Principle I with `/speckit-constitution` to name a
bottom-layer telemetry crate (MINOR bump).
