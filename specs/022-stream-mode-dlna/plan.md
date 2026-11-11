# Implementation Plan: Stream Mode with DLNA

**Branch**: `022-stream-mode-dlna` | **Date**: 2026-11-10 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/022-stream-mode-dlna/spec.md`

## Summary

Stream mode is a new folder setting, `mode = "download" | "stream"`, on `[download]` and on
each `[[download.rules]]` entry, resolved like the other rule settings. The rule decision gains
a `Stream` outcome (after skip, patterns and the type filter), so the sync pass keeps those
files on Put.io: not queued, not deleted. Each streamed file is recorded once in a new
`stream_files` table, and its "arrived" steps run from there, a bounded number per pass. The
only step in 022 asks Put.io for an MP4 version of non-MP4 videos (`download.stream_mp4`,
default on). Steps are a trait list on the download manager: 023 and 024 add their organisers
there. The media server is reused as it is; its Put.io root now defaults to the synced folder
(the account root) when `dlna.putio_folder` is unset. Settings › Downloads gets the mode, a
warning with a one-click fix when the media server can't show streamed files, and the rules
editor gets a mode per rule.

## Technical Context

**Language/Version**: Rust 1.96 (edition 2021), workspace toolchain; the web UI is plain ES
modules with no build step.

**Primary Dependencies**: Existing only: `tokio`, `sqlx` (SQLite), `reqwest` (Put.io client),
`axum` (API), `futures-util` (`BoxFuture` for the step trait). No new crates.

**Storage**: One new SQLite table, `stream_files`, created with `CREATE TABLE IF NOT EXISTS`
like every other table. Settings live in `[download]` (`mode`, `stream_mp4`) and
`[[download.rules]] mode`.

**Testing**: `cargo test` (unit tests in `core::rules`, `core::config`, `core::config_lint`,
`db`, `client`; integration tests in `putio-sync-download/tests` with `mockito` standing in for
Put.io), and the UI's `node --test` suites under `putio-sync-api/tests/ui` (run by
`tests/ui_js.rs`).

**Target Platform**: The Linux NAS in Docker and desktop installs.

**Project Type**: Daemon + HTTP API + web UI, in a Cargo workspace.

**Performance Goals**: A pass over 5,000 already-seen stream files makes no extra Put.io calls
(SC-005): one `SELECT file_id` per pass and set lookups.

**Constraints**: At most 20 arrived-step runs per pass (FR-012). No file in stream mode is ever
written to the download folder (SC-001).

**Scale/Scope**: Libraries of thousands of files on Put.io; one daemon.

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | The mode is config + rules (`core`). The `Stream` decision is acted on in the sync pass (`download`), which owns queueing. The table and its repository are in `db`. The MP4 request is a client method (`client`). The DLNA default is one line in `dlna`. API and UI only expose it. No new edges. |
| II. Local-first | All new tests run under `make ci`; no network. |
| III. Tests without Put.io | MP4 status/request and the sync pass are tested against `mockito`. |
| IV. Download integrity | Stream mode only stops files being queued. Nothing already queued or on disk is touched. Storage free-up only deletes files with a completed download, so streamed files are safe (FR-008, verified by a test). |
| V. Observable | `putio_sync_rule_decisions_total{decision="streamed"}`, `putio_sync_stream_mp4_requests_total{result}`. Each new stream file and each step outcome is logged with `file_id`, `step` fields. Sync pass summary includes the streamed count. |
| VI. Secrets | Nothing new is exposed. The existing media server deviation (Put.io data on an unauthenticated LAN port) is unchanged and documented in `specs/022-media-server/plan.md`; this feature only changes which folder it defaults to, and still only when the operator shares the Put.io root. |
| VII. Simplicity | Two config keys plus one rule key. One table. One trait with one implementation. No scheduler: steps run inside the existing sync pass. |

Post-design re-check: passes.

## Project Structure

### Documentation (this feature)

```text
specs/022-stream-mode-dlna/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── config.md        # [download] mode, stream_mp4, rules mode, dlna default
│   ├── api.md           # rule/preview/settings JSON changes
│   └── arrived-steps.md # the seam for 023/024
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/src/config.rs        # SyncMode, DownloadConfig.mode/stream_mp4, DlnaConfig::putio_root, validation
putio-sync-core/src/rules.rs         # FolderRule.mode, Effective.mode, Decision::Stream, uses_stream()
putio-sync-core/src/config_lint.rs   # stream mode without the media server's Put.io root; arr category in stream mode
putio-sync-client/src/files.rs       # request_mp4(id): POST /files/{id}/mp4
putio-sync-db/src/schema.rs          # stream_files table
putio-sync-db/src/stream_files.rs    # StreamFilesRepository: ids, add, pending, mark, prune
putio-sync-telemetry/src/{labels,sync}.rs  # Streamed label, mp4 request counter
putio-sync-download/src/arrived.rs   # ArrivedFile, ArrivedStep trait, Mp4Step, run_pending
putio-sync-download/src/manager.rs   # Decision::Stream in the pass, honour_stream flag, TreeDecision::Streamed, stats
putio-sync-download/src/dry_run.rs   # Stream counted as not downloaded
putio-sync-dlna/src/lib.rs           # Put.io root defaults to the account root
putio-sync-api/src/routes/system/rules.rs  # mode in rule and effective JSON, preview count
putio-sync-api/ui/settings/downloads.js    # Mode select + media server warning
putio-sync-api/ui/stream_mode.js     # pure helpers: is stream used, warning text/fix (tested)
putio-sync-api/ui/rules.js           # per-rule mode
putio-sync-api/ui/i18n/en.json       # make i18n
putio-sync-api/tests/ui/stream_mode.test.mjs
README.md (media-server and rules sections), config.example.toml, CHANGELOG.md, ROADMAP.md
```

**Structure Decision**: No new crate. The arrived-step runner lives in `download` because it
runs inside the sync pass and needs the client and the database the download manager already
holds.

## Complexity Tracking

No violations.

The UI size cap in putio-sync-api/tests/ui_assets.rs rises from 484 to 488 KiB for the Stream
card, the rule editor's mode select and `ui/stream_mode.js` (about 4 KB).
