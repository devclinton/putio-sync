# Implementation Plan: Organise TV Shows on Put.io

**Branch**: `claude/022-stream-mode-dlna-4b51d8` (stacked on 022 and 028) | **Date**: 2026-11-10 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/023-organise-tv-putio/spec.md`

## Summary

A new arrived step (022's seam), `organise`, plans a move for each newly streamed TV episode:
`<TV folder>/<Show>/Season NN/<Show> - SNNEEE - Title.ext`, read from the release name with
the 050 reader (moved to `core` so `download` can use it). Plans are stored in a new
`organise_moves` table. In "preview" mode they wait; the operator applies, undoes or dismisses
them from an Organise panel on the Put.io page. "Move at once" applies in the same pass. 028's
folder and rename hooks, when a plugin answers, replace the built-in target. The kind of each
move (`tv` now, `movie` in 024) is a column and a branch in one naming function, so 024 adds a
reader and a layout only.

## Technical Context

**Language/Version**: Rust stable, edition 2021; web UI in plain ES modules (no build step)

**Primary Dependencies**: tokio, reqwest, sqlx (SQLite), axum, serde, tracing; no new crates

**Storage**: SQLite, new `organise_moves` table; config in TOML, new `[organise]` section

**Testing**: `cargo test` with mockito for Put.io (Principle III), node tests in `tests/ui`

**Target Platform**: long-running daemon on a NAS; `server` engine saves settings only

**Project Type**: Cargo workspace (core, client, db, download, api, cli, telemetry …)

**Performance Goals**: at most 20 arrived steps per pass (022's limit); applying a selection runs
one file at a time, with a folder-id cache per batch

**Constraints**: eager UI under 488 KiB (288 bytes left before this feature, see research R6);
no Put.io in tests; preview mode makes no Put.io write until the operator applies

**Scale/Scope**: a household library: thousands of streamed files, tens of moves per day

## Constitution Check

| Principle | How this plan meets it |
|---|---|
| I. Crate boundaries | Naming (pure) and `[organise]` in `core`; Put.io calls in `client`; table in `db`; the organiser service and step in `download` (it owns the arrived steps); routes in `api`; wiring in `cli`. The release reader moves from `subtitles` to `core` because two crates now use it (subtitles re-exports it). |
| II. Local-first | Unit, integration and node tests run under `make ci`. |
| III. Tests without Put.io | Organiser tests use mockito for `files/{id}`, `files/list`, `files/create-folder`, `files/rename`, `files/move`. |
| IV. Download integrity / dry run | Moves are renames on Put.io, never deletes. "Preview" (the default) is the dry run; nothing moves until applied. Undo restores each move. |
| V. Observable | `putio_sync_organise_moves_total{kind,result}` (bounded labels); structured logs with file id and both paths. |
| VI. Secrets | No new secrets. Routes sit behind the API token like the rest. |
| VII. Simplicity | Four settings, one table, one step. No online metadata lookup. |

Post-design re-check: no violations.

## Project Structure

### Documentation (this feature)

```text
specs/023-organise-tv-putio/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── api.md
│   ├── config.md
│   └── naming.md
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/src/release.rs        # moved from subtitles; + Episode reader (title case kept)
putio-sync-core/src/organise.rs       # OrganiseConfig, OrganiseKind, Target, plan_target()
putio-sync-core/src/config.rs         # Config.organise, validation
putio-sync-subtitles/src/lib.rs       # `pub use putio_sync_core::release;`
putio-sync-client/src/files.rs        # rename_file, move_files
putio-sync-db/src/organise_moves.rs   # OrganiseMovesRepository
putio-sync-db/src/schema.rs, lib.rs   # table + repo accessor
putio-sync-telemetry/src/…            # organise_moves_total
putio-sync-download/src/organise.rs   # Organiser service + OrganiseStep (ArrivedStep)
putio-sync-download/tests/organise.rs # mockito tests: plan, apply, undo, auto, hooks, refusals
putio-sync-api/src/routes/system/organise.rs  # /organise and /settings/organise
putio-sync-api/ui/organise.js         # panel: settings + moves; loaded on demand
putio-sync-api/ui/pages/putio.js      # "Organise" button that imports it
putio-sync-cli/src/commands/daemon.rs # build the Organiser, register the step, attach to API
config.example.toml, README.md, PLUGINS.md, CHANGELOG.md, ROADMAP.md
```

**Structure Decision**: the organiser follows the media server's pattern: one service built by
the daemon, shared with the API through `AppState::with_organiser`, configured on settings save.

## Complexity Tracking

| Item | Why needed | Simpler alternative rejected because |
|---|---|---|
| Panel loaded with `import()` | 288 bytes of eager UI budget left | Raising the cap was ruled out by the brief; trimming unrelated pages is out of scope |
