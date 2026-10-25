# Implementation Plan: Local CI Gate

**Branch**: `001-local-ci-gate` | **Date**: 2026-10-25 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/001-local-ci-gate/spec.md`

## Summary

Get `make ci` green on Rust 1.96 and make it the merge gate. That means:

- apply rustfmt
- fix 8 clippy errors: 5 elided-lifetime errors in `putio-sync-db/src/lib.rs:121-141` and
  `field_reassign_with_default` errors in `putio-sync-core/src/logging.rs:354-376`
- replace the invalid `/*path/<action>` queue routes with `/:id/<action>`
- add an unauthenticated `/health` route, plus tests that build the full router and bind a
  real listener
- pin the toolchain and commit `Cargo.lock`
- shrink GitHub Actions to one Linux job, with the matrix and beta jobs on manual trigger only
- limit the Docker workflow to a single-arch `main` image (no PRs, multi-arch only on tags
  and manual runs)

## Technical Context

**Language/Version**: Rust 1.96.0 stable, edition 2021

**Primary Dependencies**: axum 0.7, tower 0.4 (`util`), tower-http 0.5, tokio

**Storage**: SQLite via sqlx (unchanged)

**Testing**: `cargo test`; router tests via `tower::ServiceExt::oneshot`; bind test via
`tokio::net::TcpListener::bind("127.0.0.1:0")`

**Target Platform**: macOS (local gate), Linux x86-64 (remote CI, NAS image)

**Project Type**: Cargo workspace: CLI + daemon + HTTP API

**Performance Goals**: N/A. The server must answer `/health` within 2 s of start (SC-002)

**Constraints**: No network access in tests (Constitution III); no new runtime dependencies

**Scale/Scope**: About 12 files touched; no behavior change except the queue route shape and
the new `/health`

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Crate Boundaries | ✅ | All changes are inside their owning crates; no new cross-crate deps |
| II. Local-First Verification | ✅ | This feature implements it |
| III. Tests Without Put.io | ✅ | Router/bind tests use a dummy token and no outbound calls |
| IV. Download Integrity | ✅ | Not touched |
| V. Observable by Default | ✅ | `/health` is the first piece; metrics come in a later feature |
| VI. Secrets Stay Secret | ✅ | `/health` returns only status/version and is the only unauthenticated route |
| VII. Simplicity | ✅ | No new crates; `rust-toolchain.toml` plus workflow edits only |

Post-design re-check: still passing; no Complexity Tracking entries.

## Project Structure

### Documentation (this feature)

```text
specs/001-local-ci-gate/
├── plan.md
├── research.md
├── quickstart.md
├── contracts/
│   └── http-api.md      # queue reorder + /health contract
└── tasks.md             # /speckit-tasks
```

No `data-model.md`: this feature changes no persisted data or domain entities.

### Source Code (repository root)

```text
rust-toolchain.toml                         # NEW: channel = "1.96.0", rustfmt + clippy
Cargo.lock                                  # NOW COMMITTED
.gitignore                                  # drop Cargo.lock entry
scripts/ci-local.sh                         # print pinned toolchain; unchanged stages
putio-sync-api/src/lib.rs                   # /health route outside auth layer; tests
putio-sync-api/src/routes/local/queue.rs    # /:id/<action> routes; route tests
putio-sync-api/tests/server_bind.rs         # NEW: bind 127.0.0.1:0, GET /health
putio-sync-cli/src/commands/server.rs       # fix stale endpoint list in startup log
putio-sync-db/src/lib.rs                    # explicit lifetimes ('_)
putio-sync-core/src/logging.rs              # struct-init instead of field reassign (tests)
.github/workflows/ci.yml                    # 1 Linux job on push/PR; matrix+beta on dispatch
.github/workflows/docker.yml                # no PRs; amd64 on main; multi-arch on tag/dispatch
MAINTAINING.md                              # "make ci is the gate" + manual matrix how-to
```

**Structure Decision**: Existing workspace layout; no new crates or directories besides one
integration-test file in `putio-sync-api/tests/`.

## Phase 0: Research

See [research.md](research.md). There are no open clarifications.

## Phase 1: Design

- Contract: [contracts/http-api.md](contracts/http-api.md)
- Validation guide: [quickstart.md](quickstart.md)
