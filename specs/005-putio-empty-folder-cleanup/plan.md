# Implementation Plan: Put.io Empty-Folder Cleanup

**Branch**: `005-putio-empty-folder-cleanup` | **Date**: 2026-10-26 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/005-putio-empty-folder-cleanup/spec.md`

## Summary

Find and remove empty folders in the Put.io account: a walk from the root decides, bottom-up,
which folders hold no files (directly or below), keeps protected and too-new folders, and
deletes the top-most deletable folder of each empty subtree (Put.io removes its empty
sub-folders with it). The walk lives in `putio-sync-client`; a small service in
`putio-sync-core` adds settings, a one-run-at-a-time guard, logging and metrics, and is
shared by the daemon's daily schedule, a new API endpoint (used by a button on the Put.io
page) and a new CLI subcommand. A `[cleanup]` config section (enabled, run time, minimum
age) is editable on the Settings page.

## Technical Context

**Language/Version**: Rust stable (edition 2021, `rust-toolchain.toml`); framework-free ES
modules for the UI.

**Primary Dependencies**: existing `tokio`, `reqwest`, `axum` 0.7, `serde`, `tracing`,
`prometheus`, `toml_edit`, `clap`. `chrono` 0.4 (already in the workspace lock, used by
`api`, `cli`, `db`) is added to `putio-sync-client` (parse Put.io `created_at`),
`putio-sync-core` (run time type) and `putio-sync-daemon` (local clock). `putio-sync-core`
is added to `putio-sync-daemon` (the shared cleanup service). No new crates.

**Storage**: none new. Settings in `config.toml`; nothing persisted in SQLite.

**Testing**: `cargo test`; Put.io mocked with `mockito` (as in the client and API tests);
the schedule's "is it time?" check is a pure function tested with fixed times; API through
`tower::ServiceExt::oneshot` with the existing `tests/common` app.

**Target Platform**: Linux container on the NAS (daemon), desktop CLI.

**Project Type**: Cargo workspace (CLI + daemon + HTTP API + embedded web UI).

**Performance Goals**: one list call per folder plus one transfer list; a few hundred
folders finish in well under a minute. Runs once a day.

**Constraints**: never delete what was not fully seen; never touch the live account in
tests; no tokens in logs, errors or responses; one run at a time per process.

**Scale/Scope**: a personal account: hundreds to low thousands of folders.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | How |
|-----------|--------|-----|
| I. Crate boundaries | ✅ | Put.io walk and delete in `client`; config + shared service in `core`; schedule in `daemon`; HTTP in `api`; argument parsing in `cli`; metric families in `telemetry`. `daemon → core` is downward (core does not depend on daemon). |
| II. Local-first verification | ✅ | `make ci` is the gate. |
| III. Tests without Put.io | ✅ | `mockito` for every Put.io call; the clock is injected (`now` parameter) so schedule and age tests need no real time. |
| IV. Download integrity | ✅ | Only folders with no files below them are deleted, so no file can be lost; `--dry-run` / `dry_run` on every manual path. Unseen pages and unreadable folders count as not empty. |
| V. Observable by default | ✅ | Structured `tracing` fields per deleted folder (`folder_id`, `path`, `age_hours`); four new `putio_sync_` families with closed labels (no ids or names). |
| VI. Secrets stay secret | ✅ | New endpoint is behind the existing auth middleware; errors use the client's existing redacted error text; covered by an auth test. |
| VII. Simplicity | ✅ | Three settings, no exclude list, no new crates; `chrono` already in the lock. |

Post-design re-check: unchanged, all pass. No Complexity Tracking entries.

## Project Structure

### Documentation (this feature)

```text
specs/005-putio-empty-folder-cleanup/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── api.md       # POST /putio/cleanup/empty-folders, GET/PUT /settings/cleanup
│   ├── cli.md       # putio-sync files clean-empty
│   ├── config.md    # [cleanup]
│   └── metrics.md   # new metric families
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-client/src/cleanup.rs          # NEW: walk, decide, delete; report types
putio-sync-client/src/lib.rs              # pub mod cleanup + re-exports
putio-sync-telemetry/src/cleanup.rs       # NEW: record_run / folder_deleted / delete_failed
putio-sync-telemetry/src/{lib,labels}.rs  # families + CleanupTrigger label
putio-sync-telemetry/tests/contract.rs    # new families in BASIC_FAMILIES
putio-sync-core/src/config.rs             # CleanupConfig ([cleanup]) + validation
putio-sync-core/src/cleanup.rs            # NEW: EmptyFolderCleanup service (guard, settings, metrics, logs)
putio-sync-daemon/src/schedule.rs         # NEW: is_due() + daily loop
putio-sync-daemon/src/daemon.rs           # spawn the loop when a cleanup service is set
putio-sync-api/src/routes/putio/cleanup.rs  # NEW: POST /putio/cleanup/empty-folders
putio-sync-api/src/routes/system/settings.rs # GET/PUT /settings/cleanup; watcher save reconfigures
putio-sync-api/src/lib.rs                 # AppState.cleanup, with_cleanup(), route
putio-sync-api/tests/cleanup.rs           # NEW: endpoint + settings tests
putio-sync-api/ui/pages/putio.js          # "Remove empty folders" button + confirm
putio-sync-api/ui/pages/settings.js       # Put.io cleanup card
putio-sync-api/examples/mock_ui.rs        # mock the new endpoints for manual UI checks
putio-sync-cli/src/cli.rs, main.rs        # files clean-empty
putio-sync-cli/src/commands/clean_empty.rs  # NEW
putio-sync-cli/src/commands/daemon.rs     # build one service, give it to daemon + API
README.md, config.example.toml
```

**Structure Decision**: existing workspace crates only; one new module per crate that gains
behaviour.

## Complexity Tracking

No violations.
