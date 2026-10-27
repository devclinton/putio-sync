# Implementation Plan: Put.io Doctor

**Branch**: `010-putio-doctor` | **Date**: 2026-10-26 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/010-putio-doctor/spec.md`

## Summary

A new `putio-sync-doctor` crate runs 17 read-only checks across four areas: Put.io, sync
state, local and connectivity. Each check is graded OK, Warning, Problem or Skipped, with a
summary in plain English. The doctor also previews and applies six fixes, and every fix checks
its items again before it acts.

Four surfaces use the same `Doctor` service:

- `putio-sync doctor [--json] [--fix ID [--dry-run] [--yes]]`;
- `/doctor` API routes, which require the UI token;
- a Doctor page in the web UI;
- Prometheus gauges, which the daemon refreshes every hour.

The building blocks go in the crates that own them:

- `client`: the probe, the tree walk, and re-add (moved here from `api`);
- `db`: the integrity check and the failed-download queries;
- `core`: the config lint and the `[doctor]` settings.

## Technical Context

**Language/Version**: Rust stable (edition 2021), workspace toolchain

**Primary Dependencies**: tokio, reqwest, axum, sqlx (SQLite), serde, prometheus, clap.
New: `serde_ignored`, in `core` (see R3).

**Storage**: The existing SQLite DB. Checks only read it. Fixes update or remove queue rows.
No schema change. The latest report is kept in memory.

**Testing**: `cargo test`, with:

- `mockito` for Put.io, as in the client, api and daemon crates;
- `tempfile` for the config, DB and download directories;
- API integration tests in `putio-sync-api/tests/doctor.rs`;
- a mock UI seeded with doctor faults.

**Target Platform**: Linux container on the Unraid NAS; macOS for development

**Project Type**: Cargo workspace: CLI, daemon, HTTP API and embedded web UI

**Performance Goals**: A full run on an account of a few thousand files takes less than 60 s
(SC-003). It lists 4 folders at a time and has a 5-minute overall deadline.

**Constraints**:

- The whole UI stays under 150 KB, and `doctor.js` stays at or under 9 KB (R15).
- Checks are read-only.
- No secrets go in the report, the logs or the metrics.
- Metric labels are bounded.

**Scale/Scope**: 17 checks, 6 fixes, 4 surfaces

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Crate boundaries | Pass (new crate) | Each building block goes in the crate that owns it (R2). Orchestration gets a new `doctor` crate, because no existing crate can do it without depending upward (R1). Re-add moves from `api` to `client` so `api` and `doctor` share it. |
| II. Local-first verification | Pass | The new crate is a workspace member, so `make ci` covers it. |
| III. Tests without Put.io | Pass | Every check and fix is tested against `mockito`, and the mock UI gets doctor faults. |
| IV. Download integrity | Pass | The orphan-partial check never touches a partial that a queued, downloading or paused row claims, or one modified in the last hour. Destructive fixes show a preview first (a dry run) and need confirmation. The CLI has `--dry-run`. |
| V. Observable by default | Pass | New `putio_sync_doctor_*` gauges, labelled by check id only (R13). Fixes are logged with structured fields. |
| VI. Secrets stay secret | Pass | The report never includes the token or magnet links. The secrets sweep test covers the doctor endpoints. |
| VII. Simplicity | Pass (new dependency) | `serde_ignored` is justified in R3. Thresholds are constants. Only the schedule and the enable flag are configurable. |

Re-checked after design: still passing. The new crate and the new dependency are recorded under
Complexity Tracking.

## Project Structure

### Documentation (this feature)

```text
specs/010-putio-doctor/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── http-api.md
│   ├── cli.md
│   └── metrics.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-doctor/                 # NEW crate
├── Cargo.toml
└── src/
    ├── lib.rs                     # Doctor service: run (single flight), latest, preview, apply
    ├── report.rs                  # Report, CheckResult, Status, Area, Item, FixInfo, FixOutcome
    ├── checks/
    │   ├── mod.rs                 # CheckId list, run order, shared walk
    │   ├── putio.rs               # auth, storage, transfers, empty folders, duplicates, torrents, watch files
    │   ├── sync.rs                # dead queue rows, failing downloads, orphan partials
    │   ├── local.rs               # download dir, disk space, database, config
    │   └── net.rs                 # reachability, rate limit, clock skew
    └── fixes.rs                   # preview and apply for each FixId
putio-sync-client/src/
├── probe.rs                       # NEW probe_account: account info, latency, Date and rate-limit headers
├── walk.rs                        # NEW walk_files: 4 listings at a time, with a deadline
└── transfers.rs                   # + readd_transfer (moved here from api)
putio-sync-core/src/config.rs      # + DoctorConfig, lint()
putio-sync-db/src/                 # + quick_check, failed-download queries
putio-sync-telemetry/src/doctor.rs # NEW gauges
putio-sync-daemon/src/daemon.rs    # + doctor loop, with_doctor()
putio-sync-api/src/
├── lib.rs                         # AppState.doctor, /doctor nest
├── routes/system/doctor.rs        # NEW routes
└── handlers/transfers.rs          # readd calls the client
putio-sync-api/ui/
├── pages/doctor.js                # NEW page
├── app.js, index.html             # register the page and add the nav link (one line each)
└── app.css                        # small doctor block
putio-sync-api/examples/mock_ui.rs # seed doctor faults
putio-sync-api/tests/doctor.rs     # NEW API tests
putio-sync-cli/src/
├── cli.rs, main.rs                # Doctor subcommand
└── commands/doctor.rs             # NEW: readable and JSON output, --fix flow, exit codes
config.example.toml, README.md     # [doctor] section and docs
```

**Structure Decision**: Workspace crates as above. The new crate sits between the lower crates
(`core`, `client`, `db`, `download`, `telemetry`) and the surfaces (`daemon`, `api`, `cli`).
The existing `health` and `debug` commands stay as they are; the doctor does not replace them
in this feature.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|--------------------------------------|
| New crate `putio-sync-doctor` | Orchestration needs `client`, `db`, `download` and `core` together, and `api`, `daemon` and `cli` all have to use it | No existing crate can depend on all of these without a cycle or an upward dependency (R1) |
| New dependency `serde_ignored` | Finds unknown keys exactly, against the real `Config` type | A hand-kept list of keys drifts out of date. `deny_unknown_fields` would change how the daemon starts (R3). |
