# Implementation Plan: Glob Filters with AND and OR

**Branch**: `020-glob-filters` (worktree branch `claude/elated-shtern-cb1522`) | **Date**: 2026-11-03 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/020-glob-filters/spec.md`

## Summary

`core` gains a `pattern` module: a hand-written tokenizer, parser and glob matcher for the
045 filter language plus this feature's three additions (comma as OR, `{a,b}`
alternatives, `/` patterns matched against path tails). `Pattern::parse` is lenient, like
the filter box; `pattern::check` is the strict form used by config validation (errors) and
the settings assistant (warnings). `ui/query.js` gets the same three additions, and both
test suites run one shared JSON table of cases
(`putio-sync-core/tests/data/filter-cases.json`).

`FolderRule` gains `include` and `exclude` (Only / Never download). `RuleSet` takes the
root values from `[download] whitelist_patterns` / `blacklist_patterns`, and `Effective`
carries both patterns (sourced like the other settings) and its folder path, so
`Effective::decide(name)` checks skip → exclude → include → type filter with no signature
change; every caller (sync, manual queue, dry run, release check, preview) picks the
patterns up for free. `Reason` gains `Excluded` and `NotIncluded`.

`Config::validate` and `setup` validation check every pattern strictly. The settings
assistant (`assistant::risks`) adds pattern warnings and the delete-under-include risk, so
the Settings banner and the doctor's Config check show them with no new check id.

API: `RuleBody`, `EffectiveView` and `GlobalView` gain the patterns; `POST /rules/preview`
gains `global` (candidate Only / Never download, allowed on the root) and `downloads` (the
first 50 files that would be downloaded). UI: the rule card and editor show and edit the
two patterns, `verdict()` uses `filterMatcher`, and the Settings Download group gets a
Preview button for the two account-wide fields.

## Technical Context

**Language/Version**: Rust stable (edition 2021); browser ES modules for the UI

**Primary Dependencies**: none new. The matcher is ~250 lines; `regex` would be a new
dependency for a language this small (Constitution VII), and the `glob` crate has a
different grammar (case-sensitive, character classes).

**Storage**: config TOML only (two new optional rule keys). No database change.

**Testing**: `cargo test` (core unit + shared-table integration test, API route tests with
wiremock Put.io); `node --test` over `putio-sync-api/tests/ui/` via `tests/ui_js.rs`.

**Target Platform**: Linux NAS daemon (Docker), desktop CLI

**Project Type**: Cargo workspace: daemon + HTTP API + web UI

**Performance Goals**: a decision stays O(pattern × name); patterns are compiled once per
folder (`Effective` is cached per folder by the callers).

**Constraints**: UI bundle cap 384 KB; setup mode answers 503 for every non-wizard route.

**Scale/Scope**: patterns ≤ 1000 characters; brace expansion capped at 64 alternatives per
term.

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Crate Boundaries | ✅ | Matcher and rule logic in `core` (shared by download, api, arr, doctor); API only maps bodies; UI only displays. |
| II. Local-First Verification | ✅ | `make ci` covers everything, including the node tests. |
| III. Tests Without Put.io | ✅ | Route tests use the existing wiremock Put.io; the shared table needs nothing. |
| IV. Download Integrity | ✅ | Patterns only refuse files. Deletion still goes through `delete_blocked` with its preview and confirm; a new risk warning flags delete under an include pattern. |
| V. Observable by Default | ✅ | Pattern refusals use the existing rule-decision counter ("blocked") and log/record the reason text. No new labels. |
| VI. Secrets Stay Secret | ✅ | No secrets involved. |
| VII. Simplicity | ✅ | No new dependency, no new check id, no new route; existing keys gain behaviour. |

Post-design re-check: unchanged, all ✅.

## Project Structure

### Documentation (this feature)

```text
specs/020-glob-filters/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── pattern-grammar.md
│   └── api.md
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/src/pattern.rs          # new: tokenizer, parser, matcher, check
putio-sync-core/src/rules.rs            # include/exclude, Effective patterns, Reason
putio-sync-core/src/config.rs           # validate whitelist/blacklist
putio-sync-core/src/setup.rs            # same validation for the wizard
putio-sync-core/src/assistant.rs        # pattern warnings and delete-under-include risk
putio-sync-core/tests/data/filter-cases.json   # shared cases
putio-sync-core/tests/filter_cases.rs   # runs the table against the Rust matcher
putio-sync-download/src/manager.rs      # preview: downloads sample
putio-sync-api/src/routes/system/rules.rs      # bodies, views, preview global
putio-sync-api/tests/rules.rs           # route tests
putio-sync-api/ui/query.js              # comma, braces, paths
putio-sync-api/tests/ui/query.test.mjs  # runs the shared table
putio-sync-api/ui/rules.js              # card, editor, verdict, settings preview
putio-sync-api/ui/pages/settings.js     # Download group help text + preview extra
README.md, config.example.toml, example-config.toml, CHANGELOG.md
```

**Structure Decision**: existing workspace layout; one new core module.

## Complexity Tracking

None.
