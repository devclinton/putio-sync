# Implementation Plan: Put.io Storage Management

**Branch**: `033-putio-storage` (worktree branch `claude/quizzical-mcclintock-8a20a5`) | **Date**: 2026-11-03 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/033-putio-storage/spec.md`

## Summary

A new crate `putio-sync-storage` holds a `StorageService`, built the same way as the
empty-folder cleanup service: it holds the `[storage]` settings, lets one free-up run happen
at a time, and is shared by the API, the CLI and the daemon. It reads the account's disk
numbers and the root listing for the storage card (`GET /putio/storage`). For a free-up it
walks the account with the doctor's walker, filters files through the eligibility rules
(completed download record with the same id and size, local file present with that size,
not queued, not shared, not under an unfinished transfer's save folder), sorts them oldest
first and takes files until the target is reached. The result is a **plan** kept in memory
under a random id (`POST /putio/storage/plan`). `POST /putio/storage/free {plan_id,
file_ids}` deletes only files of that plan, checking each again first, records the run in
two new tables (`storage_runs`, `storage_run_files`) and adds History "deleted" rows.
`GET /putio/storage/runs` lists recent runs with their files. The daemon gets a loop that
refreshes two Put.io storage gauges every 15 minutes and, when `[storage] auto_free` is on,
runs plan-and-free at `run_at` if free space is below the warning level. The doctor's
storage check uses the configured warning level. The web UI gets a storage card at the top
of the Put.io page (bar, top items, warning, Free up space with preview and confirm, recent
runs), a notice on the Status page, and a Storage group on the Settings page. The CLI gets
`putio-sync files free-space [--dry-run] [-y]`. The UI bundle cap goes to 384 KB.

## Technical Context

**Language/Version**: Rust stable (edition 2021); browser JavaScript (ES modules, no build)

**Primary Dependencies**: existing only (`tokio`, `axum`, `sqlx`, `serde`, `chrono`,
`tracing`, `prometheus` via telemetry, `uuid` v4 for the plan id, already used by the
API; see research R7). No new external crate.

**Storage**: SQLite, two new tables (see [data-model.md](data-model.md)); reads `downloads`
and `download_queue`; writes `download_history`.

**Testing**: `cargo test`: planner unit tests (pure function over walked files, records,
queue and transfers); service tests against `mockito` Put.io and a temp database with real
temp files as local copies; db repository tests; API integration tests
(`putio-sync-api/tests/storage.rs`) for summary, plan, free, stale plan, runs, auth, setup
mode 503; doctor check unit tests; config validation tests; UI asset tests (module embedded,
`node --check`, size cap).

**Target Platform**: the daemon on Linux (Docker on a NAS), macOS and Windows desktops.

**Project Type**: Cargo workspace with an embedded web UI.

**Performance Goals**: the storage card is two Put.io calls (account info, root listing).
A plan is one account walk (4 folders at a time, 120 s limit), one transfer list and two
SQLite reads. A free-up does one file-details call per planned file and deletes in batches
of 100 ids.

**Constraints**: never delete without a verified local copy (Principle IV); never delete a
file the user did not see in the preview; dry run everywhere (the plan is the dry run).
UI bundle cap raised from 300 KB to 384 KB (user decision).

**Scale/Scope**: accounts of a few thousand files; plans are capped at 1,000 files; the
runs list shows the latest 20 runs.

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | The service needs the Put.io client, the database and the local filesystem; `core` must not depend on `db`, so it gets its own crate `putio-sync-storage` (like `putio-sync-precheck`), used by `api`, `cli` and `daemon`. Disk-number and walk helpers stay in `client`; tables in `db`; `[storage]` config in `core`; gauges and counters in `telemetry`. No upward dependency. Pass. |
| II. Local-first verification | Covered by `make ci`; the Dockerfile check gets the new crate's lines. Pass. |
| III. Tests without Put.io | All Put.io calls go to `mockito`; local copies are temp files. Pass. |
| IV. Download integrity | Files are deleted only with a completed record whose local file exists with the Put.io size, and each is re-checked just before deletion. The plan is the dry run; the CLI has `--dry-run`. Pass. |
| V. Observable by default | Runs log with structured fields; metrics: `putio_sync_putio_storage_size_bytes`, `putio_sync_putio_storage_used_bytes`, `putio_sync_storage_free_runs_total{trigger,result}`, `putio_sync_storage_freed_files_total`, `putio_sync_storage_freed_bytes_total`. Bounded labels. Pass. |
| VI. Secrets stay secret | Routes behind the auth middleware; setup mode 503s them (separate router). No tokens in responses. Pass. |
| VII. Simplicity | One new crate is justified by I. Four config keys, each needed by the spec. The plan id avoids trusting client-sent paths. Pass. |

Post-design re-check: unchanged, all pass.

## Project Structure

### Documentation (this feature)

```text
specs/033-putio-storage/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/storage-api.md
└── tasks.md
```

### Source Code (repository root)

```text
Cargo.toml                                  # + putio-sync-storage member
Dockerfile                                  # + the new crate's COPY / stub lines
config.example.toml                         # + [storage]
putio-sync-storage/                         # NEW crate
├── Cargo.toml
└── src/
    ├── lib.rs                              # StorageService, errors, summary, runs
    └── plan.rs                             # eligibility + oldest-first selection (pure)
putio-sync-client/src/walk.rs               # WalkedFile gains is_shared (not serialized)
putio-sync-core/src/config.rs               # StorageConfig + validate
putio-sync-db/src/schema.rs, lib.rs         # storage_runs, storage_run_files
putio-sync-db/src/storage.rs                # StorageRepository (record run, recent runs)
putio-sync-db/src/downloads.rs, queue.rs    # completed records; active queue ids
putio-sync-telemetry/src/storage.rs         # gauges + counters
putio-sync-doctor/src/checks/putio.rs       # storage check uses [storage] warn level
putio-sync-api/src/lib.rs                   # AppState.storage, router nest
putio-sync-api/src/routes/putio/storage.rs  # the four routes
putio-sync-api/src/routes/system/settings.rs, assistant.rs  # storage settings group
putio-sync-daemon/src/schedule.rs, daemon.rs# storage loop (gauges, auto run)
putio-sync-cli/src/commands/free_space.rs   # files free-space
putio-sync-api/ui/pages/putio.js            # storage card, preview, runs
putio-sync-api/ui/storage.js                # NEW: the card's code (keeps putio.js small)
putio-sync-api/ui/pages/status.js           # low-space notice
putio-sync-api/ui/pages/settings.js         # Storage group
putio-sync-api/ui/app.css                   # bar styles
putio-sync-api/tests/ui_assets.rs           # cap 384 KB
putio-sync-api/tests/storage.rs             # NEW integration tests
```

**Structure Decision**: one new crate for the service (Principle I), everything else in the
crates that own each concern.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| New crate `putio-sync-storage` | The service joins client, db and local fs | Putting it in `core` would make `core` depend on `db`; putting it in `api` would leave the CLI and daemon without it |
