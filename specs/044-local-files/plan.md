# Implementation Plan: Local Files Page

**Branch**: `044-local-files` | **Date**: 2026-11-01 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/044-local-files/spec.md`

## Summary

A "Local files" page that browses the download folder and the enabled watch folders and can
delete, rename, and re-queue `.rejected` watch files. Path checking and the file operations live
in `putio-sync-file-manager` (new `local_files` module); the API replaces the old unchecked
`/local/files` routes with root-keyed ones; the UI gets one new page module. Every path is
resolved against the canonical root with no `..`, and parent folders are canonicalized so a
symlink cannot lead out ([research.md](research.md) R1).

## Technical Context

**Language/Version**: Rust stable (edition 2021); the UI is plain ES modules, no build step

**Primary Dependencies**: existing only: axum, tokio, tracing, chrono (api), sqlx (db). No new
crates.

**Storage**: SQLite `processed_torrents` (one new delete-by-checksum query); files on disk

**Testing**: `cargo test -p putio-sync-api` (router driven with `oneshot`, temp dirs as roots),
unit tests in `putio-sync-file-manager` for path resolution

**Target Platform**: Linux container on Unraid; macOS for development

**Project Type**: workspace of crates; daemon with HTTP API and embedded web UI

**Performance Goals**: a listing of a folder with 2,000 entries answers in well under a second

**Constraints**: UI assets stay under the 260 KB budget in `tests/ui_assets.rs` (about 33 KB
left before this feature); listings are capped at 2,000 entries

**Scale/Scope**: one operator; roots hold hundreds of entries at most

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | Path checks and file operations in `file-manager` (owns the local filesystem); the processed-record delete in `db`; the in-use paths query in `download`; `api` only maps config to roots and HTTP to calls. No upward dependency: `file-manager` already depends on `db`. |
| II. Local-first verification | Covered by `cargo test`, clippy and `make ci`. |
| III. Tests without Put.io | All tests use temp folders; Put.io is never called. |
| IV. Download integrity | Delete refuses partials a worker is writing (FR-005), so a live download is never cut. |
| V. Observable | Every action logs `root`, `path`, and outcome as structured fields. No metric: these are rare manual actions, and a counter would not help diagnose anything the log does not. |
| VI. Secrets | Routes sit behind the existing auth middleware; responses hold paths and sizes only. |
| VII. Simplicity | No new dependency or config knob. Old routes are removed, not kept beside the new ones. |

Post-design re-check: passes; nothing in Complexity Tracking.

## Project Structure

### Documentation (this feature)

```text
specs/044-local-files/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/api.md
└── tasks.md
```

### Source Code

```text
putio-sync-file-manager/src/
├── local_files.rs        # new: resolve(), list(), summary(), delete(), rename(), requeue()
├── watcher.rs            # + TorrentWatcher::forget(path)
└── lib.rs                # export local_files
putio-sync-db/src/torrents.rs        # + forget(checksum)
putio-sync-download/src/manager.rs   # + partial_paths_in_use()
putio-sync-api/src/routes/local/files.rs   # rewritten: roots, list, summary, delete, rename, requeue
putio-sync-api/src/ui.rs             # + pages/files.js
putio-sync-api/ui/pages/files.js     # new page
putio-sync-api/ui/{app.js,index.html,app.css}  # nav entry, route, a few styles
putio-sync-api/tests/local_files.rs  # new API tests
putio-sync-api/tests/{ui_assets.rs,secrets_sweep.rs}  # page list, new route paths
```

**Structure Decision**: follows the existing crate split; the page is one module like the others.

## Complexity Tracking

None.
