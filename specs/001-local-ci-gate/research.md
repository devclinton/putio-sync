# Research: Local CI Gate

## R1. Queue reorder route shape

- **Decision**: `POST /local/queue/:file_id/{move_up,move_down,move_first,move_last,skip}`, where
  `file_id` is the Put.io file id the queue rows are keyed on.
- **Rationale**: axum 0.7 allows a catch-all (`/*x`) only as the final segment, so
  `/*path/move_up` panics in `Router::route`. The handlers already take `Path<String>` and
  parse it to `i64`, so a single `:id` segment matches what they expect. `:id` is the same
  style used by `/putio/files/:id` and `/putio/transfers/:id/start`.
- **Alternatives considered**: `POST /local/queue/:action/*path` (an action-first catch-all)
  was rejected because the ids are numbers, not paths. A single `POST /local/queue/:id` with
  an `{action}` body was rejected because it differs from the other route modules.

## R2. Health endpoint

- **Decision**: Add `GET /health`, outside the auth middleware, returning
  `{"status":"ok","version":"<crate version>"}`.
- **Rationale**: The bind test (FR-003) and the NAS container healthcheck need an endpoint
  that works without a token. Constitution VI allows health without auth.
- **Alternatives considered**: Reusing `/local/watcher/status` was rejected: it sits behind
  auth and depends on the watcher.

## R3. Bind test without a fixed port

- **Decision**: Add an integration test that binds `TcpListener::bind("127.0.0.1:0")`, runs
  `axum::serve` on a spawned task, and makes one HTTP request to `/health`. The request uses
  `reqwest`, which is already a workspace dependency, added to `[dev-dependencies]`.
- **Rationale**: Port 0 avoids collisions, and the test runs `axum::serve` itself, the same
  serve path that `server.rs` uses.
- **Alternatives considered**: Refactoring `server.rs` into a testable `run(listener)`
  function is a good follow-up, but it isn't needed to satisfy FR-003.

## R4. Clippy fixes

- **Decision**:
  - In `putio-sync-db`, write the elided lifetime explicitly as `'_` on the 5 repository
    accessors (lines 121-141), for example `-> DownloadRepository<'_>`.
  - In `putio-sync-core` logging tests, build the config with struct-update syntax
    (`Config { field: x, ..Default::default() }`) instead of assigning fields afterwards.
- **Rationale**: These are mechanical, behavior-neutral fixes, and they need no
  `#[allow]`s (FR-005).

## R5. Toolchain pinning

- **Decision**: Add `rust-toolchain.toml` with `channel = "1.96.0"` and
  `components = ["rustfmt","clippy"]`. Remote CI then installs that toolchain through
  `dtolnay/rust-toolchain@1.96.0`, or simply through rustup reading the file.
- **Rationale**: New stable releases add lints, which is how CI broke without any code
  change. Pinning keeps local and remote in lock-step, and bumping becomes a deliberate
  one-line PR.
- **Alternatives considered**: Tracking `stable` was rejected because it reproduces the
  current breakage.

## R6. GitHub Actions cost

- **Decision**:
  - `ci.yml`: on push/PR, run one `ubuntu-latest` job that executes `bash scripts/ci-local.sh`
    with `Swatinem/rust-cache`. On `workflow_dispatch` with input `full: true`, also run
    the macOS/Windows matrix and the beta job.
  - `docker.yml`: drop `pull_request`. On push to `main`, build `linux/amd64` only and push
    the `main` and `sha` tags. On `v*` tags and `workflow_dispatch`, build
    `linux/amd64,linux/arm64`.
- **Rationale**: Remote CI now calls the same script as local runs, so the two cannot drift.
  The arm64 QEMU build is the most expensive step, and the NAS doesn't need it (assumed
  amd64, to be confirmed via SSH).
- **Alternatives considered**: Disabling remote CI entirely was rejected because a single
  Linux job is cheap and catches Linux-only breakage before the NAS image builds.

## R7. Cargo.lock

- **Decision**: Remove `Cargo.lock` from `.gitignore` and commit it.
- **Rationale**: The workspace builds binaries, and Cargo's guidance is to commit the lock
  file for binaries. The CI cache keys already hash it.
