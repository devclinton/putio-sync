# Implementation Plan: Per-folder file rules

**Branch**: `009-folder-rules` | **Date**: 2026-10-26 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/009-folder-rules/spec.md`

## Summary

Folder rules keyed by the Put.io-relative folder path (which is also the local path) live in `[download]` as `[[download.rules]]`, with user presets in `[download.presets.<name>]` on top of five built-ins. A new pure module `putio-sync-core::rules` normalises paths, validates, and resolves the effective rule for a folder (per-setting nearest-rule-wins, type filter folded root to leaf, R2/R3). The download manager uses it in `enqueue_tree` (block, skip, delete blocked files in one Put.io call), and per file for delete-after-download and skip-existing. Decisions are recorded in a new `rule_decisions` table, logged once per file and counted by `putio_sync_rule_decisions_total{decision}`. A new `/rules` API sets and clears rules by Put.io folder id, edits presets, previews (dry run) a rule, and lists decisions. The Put.io page gets a rule card with the effective rule and its sources plus an editor; Settings gets rules, presets and recent decisions. `edit_table` learns arrays of tables so UI saves keep `[[download.rules]]` and its comments. Decisions: [research.md](research.md) R1–R9.

## Technical Context

**Language/Version**: Rust 1.96 (`rust-toolchain.toml`), edition 2021. Browser: ES2020 modules, no build step (feature 004).

**Primary Dependencies**: existing only: serde, toml, toml_edit, sqlx, axum, prometheus, mockito (tests).

**Storage**: `config.toml` for rules and presets; SQLite table `rule_decisions` (new, `CREATE TABLE IF NOT EXISTS`).

**Testing**: `cargo test` (unit tests in core for resolution; mockito for the download manager and API routes); UI by hand via `mock_ui`.

**Target Platform**: Linux Docker on the NAS; desktop builds unaffected.

**Project Type**: Cargo workspace (daemon + HTTP API + embedded web UI).

**Performance Goals**: no extra Put.io calls in the sync except one `delete_files` per sync when there is something to delete; rule lookup is one hash probe per path segment per file.

**Constraints**: UI under 200 KB uncompressed (raised from 150 KB with this feature; about 160 KB after it, with 012 themes); no file names or paths in metric labels; one config writer.

**Scale/Scope**: tens of rules, thousands of files per sync.

## Constitution Check

| Principle | Status |
|---|---|
| I. Crate boundaries | Rule types and resolution in `core` (used by `download` and `api`); application in `download`; table in `db`; counter in `telemetry`; routes and UI in `api`. ✅ |
| II. Local-first | All checks in `make ci`. ✅ |
| III. Tests without Put.io | Resolution is pure; manager and routes tested against mockito; UI via `mock_ui`. ✅ |
| IV. Download integrity | Delete-after-download keeps its place after the verified rename. Delete-blocked removes only files that were never going to be downloaded, skips queued ones, and has a dry run (`POST /rules/preview`) shown before saving (R6). ✅ |
| V. Observable | Structured warn log per decision (file_id, name, folder, rule, reason), new counter with a closed label set, existing counter kept. ✅ |
| VI. Secrets | Routes behind the token middleware; saves keep file credentials. ✅ |
| VII. Simplicity | No new dependencies. New config knobs are the feature itself. `blocked_extensions` stays the root filter rather than being migrated. ✅ |

Post-design re-check: unchanged, no violations.

## Project Structure

### Documentation (this feature)

```text
specs/009-folder-rules/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/api.md
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/src/
├── rules.rs            # NEW: FolderRule, Preset, built-ins, normalise/validate, RuleSet, Effective, Decision
├── config.rs           # DownloadConfig.rules / .presets, validate, edit_table arrays of tables
└── lib.rs              # pub mod rules
putio-sync-download/src/
├── manager.rs          # DownloadConfig.rules, enqueue_tree decisions + deletes, per-file options, preview, folder path
└── blocklist.rs        # kept for its defaults
putio-sync-db/src/
├── schema.rs, lib.rs   # rule_decisions table
└── decisions.rs        # NEW: RuleDecisionsRepository
putio-sync-telemetry/src/
├── labels.rs, lib.rs, sync.rs   # RuleDecision label, rule_decisions_total
putio-sync-api/src/
├── routes/system/rules.rs       # NEW: /rules routes
├── routes/system/settings.rs    # update() shared (pub(crate))
├── routes/local/queue.rs        # `deleted` in the response
├── lib.rs                       # nest /rules
putio-sync-api/ui/
├── pages/putio.js      # rule card + editor + preview + blocked badges
├── rules.js            # NEW: shared rule helpers (labels, effective rendering) for putio.js and settings.js
├── pages/settings.js   # Folder rules, Presets, Recently blocked
└── app.css             # small additions
putio-sync-api/examples/mock_ui.rs  # temp config file with a rule and a comment, nfo file, seeded decision
README.md, config.example.toml
```

**Structure Decision**: existing workspace layout; one new module per crate that owns a concern.

## Complexity Tracking

None.
