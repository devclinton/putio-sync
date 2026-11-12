# Implementation Plan: Update Checker and Self-Update

**Branch**: `055-update-checker` | **Date**: 2026-11-10 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/055-update-checker/spec.md`

## Summary

A new crate, `putio-sync-update`, checks GitHub Releases about a minute after start and every
24 h, works out how putio-sync was installed, and either shows that channel's update command
or, for a GitHub archive, updates itself. It downloads a signed bsdiff + xz patch from the
running version, or a signed xz full binary when there is no patch, verifies both hashes,
renames the binary into place, and re-execs. A 90-second trial on the new version rolls back the
binary and the database if it doesn't come up. The `.pkg` and `-setup.exe` installs download and
open the new installer instead. The web UI shows the notice and the release notes (parsed by
the existing changelog code) on the What's new page. The release workflow gains a `patches`
job and signs `SHA256SUMS` with minisign. Decisions are in [research.md](research.md).

## Technical Context

**Language/Version**: Rust stable (`rust-toolchain.toml`), edition 2021; vanilla ES modules for the UI

**Primary Dependencies**: existing `tokio`, `reqwest`, `sha2`, `axum`, `sqlx`, `toml_edit`; new
`bsdiff` 0.2, `lzma-rs` 0.3, `minisign-verify` 0.2 (all pure Rust), and `windows-sys` (Windows
only: `MoveFileExW`, `ShellExecuteW`, `WTSGetActiveConsoleSessionId`)

**Storage**: SQLite table `update_state` (one row), and `update-pending.json` in the config folder for the swap and trial

**Testing**: `cargo test` with `mockito` for the releases API and assets, scratch folders (`tempfile`) for swap, trial and rollback; the swap tests run on every OS `make ci` runs on

**Target Platform**: the 5 archive targets (Linux x86_64/aarch64 musl, macOS arm64, Windows x64/arm64); detection only everywhere else

**Project Type**: Cargo workspace: daemon, HTTP API and embedded web UI

**Performance Goals**: the check is one request a day; a patch update takes under 1 minute from click to running (SC-004); patches are at most ¼ of a full download (SC-003)

**Constraints**: never touch files on a managed channel (FR-010); never replace the binary without a verified signature and hashes (FR-015); the install folder always holds one complete binary (FR-016)

**Scale/Scope**: up to 15 patches + 5 full binaries per release; about 13 channels; 4 routes; one Settings card; one page section

## Constitution Check

| Principle | Status | How |
|-----------|--------|-----|
| I. Crate boundaries | ✅ | `putio-sync-update` owns the concern and depends only on `core`, `telemetry` and leaf crates; `db` holds the repository; `api` has the routes; `cli` only wires (R12) |
| II. Local-first verification | ✅ | Everything runs under `make ci`; `make release-patches` runs the patch tool locally. The workflow job is remote-only by nature (like 053's builds) and runs nothing `make ci` doesn't, apart from packaging |
| III. Tests without Put.io | ✅ | GitHub is mocked with mockito; swap and rollback run on scratch folders with tiny fake binaries (shell or batch scripts that print a version) |
| IV. Download integrity | ✅ | An update interrupts downloads only after confirmation; partial files resume as today; the swap is two renames on the same filesystem |
| V. Observable | ✅ | Structured `tracing` fields (`from`, `to`, `kind`, `stage`, `error`); four metrics with bounded labels (contracts/api.md) |
| VI. Secrets | ✅ | The routes are authenticated; the minisign secret key exists only as a CI secret; the check sends no user data (FR-003) |
| VII. Simplicity | ⚠ justified | 3 small pure-Rust crates plus `windows-sys`; see Complexity Tracking. No `tar`/`zip`/`semver` (R1, R4) |

Re-check after design: unchanged. The debug-only overrides (`PUTIO_SYNC_UPDATE_API`,
`PUTIO_SYNC_UPDATE_PUBKEY`, `PUTIO_SYNC_TEST_FAIL_START`) are compiled out of release builds
with `cfg(debug_assertions)`, so a release binary can't be pointed at another server or key.

## Project Structure

### Documentation (this feature)

```text
specs/055-update-checker/
├── plan.md              # this file
├── research.md          # R1–R12
├── data-model.md        # config, update_state, update-pending.json, types, states
├── quickstart.md        # manual end-to-end checks
├── contracts/
│   ├── api.md           # /api/update routes, Settings section, metrics
│   └── release-assets.md# new assets, SHA256SUMS lines, signature, workflow job
└── tasks.md             # /speckit-tasks
```

### Source code

```text
putio-sync-update/                 # new crate
├── Cargo.toml
├── examples/make-patch.rs         # bsdiff old new out (used by CI and make release-patches)
└── src/
    ├── lib.rs                     # UpdateService: schedule, state, apply()
    ├── version.rs                 # Version parse/compare
    ├── github.rs                  # releases fetch, ETag, filtering
    ├── channel.rs                 # detection (R10) and instructions
    ├── verify.rs                  # minisign public key, SHA256SUMS parse and check
    ├── fetch.rs                   # patch or full download, xz decode, bsdiff apply
    ├── swap.rs                    # unix rename+exec / windows move+spawn
    ├── trial.rs                   # update-pending.json, trial mode, rollback, panic hook
    └── installer.rs               # .pkg / setup.exe download and open (R9)

