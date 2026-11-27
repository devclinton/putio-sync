# Implementation Plan: Usage Profile in the Setup Wizard

**Branch**: `feat/056-usage-profile-wizard` | **Date**: 2026-11-26 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/056-usage-profile-wizard/spec.md`

## Summary

The setup wizard gets a profile path: language, setup link (code in the URL fragment), Put.io
device link, one "how will you use putio-sync?" screen, and a review screen with **Start
syncing**. A new `putio_sync_core::profile` module turns the answers (kinds, arrival methods,
watch folder, UI language) and what was detected (top-level Put.io folders) into a list of
recommendations, each a few typed settings with a reason code. Setup mode stores the answers in
the session and applies the plan at `POST /setup/finish`; the answers and the values the plan
set go into a new `[profile]` config section. Settings › Usage profile runs the same plan
against the live config, previews it (015 style, with kept hand-made changes), and saves it
through the normal settings save, so hot reload applies it. Radarr/Sonarr use the existing
018/049 routes; the 049 folder mapping and recommendations are shown after the daemon starts.

## Technical Context

**Language/Version**: Rust stable (edition 2021); browser JavaScript modules for the UI

**Primary Dependencies**: existing only (axum, serde, serde_json, toml, tokio); no new crates

**Storage**: `config.toml` (new `[profile]` section); browser localStorage for the UI language (051)

**Testing**: `cargo test` (core unit tests for the plan; API tests against mocked Put.io with
mockito, as 013's setup tests); `node --test` for UI helpers

**Target Platform**: NAS daemon (Docker), desktop installs

**Project Type**: Cargo workspace: core library + HTTP API + embedded web UI

**Performance Goals**: setup in under 60 s; the usage screen shows within 3 s even while
discovery runs (discovery results stream in when they arrive)

**Constraints**: UI text translatable (051) — the daemon returns reason codes, the UI words
them; secrets never in responses (VI); the setup code never in a query string or log

**Scale/Scope**: six kinds, two *arr kinds, ~8 settings keys

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | Plan logic in `putio-sync-core::profile` (shared by setup mode and Settings); routes in `putio-sync-api`; banner in `putio-sync-cli`. Pass |
| II. Local-first verification | `make ci`, clippy, tests, lite build, UI tests all local. Pass |
| III. Tests without Put.io | Put.io and *arr mocked with mockito; plan tested as pure functions. Pass |
| IV. Download integrity | Delete-after-download only recommended, shown with the 015 risks, and switchable off; existing verify-before-delete stays. Pass |
| V. Observable | Setup finish logs the profile kinds (no paths or secrets) with tracing fields. Pass |
| VI. Secrets | Setup code only in the URL fragment (never sent to the server in a URL), cleared from history; *arr keys never returned. Pass |
| VII. Simplicity | No new dependency; one new config section; reuses 013/015/018/049 routes. Pass |

Post-design re-check: Pass (no violations).

## Project Structure

### Documentation (this feature)

```text
specs/056-usage-profile-wizard/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/profile-api.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/src/profile.rs          # Kind, Answers, plan(), Key get/set, diff/apply, ProfileConfig
putio-sync-core/src/config.rs           # Config.profile ([profile])
putio-sync-api/src/setup.rs             # /setup page route, link base, profile in Session
putio-sync-api/src/routes/setup.rs      # /setup/profile/detect, /setup/profile, finish applies plan
putio-sync-api/src/routes/system/profile.rs  # /settings/profile GET, preview, PUT
putio-sync-cli/src/commands/setup_mode.rs    # banner prints the setup link
putio-sync-api/ui/profile.js            # pure helpers: code from hash, reason text, value editors
putio-sync-api/ui/pages/setup.js        # language first, link unlock, profile path, manual path
putio-sync-api/ui/setup_profile.js      # usage screen, review screen, after-start *arr checks
putio-sync-api/ui/settings/profile.js   # Settings › Usage profile
putio-sync-api/tests/setup_profile.rs   # API tests
putio-sync-api/tests/ui/profile.test.mjs
```

**Structure Decision**: existing workspace layout; one new core module, one new settings
route module, three new UI modules (registered in `putio-sync-api/src/ui.rs`).

## Complexity Tracking

None.
