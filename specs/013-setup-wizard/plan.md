# Implementation Plan: First-Run Setup Wizard

**Branch**: `013-setup-wizard` (worktree branch `claude/great-turing-febaee`) | **Date**: 2026-11-03 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/013-setup-wizard/spec.md`

## Summary

`putio-sync daemon` stops exiting when it is not configured. It decides at startup whether
setup is needed (no or unparsable config, no Put.io token, a token Put.io rejects, an unusable
download folder) and, if so, runs a small **setup server** on the normal API address: the web UI
plus `/setup/*` routes, unlocked by a one-time setup code printed to the console (or the API
token). The web UI shows a wizard page (unlock → Put.io → folder → what to download → extras →
review → finish). Choices live in an in-memory setup session; "Start syncing" writes them to the
config file in one save, the setup server shuts down, and the daemon continues into its normal
startup in the same process. A config poller lets `putio-sync setup` (the CLI version of the same
flow) finish a daemon that is waiting in setup mode.

The engine pieces live in the crates that own them (Principle I): the Put.io device link in
`client`, setup checks and applying choices to a `Config` in `core`, the dry run in `download`
(built on the existing read-only `preview_rules` tree walk), the HTTP surface in `api`, the
terminal flow and the daemon's setup loop in `cli`. 018's discovery and auto-setup functions are
called directly; 009's presets get one addition: an account-wide `[download] preset`.

## Technical Context

**Language/Version**: Rust stable (edition 2021, workspace `rust-toolchain.toml`); the web UI is
plain ES modules with no build step (`putio-sync-api/ui/`).

**Primary Dependencies**: existing only — tokio, axum, reqwest, serde, toml/toml_edit, uuid (api),
rustix (telemetry's `statvfs`). No new crates (R10).

**Storage**: `config.toml` (owner-only permissions, written by `Config::save`); the setup session
is in memory. No database use in setup mode.

**Testing**: `cargo test` with `wiremock`/`mockito` for Put.io (device link, account info, file
listing) and the *arr; `tower::ServiceExt::oneshot` for the setup router; `node --test` for any
plain UI module; CLI setup tested through its non-interactive path with a fake prompt source.

**Target Platform**: Linux (Docker, snap, NAS), macOS and Windows binaries.

**Project Type**: Cargo workspace — daemon + HTTP API + embedded web UI + CLI.

**Performance Goals**: link confirmed within 10 s of the user entering the code (polling every
3 s); dry run bounded at 60 s; leaving setup mode within 15 s of a CLI save (poll every 5 s).

**Constraints**: no secrets in logs/metrics/responses (VI); UI bundle cap raised from 260 KB to
300 KB (user decision); setup mode must never sync, watch, download or delete (FR-002).

**Scale/Scope**: one operator per install; accounts up to tens of thousands of files (dry run
reports partial past the deadline).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Crate boundaries | Pass | Device link → `client::oob`; checks/apply → `core::setup`; dry run → `download::dry_run`; routes → `api::setup`; terminal flow + daemon loop → `cli`. No upward deps (core already depends on client and telemetry). |
| II. Local-first verification | Pass | Everything runs under `make ci`; no remote-only checks. |
| III. Tests without Put.io | Pass | OOB link, account info and listings mocked; dry run against mocked listings; setup router via `oneshot`. |
| IV. Download integrity | Pass | Dry run lists only (reuses the read-only walk); setup mode starts no download manager loop; nothing deleted. |
| V. Observable by default | Pass | Structured logs for enter/leave setup with reasons; new gauge `putio_sync_setup_required` (no labels). |
| VI. Secrets stay secret | Pass with note | Setup code is printed to the console with `println!`/`eprintln!`, never through `tracing` (log files, archive). API token returned once by `/setup/finish`. Put.io token never returned. See Complexity Tracking. |
| VII. Simplicity | Pass | One new config key each for app ID and root preset; no new dependencies; the wizard reuses 018/009 code. |

Post-design re-check (after Phase 1): still passes; the only deviation is recorded below.

## Project Structure

### Documentation (this feature)

```text
specs/013-setup-wizard/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── setup-api.md      # /setup/* routes, setup-mode behaviour of other routes
│   ├── putio-oob.md      # Put.io device-link calls putio-sync makes
│   └── cli.md            # putio-sync setup
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-client/src/
├── oob.rs                 # NEW: device link (request code, check code), base URL for tests
└── error.rs               # is_unauthorized()

putio-sync-core/src/
├── setup.rs               # NEW: SetupCheck/reasons, FolderCheck, suggestions, SetupChoices::apply,
│                          #      app ID resolution, token/code generation, in_container()
├── config.rs              # [putio] app_id, [download] preset, validation
└── rules.rs               # account-wide preset at the root of RuleSet::resolve

putio-sync-download/src/
└── dry_run.rs             # NEW: DryRun over the tree walk + local existence check

putio-sync-telemetry/src/  # putio_sync_setup_required gauge (+ contract test)

putio-sync-api/src/
├── setup.rs               # NEW: SetupState, SetupSession, setup router (own auth)
├── routes/setup.rs        # NEW: /setup/* handlers
├── routes/system/auth.rs  # /auth/status gains `setup_required: false`
└── lib.rs                 # pub use setup

putio-sync-api/ui/
├── pages/setup.js         # NEW: wizard page
├── app.js                 # boot: setup-mode check, route to the wizard
├── api.js                 # setup-session bearer (sessionStorage) helpers
└── app.css                # wizard styles (steps, checks)

putio-sync-cli/src/
├── commands/setup.rs      # NEW: `putio-sync setup` (prompts + flags)
├── commands/setup_mode.rs # NEW: daemon's setup loop (serve, wait, poll config, shut down)
├── commands/daemon.rs     # check → setup mode → normal start
└── cli.rs / main.rs       # `setup` subcommand

config.example.toml, README.md, INSTALLATION.md, snap/README.md, CHANGELOG.md, ROADMAP.md
```

**Structure Decision**: existing workspace crates; one new module per owning crate, as listed.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| A secret (the one-time setup code) is printed to the console (VI says tokens never appear in logs) | Docker/snap users can only read the console to bootstrap; this is the user's chosen gate | Writing the code to a file needs `docker exec` and a shell; leaving setup open lets anyone on the LAN claim the install. The code is printed outside `tracing` (no log file, no telemetry archive), is valid only while setup mode runs, and is dead once setup completes. |