putio-sync-core/src/config.rs      # [updates] UpdatesConfig
putio-sync-db/src/updates.rs       # update_state repository (+ schema.rs table)
putio-sync-telemetry/src/update.rs # the four metrics
putio-sync-api/src/routes/system/update.rs   # GET/POST /update…
putio-sync-api/src/routes/system/settings.rs # Updates section
putio-sync-api/src/routes/system/status.rs   # install_channel
putio-sync-api/ui/pages/news.js    # the notice, notes, Update now, progress
putio-sync-api/ui/settings/…       # Updates card
putio-sync-api/ui/i18n/*.json      # new strings (atranslate)
putio-sync-cli/src/main.rs         # trial::on_start() before anything else
putio-sync-cli/src/commands/daemon.rs   # build and start UpdateService, pass to AppState
Dockerfile, Dockerfile.dhi         # PUTIO_SYNC_IMAGE_VARIANT, PUTIO_SYNC_IMAGE
packaging/casaos/…, packaging/umbrel/…  # PUTIO_SYNC_INSTALL
.github/workflows/release-binaries.yml  # patches job, signing in checksums
Makefile                           # release-patches
scripts/fake-release.sh            # quickstart helper
config.example.toml, INSTALLATION.md, CHANGELOG.md
```

**Structure Decision**: one new workspace crate, following the existing one-concern-per-crate
layout, plus thin additions to core, db, telemetry, api, cli, packaging and the release workflow.

## Phases for tasks

1. **Checking and notice (US1, P1)**: version, github, db repository, UpdateService schedule,
   GET/check/dismiss routes, the notice on the What's new page, Settings card, metrics.
2. **Channels (US2, P2)**: channel.rs with a test per row of R10, the instructions, the image
   variant env in both Dockerfiles, `PUTIO_SYNC_INSTALL` in the CasaOS and Umbrel files, and
   `install_channel` in status.
3. **Release pipeline (before US3 can be tested end to end)**: make-patch example, the
   `patches` job, signing, `make release-patches`, and the `MINISIGN_*` secrets documented in
   MAINTAINING.md.
4. **Self-update (US3, P3)**: verify, fetch, swap (Unix, then Windows), trial and rollback,
   apply route, UI progress and confirm-to-interrupt. Then installer.rs for `.pkg` and setup.exe.
5. **Auto-install (US4, P4)**: the idle wait, never retrying a rolled-back version.
6. **Docs and polish**: config.example.toml, INSTALLATION.md ("Updating"), CHANGELOG line,
   i18n strings, quickstart run on Linux, macOS and Windows.

## Complexity Tracking

| Addition | Why needed | Simpler alternative rejected because |
|----------|------------|-------------------------------------|
| New crate `putio-sync-update` | Principle I: a concern with its own platform code | putting it in `core` would pull Windows APIs and patching into every crate's dependency |
| `bsdiff`, `lzma-rs` | the patch format the user chose (clarify) | C liblzma complicates the static musl build |
| `minisign-verify` | signed `SHA256SUMS` (clarify) | hand-rolled Ed25519 verification is riskier than a 300-line audited crate |
| `windows-sys` (Windows only) | rename a running exe, open an installer elevated, detect the session | `std` has no `MoveFileEx` flags or `ShellExecute` |
| New release assets (`.xz`, patches, `.minisig`) | patch and full downloads without unpacking archives (R4) | unpacking would add `tar` and `zip`, and the patch would still need the bare binary |
