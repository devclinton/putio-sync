# Tasks: Local CI Gate

**Input**: Design documents from `/specs/001-local-ci-gate/`

**Prerequisites**: plan.md, spec.md, research.md, contracts/http-api.md, quickstart.md

**Tests**: Required (Constitution III: bug fixes start with a failing test).

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [x] T001 Add `rust-toolchain.toml` (channel `1.96.0`, components rustfmt, clippy) at repo root
- [x] T002 Remove `Cargo.lock` from `.gitignore`; generate it with `cargo generate-lockfile` if
  missing, and stage it

## Phase 2: Foundational

- [x] T003 Run `cargo fmt --all` across the workspace (formatting only, no other edits)

**Checkpoint**: `cargo fmt --all -- --check` passes.

## Phase 3: User Story 1 - API server starts again (P1) 🎯 MVP

**Goal**: `create_app` builds every route; the server binds and answers `/health`.

**Independent Test**: `cargo test -p putio-sync-api` passes, including the bind test.

### Tests (write first, confirm red)

- [x] T004 [US1] In `putio-sync-api/src/routes/local/queue.rs` tests, add router tests that
  send `POST /:id/move_up`, `/move_down`, `/move_first`, `/move_last` and `/skip` through
  `oneshot`, and assert that none of them returns 404/405. Add one that sends
  `POST /abc/move_up` and asserts 400. These sit with the existing `test_router_creation`,
  which currently panics.
- [x] T005 [P] [US1] Add `putio-sync-api/tests/server_bind.rs`: bind `127.0.0.1:0`, serve
  `create_app(...)` with a dummy-token config and no DB, `GET /health` → 200 with a `status`
  of `ok`, and `GET /stats` without a token → 401. Add `reqwest` to `putio-sync-api`
  `[dev-dependencies]`.

### Implementation

- [x] T006 [US1] Change the queue routes in `putio-sync-api/src/routes/local/queue.rs` from
  `/*path/<action>` to `/:id/<action>`, and update the module doc comments
- [x] T007 [US1] Make the queue action handlers return `ApiError::NotFound` when the id isn't
  in the queue (contract: 404, not 500) in `putio-sync-api/src/routes/local/queue.rs`
- [x] T008 [US1] Add a `GET /health` handler returning `{status, version}`, mounted outside
  the auth middleware, in `putio-sync-api/src/lib.rs`
- [x] T009 [P] [US1] Fix the startup log in `putio-sync-cli/src/commands/server.rs` so it
  lists the real endpoint prefixes (`/health`, `/putio/*`, `/local/*`, `/settings`, `/stats`,
  `/history`)

**Checkpoint**: `cargo test -p putio-sync-api` is green, and SC-004 is verified by temporarily
reverting T006.

## Phase 4: User Story 2 - One local command is the gate (P1)

**Goal**: `make ci` exits 0 on a clean checkout.

**Independent Test**: `make ci`

- [x] T010 [P] [US2] Fix the 5 `elided lifetime` clippy errors in
  `putio-sync-db/src/lib.rs:121-141` with explicit `'_`
- [x] T011 [P] [US2] Fix the 3 `field_reassign_with_default` clippy errors in
  `putio-sync-core/src/logging.rs:354-376` by using struct-update initialization
- [x] T012 [US2] Run `cargo clippy --all-targets --all-features -- -D warnings` and fix any
  remaining errors the first fixes uncover (no blanket `#[allow]`)
- [x] T013 [US2] Update `scripts/ci-local.sh` so it reports the toolchain from
  `rust-toolchain.toml`, and make sure every stage exits non-zero on failure (FR-004)
- [x] T014 [US2] Run `make ci` end-to-end and record the result

**Checkpoint**: `make ci` passes (SC-001).

## Phase 5: User Story 3 - Remote CI is cheap (P2)

**Goal**: A push to `main` runs one CI job and one single-arch image build.

**Independent Test**: Read the workflow triggers against quickstart §4.

- [x] T015 [P] [US3] Rewrite `.github/workflows/ci.yml`:
  - push/PR: one `ubuntu-latest` job that runs `bash scripts/ci-local.sh`, using
    `Swatinem/rust-cache`
  - `workflow_dispatch` input `full` (boolean): also runs the macOS/Windows matrix and the
    beta job
- [x] T016 [P] [US3] Edit `.github/workflows/docker.yml`:
  - remove the `pull_request` trigger
  - platforms: `linux/amd64` on a branch push, `linux/amd64,linux/arm64` on tags and
    `workflow_dispatch`
- [x] T017 [US3] Confirm that the release and packaging workflows still trigger only on
  release, tag, or dispatch (no edits expected)

## Phase 6: Polish

- [x] T018 [P] Update `MAINTAINING.md`: `make ci` is the merge gate, how to run the full
  remote matrix (`gh workflow run ci.yml -f full=true`), and how to bump the pinned toolchain
- [x] T019 Run quickstart.md §1-3 and confirm the expected outputs

## Dependencies

- T001 → T003 → everything else (pinned toolchain and formatting baseline).
- US1 (T004-T009) and US2 (T010-T013) touch different files and can run in parallel. T014
  depends on both.
- US3 depends only on T013 (the script it calls).

## Implementation Strategy

MVP is US1 (the server starts). Then US2 makes `make ci` green, and then US3 cuts the
remote cost. Commit after each phase.

## Implementation Notes (2026-10-25)

- Once the first failures were fixed, `make ci` exposed pre-existing failures that had been
  hidden behind them. All are fixed:
  - `logging::test_invalid_log_level`: a real bug. `EnvFilter` accepted any bare word as a
    target name, so a typo in `logging.level` silently disabled logging. Plain values are now
    validated as levels.
  - 13 stale `ApiClient` doc examples in `putio-sync-client`, now `PutioApiClient`.
  - The stale `Config::save` doc example, plus 2 `create_app` doc examples in
    `putio-sync-api`.
- Every Docker workflow run on `main` has failed: the Dockerfile copies `Cargo.lock`, which
  was gitignored. The fix is committing the lock file (T002).
- Result: `make ci` exits 0 with 314 tests passing. The release binary serves `/health`
  without a token, and other routes return 401 without the token and 200 with it.
