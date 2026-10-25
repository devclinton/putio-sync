# Feature Specification: Local CI Gate

**Feature Branch**: `001-local-ci-gate`

**Created**: 2026-10-25

## Clarifications

### Session 2026-10-25

- Q: How does the NAS get its image? → A: It pulls the GHCR image built on push to `main`.

**Status**: Ready for planning

**Input**: User description: "Make the local CI gate pass and make it the source of truth. `make ci` must pass on Rust 1.96: fix rustfmt diffs; fix clippy -D warnings errors (5x elided-lifetime lint in putio-sync-db, 3x field_reassign_with_default in putio-sync-core tests); fix the API router panic in putio-sync-api/src/routes/local/queue.rs where "/*path/move_up" etc. use a catch-all that is not at the end of the route (axum rejects it, so create_app panics and the server cannot start; 2 tests fail). Commit Cargo.lock (currently gitignored) so builds are reproducible. Reduce GitHub Actions cost: the CI workflow should mirror `make ci` on a single OS, with the OS matrix and beta job only on manual trigger; packaging/release workflows should not run on every push. Add a regression test proving the full app router builds and the server binds."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - API server starts again (Priority: P1)

The maintainer runs `putio-sync server` (locally or on the NAS) and the HTTP API comes up and
answers requests, including the download-queue reordering actions (move up, move down, move to
first, move to last, skip).

**Why this priority**: On current `main` the server aborts during startup because one route
definition is invalid. Nothing that depends on the API (the planned web UI, telemetry
endpoint, remote control) can work until this is fixed.

**Independent Test**: Start the server with a test config and a temporary database, call the
health endpoint and a queue reorder endpoint with a valid token, and get successful responses.

**Acceptance Scenarios**:

1. **Given** a valid config, **When** the server is started, **Then** it binds its port and
   the health endpoint responds successfully.
2. **Given** a queued item with id 42, **When** a client asks to move item 42 up, **Then** the
   request is routed to the move-up action and the queue order changes.
3. **Given** a non-numeric item id, **When** a client asks to move it, **Then** the client
   receives a "bad request" error, not a server error.
4. **Given** the full set of API routes, **When** the automated test suite runs, **Then** a
   test builds the complete application router and fails if any route definition is invalid.

---

### User Story 2 - One local command is the quality gate (Priority: P1)

The maintainer runs `make ci` on their Mac and gets a single pass/fail answer covering
formatting, lints (warnings treated as errors), all tests, and a release build. When it passes
locally, the change is considered ready.

**Why this priority**: Remote CI minutes are scarce; the maintainer needs to trust the local
run. Today it fails at every stage (formatting, 8 lint errors, 2 test failures).

**Independent Test**: On a clean checkout of the branch, run `make ci`; it exits 0.

**Acceptance Scenarios**:

1. **Given** a clean checkout, **When** `make ci` runs, **Then** it exits successfully with no
   formatting diffs, no lint errors, no failing tests, and a successful release build.
2. **Given** a change that introduces a formatting diff, a lint warning, or a failing test,
   **When** `make ci` runs, **Then** it exits non-zero and names the failing stage.
3. **Given** two machines building the same commit, **When** each builds, **Then** both
   resolve the exact same dependency versions.

---

### User Story 3 - Remote CI is cheap (Priority: P2)

Pushing to `main` or opening a pull request triggers at most one small CI job that runs the
same checks as `make ci` on a single operating system. Cross-platform checks, the beta
toolchain check, and packaging builds run only when started manually or on a release.

**Why this priority**: Saves GitHub Actions minutes; correctness is already guaranteed by the
local gate.

**Independent Test**: Inspect the workflow triggers: a push to `main` schedules exactly one
CI job on one OS and no packaging jobs.

**Acceptance Scenarios**:

1. **Given** a push to `main`, **When** workflows are evaluated, **Then** only the single-OS
   CI job runs plus the single-architecture image build the NAS pulls.
2. **Given** the maintainer manually triggers CI with the "full matrix" option, **When** it
   runs, **Then** Linux, macOS, Windows, and the beta toolchain are all checked.
3. **Given** a published release or a version tag, **When** workflows are evaluated, **Then**
   the packaging/release workflows run as they do today.

### Edge Cases

- A newer stable toolchain introduces new default lints: the gate should fail loudly; the
  toolchain version used by the gate is pinned so local and remote agree.
- Two routes that differ only by a trailing action segment (`/42/move_up` vs `/42/skip`) must
  each reach their own action.
- An item id that parses but does not exist in the queue returns "not found", not a server
  error.
- The regression test must not need network access, a Put.io token, or a fixed free port.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The API server MUST start successfully with the complete route set; no route
  definition may cause a startup failure.
- **FR-002**: The queue reorder actions (move up, move down, move first, move last, skip)
  MUST be addressable per queue item id, and existing behavior of each action MUST be
  preserved.
- **FR-003**: The automated tests MUST include a test that builds the full application with
  every route and a test that starts the server on an ephemeral local port and gets a
  successful health response.
- **FR-004**: `make ci` MUST run, in order: format check, lint with warnings as errors (all
  targets, all features), all tests (including doc tests), and a release build; it MUST stop
  at the first failing stage with a non-zero exit.
- **FR-005**: `make ci` MUST pass on the pinned stable toolchain (Rust 1.96) with zero lint
  suppressions added solely to silence the current 8 errors, unless a suppression is
  justified inline.
- **FR-006**: The dependency lock file MUST be committed so every build of a commit uses the
  same dependency versions.
- **FR-007**: The toolchain version used by the gate MUST be pinned in the repository so
  local and remote runs use the same compiler and lints.
- **FR-008**: The CI workflow on push/pull request MUST run the same checks as `make ci` on a
  single OS; the multi-OS matrix and beta-toolchain checks MUST run only on manual trigger.
- **FR-009**: The container image workflow MUST NOT run on pull requests. On pushes to `main`
  it MUST still build and push the `main`-tagged image (the NAS pulls it), but only for the
  NAS's CPU architecture; multi-architecture images are built only for version tags and
  manual runs.
- **FR-010**: Release and packaging workflows (binaries, Homebrew, Snap, Flatpak, Debian,
  Winget) MUST continue to run only on release/tag/manual triggers (no change from today
  beyond fixing anything that breaks the shared gate).
- **FR-011**: Documentation (README or MAINTAINING) MUST state that `make ci` is the merge
  gate and how to trigger the full remote matrix manually.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: `make ci` on a clean checkout exits successfully on the maintainer's machine.
- **SC-002**: The server starts and answers its health check within 2 seconds of launch with a
  test configuration.
- **SC-003**: A push to `main` consumes at most one CI job on one OS (versus 4 jobs across 3
  OSes plus a multi-architecture image build today).
- **SC-004**: Reintroducing an invalid route definition causes the test suite to fail.

## Assumptions

- The queue reorder endpoints are not yet called by any client (there is no UI yet), so their
  URL shape can change from a catch-all to a per-item-id path without a compatibility shim.
- Fixes for the 8 lint errors are mechanical and do not change behavior.
- `make ci` is run on macOS (Apple Silicon); the single remote OS will be Linux, matching the
  NAS deployment target.
- The NAS is x86-64 (linux/amd64); confirm when SSH access is set up and adjust the
  single-architecture build if not.
- Live-instance bug fixes, telemetry, and the web UI are separate features (002+).
