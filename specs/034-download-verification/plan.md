# Implementation Plan: Download Verification

**Branch**: `034-download-verification` (worktree branch `claude/infallible-mclaren-082190`) | **Date**: 2026-11-04 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/034-download-verification/spec.md`

## Summary

Hash each download with CRC-32 as its bytes are written (seeded from the part already on
disk when it resumes), compare with Put.io's `crc32` before the hidden file is renamed, and
download again from the start on a mismatch, up to `verify_max_redownloads` times, before
failing with reason `integrity`. `download.finished` keeps its place after the rename, so
031's media-server plugins only hear about verified files. A `VerifyRunner` in
`putio-sync-download` walks the `downloads` table for "verify everything" (or a chosen set
of paths), reading from disk through a rate limiter of its own, waiting while 032 says
paused, and queues mismatches to be downloaded over the bad copy. Results go to new columns
on `download_history` and `downloads`, and to two new tables (`verify_runs`,
`verify_problems`), and surface in History, the doctor, metrics, events, the Queue page and
the Local files list.

## Technical Context

**Language/Version**: Rust (workspace toolchain in `rust-toolchain.toml`); vanilla ES modules for the UI

**Primary Dependencies**: `crc` 3.4 (already used by 040), tokio, sqlx (runtime queries), axum, prometheus

**Storage**: SQLite via `putio-sync-db` (columns added with `add_column_if_missing`, new tables with `CREATE TABLE IF NOT EXISTS`)

**Testing**: `cargo test` with mockito/wiremock Put.io mocks and small synthetic payloads; node tests for UI helpers

**Target Platform**: Linux NAS (Docker, snap), macOS for development

**Project Type**: Rust workspace: daemon + HTTP API + web UI

**Performance Goals**: no extra disk read for a fresh download; CRC-32 (slice-by-16) at several GB/s, well above line speed; a run within 10% of its read limit

**Constraints**: UI bundle under 384 KB; setup mode returns 503; no tokens in logs/events; bounded metric labels

**Scale/Scope**: tens of thousands of downloaded files per NAS; one run at a time

## Constitution Check

| Principle | How this plan meets it |
|-----------|------------------------|
| I. Crate boundaries | Hashing and the runner live in `putio-sync-download`; tables in `putio-sync-db`; config and events in `core`; metrics in `telemetry`; the check in `doctor`; routes and UI in `api`. No upward dependency. |
| II. Local-first verification | `make ci` covers all of it. |
| III. Tests without Put.io | Mismatch, re-download, resume and run tests use mockito/wiremock and files in temp dirs. |
| IV. Download integrity | Strengthened: the CRC32 is checked before the atomic rename; the post-download delete only follows a verified success; a run only reads and re-queues, never deletes a local file. |
| V. Observable by default | Structured `tracing` fields (file_id, name, method, expected, actual); new `putio_sync_verify_*` metrics with bounded labels (method, result, trigger). |
| VI. Secrets stay secret | Events, problems and logs carry names and paths only; download URLs are never stored or logged. |

Gate: passes; nothing in Complexity Tracking.

## Project Structure

### Documentation (this feature)

```text
specs/034-download-verification/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── api.md
│   └── events.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/src/config.rs          # [download] verify_* keys, validation
putio-sync-core/src/events.rs          # verify.passed / verify.failed / verify.redownload
putio-sync-core/src/events/schema.json
putio-sync-db/src/schema.rs, lib.rs    # columns + verify_runs, verify_problems
putio-sync-db/src/verify.rs            # VerifyRepository (new)
putio-sync-db/src/history.rs, downloads.rs, queue.rs
putio-sync-download/src/files.rs       # Crc32 digest type, prefix hashing
putio-sync-download/src/stream.rs      # hash in the writer task
putio-sync-download/src/manager.rs     # verify before rename, re-download loop, overwrite flag
putio-sync-download/src/verify.rs      # VerifyRunner (new)
putio-sync-download/src/pipeline.rs    # "verifying" in the 039 view
putio-sync-telemetry/src/verify.rs     # metrics (new)
putio-sync-doctor/src/checks/verify.rs # "Download verification" check (new)
putio-sync-api/src/routes/system/verify.rs  # /verify routes (new)
putio-sync-api/src/routes/system/history.rs # verify fields
putio-sync-api/ui/pages/history.js, files.js, queue.js, settings.js
putio-sync-api/ui/verify.js            # DOM-free helpers + node tests (new)
EVENTS.md, README.md, CHANGELOG.md, config.example.toml, example-config.toml
```

**Structure Decision**: existing workspace crates; one new module per crate where the
concern is new (runner, repository, metrics, check, route, UI helper).

## Phase 0 / Phase 1 outputs

- [research.md](research.md): R1–R9 decisions.
- [data-model.md](data-model.md): columns, tables, run state machine.
- [contracts/api.md](contracts/api.md): `/verify` routes and History fields.
- [contracts/events.md](contracts/events.md): the three event types.
- [quickstart.md](quickstart.md): how to check it end to end.

Post-design Constitution Check: unchanged, passes.

## Complexity Tracking

None.
