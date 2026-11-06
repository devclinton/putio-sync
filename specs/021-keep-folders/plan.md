# Implementation Plan: Keep folders out of empty-folder cleanup

**Branch**: `021-keep-folders` (worktree branch `claude/021-keep-folders-d85578`) | **Date**: 2026-11-05 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/021-keep-folders/spec.md`

## Summary

`FolderRule` gains `keep: Option<bool>` and `Effective` a sourced `keep`, so the rule
resolution of feature 009 gives inheritance and "set back to no" for free. `[cleanup]`
gains `keep_patterns: Vec<String>`, checked with `pattern::check` (feature 020).

The walk in `putio-sync-client::cleanup` can't see core's rules (the client crate sits
below core), so `EmptyFolderOptions` gains a `keep` callback (`KeepFn`): given a folder's
path from the root, it returns why the user keeps it, or nothing. `core::cleanup` builds it
from the `RuleSet` and the parsed keep patterns, and refreshes it on `configure`, which
rule saves now call too. The walk adds `Keep::User(reason)` after `Protected` and before
`TooNew`, and the report gains `kept: Vec<KeptFolder>` (every empty folder not removed,
with its reason) and a `kept_by_user` count. The doctor reuses the run, so it follows the
keep settings; its check summary mentions the user-kept count.

UI: the rule FLAGS list gains Keep (card row, editor select, summary in Settings), the rule
card gets a one-click Keep folder / Don't keep button, the Put.io cleanup settings group
gets a Keep patterns list field, and the Remove empty folders preview names kept folders.
`clean-empty` prints the kept list.

## Technical Context

**Language/Version**: Rust stable (edition 2021); browser ES modules for the UI

**Primary Dependencies**: none new

**Storage**: config TOML only (one optional rule key, one `[cleanup]` list). No database change.

**Testing**: `cargo test` (client walk tests with mockito, core rules/cleanup unit tests, API
settings/rules route tests); `node --test` UI tests via `tests/ui_js.rs` if UI test files
cover the touched modules.

**Target Platform**: Linux NAS daemon (Docker), desktop CLI

**Project Type**: Cargo workspace: daemon + HTTP API + web UI

**Performance Goals**: one rule resolution and pattern match per folder walked; negligible
next to the Put.io listing call per folder.

**Constraints**: no new crate dependency edges (client must not depend on core).

**Scale/Scope**: a handful of rules and patterns; folder trees of thousands.

## Constitution Check

| Principle | Status |
|---|---|
| I. Crate boundaries | Pass: walk stays in `client`; rules and patterns stay in `core`, passed down as a callback. No new dependency edge. |
| II. Local-first verification | Pass: covered by `make ci` (fmt, clippy, tests). |
| III. Tests without Put.io | Pass: walk tests use mockito; API tests the existing mocks. |
| IV. Download integrity | Pass: the feature only keeps more folders; `--dry-run` previews show kept folders. |
| V. Observable | Pass: kept folders logged with `folder_id`, `path`, `reason`; the finish line logs `kept_by_user`. No new metric (no need). |
| VI. Secrets | Not affected. |
| VII. Simplicity | Pass: one rule key, one settings list, no new abstraction beyond the callback the crate split requires. |

## Project Structure

### Documentation (this feature)

```text
specs/021-keep-folders/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/api.md
├── checklists/requirements.md
└── tasks.md
```

### Source Code (files touched)

```text
putio-sync-client/src/cleanup.rs        # KeepFn, Keep::User, KeptFolder, report fields
putio-sync-client/src/lib.rs            # re-exports
putio-sync-core/src/rules.rs            # FolderRule.keep, Effective.keep
putio-sync-core/src/config.rs           # CleanupConfig.keep_patterns + validation
putio-sync-core/src/cleanup.rs          # build KeepFn from rules + patterns
putio-sync-doctor/src/checks/putio.rs   # summary mentions kept_by_user
putio-sync-cli/src/commands/clean_empty.rs  # print kept list
putio-sync-api/src/routes/system/rules.rs     # RuleBody/EffectiveView keep; reconfigure cleanup
putio-sync-api/src/routes/system/settings.rs  # CleanupSettings.keep_patterns
putio-sync-api/examples/mock_ui.rs      # mock data for the UI check
putio-sync-api/ui/rules.js              # Keep flag + one-click button
putio-sync-api/ui/settings/putio.js     # Keep patterns field
putio-sync-api/ui/pages/putio.js        # preview names kept folders
config.example.toml                     # document keep and keep_patterns
```

**Structure Decision**: existing workspace layout; no new modules.

## Complexity Tracking

None.
