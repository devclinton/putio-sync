# Implementation Plan: Configuration Assistant

**Branch**: `015-config-assistant` (worktree branch `claude/objective-bell-6f6977`) | **Date**: 2026-11-03 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/015-config-assistant/spec.md`

## Summary

Add a preview to every settings group, risk warnings, field help, per-group defaults, and
settings import/export. The logic lives in a new `putio-sync-core/src/assistant.rs`: risk
rules over a `Config`, a dotted-key diff between two configs' settings, and the
export/import merge, reusing `settings_table` and `Config::save_keeping_file_credentials`.
The API refactors each group's PUT so that the code that applies the body to a config copy
is shared by the new `POST /settings/{group}/preview`, and adds `GET /settings/assistant`
(help, live fields, defaults, current risks), `GET /settings/export` and
`POST /settings/import`. The doctor's config check adds the risk warnings. The Settings page
shows the preview and help in each card, a Defaults button, a warnings banner, and a
"Backup and restore" card.

## Technical Context

**Language/Version**: Rust stable (edition 2021); browser JavaScript (ES modules, no build)

**Primary Dependencies**: existing only: `axum`, `serde`, `serde_json`, `toml`, `toml_edit`,
`serde_ignored` (all already in the workspace). No new dependencies.

**Storage**: the TOML config file; nothing new in SQLite.

**Testing**: `cargo test` (core unit tests for risks, diff, export/import; API integration
tests in `putio-sync-api/tests/settings_assistant.rs` against a temp config file; doctor unit
test); `node --check` on the changed UI modules through the existing UI asset tests.

**Target Platform**: the daemon on Linux (Docker on a NAS), macOS and Windows desktops.

**Project Type**: Cargo workspace with an embedded web UI.

**Performance Goals**: a preview is a config clone, a validate and a diff: well under 50 ms;
the UI debounces previews to one per 400 ms of typing.

**Constraints**: UI total under 300 KB (now about 285.7 KB, so at most about 14 KB for
this feature, leaving room for others); no secrets in any response or export; a preview
never writes.

**Scale/Scope**: six settings groups, about 45 fields, seven risk rules.

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | Risk rules, diff and import merge are config logic: `core`. The API only wires routes; the doctor calls core. No upward dependency. ✅ |
| II. Local-first | Everything is covered by `make ci`. ✅ |
| III. Tests without Put.io | No Put.io calls. Container and temp-folder checks take their inputs as parameters so tests do not depend on the host. ✅ |
| IV. Download integrity | The feature adds a warning for delete-after-download without a lasting local copy; it changes no download or delete path. Previews never write. ✅ |
| V. Observable | No new metrics needed: the feature is request/response, and the doctor surfaces the warnings. Import apply logs at info with the count of changed keys (no values). ✅ |
| VI. Secrets | Export and the import diff use `settings_table`, which drops `[auth]`, `[arr]` and `api.auth_token`; tests assert no secret in export or responses. Help and defaults hold no secrets. ✅ |
| VII. Simplicity | No new crates, config knobs or dependencies. The PUT refactor removes duplication instead of adding a layer. ✅ |

Post-design re-check: unchanged, all pass.

## Project Structure

### Documentation (this feature)

```text
specs/015-config-assistant/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   └── settings-assistant-api.md
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/src/
├── assistant.rs          # NEW: Risk, risks(), Change, diff(), export_toml(), import()
├── config.rs             # settings_table made pub(crate)
└── lib.rs                # pub mod assistant

putio-sync-api/src/routes/system/
├── settings.rs           # apply_<group> fns shared by PUT and preview; preview routes;
│                         # live-field fns shared by save message and help
└── assistant.rs          # NEW: GET /settings/assistant, GET /settings/export,
                          # POST /settings/import (mounted under /settings)

putio-sync-api/tests/settings_assistant.rs   # NEW integration tests
putio-sync-doctor/src/checks/local.rs        # config check adds risk warnings

putio-sync-api/ui/
├── api.js                # previewSettings, settingsAssistant, exportSettings, importSettings
├── pages/settings.js     # preview panel, help line, Defaults, warnings banner, backup card
└── app.css               # a few rules for the preview and badges
```

**Structure Decision**: one new core module and one new route module; the rest are edits to
existing files.

## Complexity Tracking

No violations.
