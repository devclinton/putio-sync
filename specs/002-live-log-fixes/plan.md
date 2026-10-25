# Implementation Plan: Fixes from the Live NAS Logs

**Branch**: `002-live-log-fixes` | **Date**: 2026-10-25 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/002-live-log-fixes/spec.md`

## Summary

Six defects found in seven weeks of NAS daemon logs, each fixed in the crate that owns it, with a reproducing test first:

| Fix | Crate | Change |
|-----|-------|--------|
| Stale pooled connections and a short retry budget | `client` | Short idle timeout and TCP keepalive; retries spread over about 31 s; full error chain in logs |
| Lock errors when workers claim queue items | `db` | Single-statement atomic claim; WAL mode with an explicit busy timeout |
| Stuck watch files | `file-manager` | Process outcomes as an enum; cleanup also runs for files that were already uploaded |
| Log noise | `file-manager`, `daemon` | Idle scan summaries go to debug |
| Settings that lie | `cli`, `daemon`, `core` | Honour `--download-interval`; banner prints effective values; log which source set the log level |
| Executable guard | `core`, `download` | `download.blocked_extensions`, default-on |

Design details are in [research.md](research.md).

## Technical Context

**Language/Version**: Rust 1.96.0 (pinned in `rust-toolchain.toml`), edition 2021

**Primary Dependencies**: tokio, reqwest 0.12, sqlx 0.8 (SQLite, bundled), tracing and
tracing-subscriber, serde/toml. No new dependencies.

**Storage**: SQLite file `<download_dir>/.putiosync.db`, which moves to WAL mode (new
`-wal`/`-shm` sidecar files).

**Testing**: `cargo test` via `make ci`. Tests use mockito for Put.io, a raw tokio TCP
server for connection-drop cases, and tempfile-backed SQLite.

**Target Platform**: Linux x86_64 Docker on Unraid (primary), plus desktop CLI targets.

**Project Type**: Cargo workspace providing a CLI, a daemon and an HTTP API.

**Performance Goals**: no regression in sync or download throughput. Claiming a queue item
waits at most 10 s under contention.

**Constraints**:
- No Put.io network access in tests.
- Tokens never appear in logs.
- Hot reload must not drop downloads.

**Scale/Scope**: one daemon, 3 workers, about 40 watch files, and a few hundred Put.io files.

## Constitution Check

*Checked before Phase 0 and again after Phase 1.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Crate boundaries | ✅ | Each fix lives in its owning crate. The shared config field goes in `core`. No upward dependencies. |
| II. Local-first verification | ✅ | `make ci` is the gate; no workflow changes. |
| III. Tests without Put.io | ✅ | mockito, a local TCP server and temp SQLite. Every fix starts red (FR-014). |
| IV. Download integrity | ✅ | Blocked files are never downloaded or deleted. The claim stays atomic (stronger than before). Watch-file cleanup deletes local blackhole files only, as today. |
| V. Observable by default | ✅ (partial) | Structured error chain and fewer noisy logs. Metrics are feature 003. New log lines use structured fields (`file_id`, `name`, `reason`). |
| VI. Secrets stay secret | ✅ | The error chain is asserted token-free. |
| VII. Simplicity | ✅ | No new dependencies or abstraction layers. The TOML download-interval key is deferred until it's needed. The unused `blacklist_patterns` is left as-is and noted for later. |

No violations, so the Complexity Tracking table is empty.

## Project Structure

### Documentation (this feature)

```text
specs/002-live-log-fixes/
├── spec.md
├── plan.md            # this file
├── research.md        # R1–R6 decisions
├── data-model.md      # touched entities and states
├── quickstart.md      # local + NAS validation
├── contracts/
│   └── config-cli-logs.md
└── checklists/requirements.md
```

### Source Code (files touched)

```text
putio-sync-client/src/client.rs           # R1: pool/keepalive, RetryPolicy, error_chain
putio-sync-client/tests/stale_connection.rs  # R1 test (new)
putio-sync-db/src/lib.rs                  # R2: WAL, busy_timeout
putio-sync-db/src/queue.rs                # R2: atomic claim + concurrency test
putio-sync-file-manager/src/watcher.rs    # R3/R4: ProcessOutcome, cleanup, log levels
putio-sync-daemon/src/daemon.rs           # R4/R5: download_interval, scan log level, blocked list wiring
putio-sync-cli/src/cli.rs, main.rs, commands/daemon.rs, commands/download.rs  # R5/R6
putio-sync-core/src/config.rs, logging.rs # R5/R6: blocked_extensions, LevelSource
putio-sync-download/src/manager.rs        # R6: is_blocked, files_blocked, warn-once
putio-sync-api/src/routes/system/settings.rs  # R6: expose blocked_extensions
config.example.toml, README.md            # docs for the new setting / behaviour
```

**Structure Decision**: existing workspace layout; no new crates or modules beyond one
integration test file.

## Phasing

Each phase can be implemented and shipped alone, in this order of value:

1. **US2: DB claim.** It's the smallest change and removes ERROR lines.
2. **US1: HTTP client.** The biggest source of log noise, and it has lost syncs.
3. **US4: executable guard.** The security issue.
4. **US3: watch cleanup.** It unblocks the 41 stuck files.
5. **US5: logs and settings.**

## Complexity Tracking

None.
