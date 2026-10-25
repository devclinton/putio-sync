# putio-sync Constitution

## Core Principles

### I. Crate Boundaries

putio-sync is a Cargo workspace; each crate owns one concern (`client` talks to Put.io, `db`
persists state, `download` moves bytes, `file-manager` touches the local filesystem, `daemon`
schedules work, `api` serves HTTP, `cli` parses arguments, `core` holds shared config and utils).

- New behavior MUST live in the crate that owns the concern, not in `cli` or `api` glue.
- Crates MUST NOT depend "upward" (e.g. `client` or `db` MUST NOT depend on `daemon`, `api`,
  or `cli`).
- Shared types used by two or more crates belong in `core`.

Rationale: the binary is used as both a CLI and a long-running daemon with an HTTP API; clean
boundaries let each surface reuse the same engine.

### II. Local-First Verification (NON-NEGOTIABLE)

Every quality gate MUST be runnable on a developer machine with one command, and that command
is the source of truth. GitHub Actions is an optional mirror, not a requirement.

- `make ci` MUST run the full gate: `cargo fmt --check`, `cargo clippy --all-targets
  --all-features -- -D warnings`, `cargo test --all-features`, and a release build.
- A change is done when `make ci` passes locally; remote CI passing is not required to merge.
- Remote workflows MUST NOT run anything the local gate does not, and SHOULD be cheap
  (single OS by default, manual trigger for the matrix and packaging jobs).

Rationale: GitHub Actions minutes are limited; the maintainer runs checks locally.

### III. Tests Without Put.io

Automated tests MUST NOT require network access to Put.io, a real OAuth token, or real
downloads.

- Put.io interactions MUST be tested against a mock HTTP server (e.g. `wiremock`) or a trait
  seam with a fake implementation.
- Downloads MUST be tested with small synthetic payloads served locally, including
  range/resume and failure-mid-stream cases.
- The API and UI MUST be exercisable against a mocked backend so behavior can be checked
  locally without touching the live instance.
- Bug fixes MUST start with a failing test that reproduces the bug (red → green).

Rationale: tests must be fast, free, deterministic, and safe to run anywhere.

### IV. Download Integrity

A completed download MUST be byte-identical to the Put.io file, and the tool MUST never report
success for a partial file.

- Downloads write to a temporary/partial path and are atomically renamed on completion.
- Resume MUST validate the existing partial size against the remote size before issuing a
  range request.
- Files on Put.io MUST NOT be deleted (post-download delete, empty-folder cleanup) until the
  local copy is verified complete.
- Destructive operations MUST support `--dry-run`.

Rationale: this tool deletes remote data after syncing; a false "complete" loses data.

### V. Observable by Default

A long-running daemon MUST be diagnosable from its logs and metrics alone.

- Logging uses `tracing` with structured fields (file id, name, bytes, duration, error);
  free-form string interpolation of those values is not sufficient.
- The daemon MUST expose Prometheus metrics (text exposition format) for downloads, bytes,
  errors, queue depth, Put.io API calls/latency, watcher activity, and disk space.
- Metric names follow Prometheus conventions (`putio_sync_` prefix, `_total` for counters,
  base units: `_bytes`, `_seconds`); label cardinality MUST stay bounded (no file names or
  ids as labels).
- Metrics and their endpoint MUST be configurable (enable flag, bind address/path) in the
  TOML config.

Rationale: the daemon runs unattended on a NAS for long periods; problems are found from
telemetry, not from a debugger.

### VI. Secrets Stay Secret

- OAuth tokens and API tokens MUST NOT appear in logs, metrics, error messages, or API
  responses; redact them in `Debug`/`Display` impls.
- Config files that can hold tokens stay gitignored; examples use placeholders.
- The HTTP API MUST require authentication for any endpoint that reads Put.io data or
  changes state; only health and metrics endpoints MAY be unauthenticated, and metrics MAY
  be put behind auth via config.

### VII. Simplicity

- Prefer the smallest change that solves the problem; do not add abstraction layers,
  dependencies, or config knobs without a current need.
- Remove dead code and deprecated modules rather than leaving them behind feature flags.
- Every new dependency MUST be justified in the feature plan.

## Technology & Operational Constraints

- Language: Rust (stable, edition 2021), async runtime `tokio`, HTTP client `reqwest`,
  persistence `sqlx` + SQLite, logging `tracing`.
- Configuration: TOML at `~/.putiosync/config.toml`, with environment-variable overrides;
  new settings MUST be documented in `config.example.toml` with safe defaults.
- Deployment target: long-running daemon on a home NAS (Docker image), plus desktop CLI
  installs (Homebrew, Snap, Flatpak, Debian, Winget).
- Hot reload of config MUST NOT drop in-flight downloads.
- Prometheus scraping is done by the home-network monitoring stack (managed in the
  `home-network-new` Ansible repo); this repo only exposes the endpoint and documents it.

## Development Workflow

- Work is driven through Spec Kit: `/speckit-specify` → (`/speckit-clarify`) →
  `/speckit-plan` → `/speckit-tasks` → (`/speckit-analyze`) → `/speckit-implement`.
- One feature per branch, named by Spec Kit's sequential numbering.
- Diagnosis of live-instance problems uses the live logs as evidence; each confirmed bug
  becomes a reproducing test before it is fixed (Principle III).
- Before marking a feature complete: `make ci` passes, `config.example.toml` and README are
  updated for any user-facing change.

## Governance

This constitution supersedes ad-hoc conventions in `Tasks/` and other notes. Feature plans
MUST include a Constitution Check against these principles; any violation MUST be recorded
with a justification in the plan's Complexity Tracking section.

Amendments are made by editing this file through `/speckit-constitution`, with a version bump:
MAJOR for removing or redefining a principle, MINOR for adding a principle or section, PATCH
for wording fixes.

**Version**: 1.0.0 | **Ratified**: 2026-10-25 | **Last Amended**: 2026-10-25
