# Implementation Plan: Organise Movies on Put.io

**Branch**: `claude/022-stream-mode-dlna-4b51d8` (no branch of its own) | **Date**: 2026-11-10 | **Spec**: [spec.md](spec.md)

## Summary

Add movies as a second organise kind. Core gets a `Movie` release reader (title, year,
part, extra/sample words) beside 023's `Episode`, a `movie_target` beside `tv_target`, and
`movies`/`movies_folder` settings. The download crate's organiser, the `organise_moves` table,
the API and the Organise panel are already kind-agnostic: they get the new kind, the new
settings fields and a kind column.

## Technical Context

**Language/Version**: Rust (workspace edition), plain ES modules for the UI
**Primary Dependencies**: none new
**Storage**: SQLite `organise_moves` (kind is already a text column; no migration)
**Testing**: `cargo test` (core unit tests, download and API integration tests against the mock Put.io), `node --test` for UI helpers
**Target Platform**: the daemon on Linux/macOS/Docker
**Project Type**: workspace of crates + static web UI
**Constraints**: eager UI under 488 KiB (the panel is lazy-loaded); no regex crate in core

## Constitution Check

- I. Crate boundaries: naming stays pure in `putio-sync-core`; Put.io calls stay in the download crate. ✅
- II/III. Local-first, no Put.io in tests: all tests use the mock client. ✅
- IV. Download integrity: moves are refused outside stream mode (023 check, reused). ✅
- V. Observable: `putio_sync_organise_moves_total{kind="movie"}`. ✅
- VI. Secrets: none touched. ✅
- VII. Simplicity: one new reader and one target function; samples are left alone rather than deleted. ✅

## Project Structure

```text
putio-sync-core/src/release.rs        # Movie reader
putio-sync-core/src/organise.rs       # OrganiseKind::Movie, movie_target, settings
putio-sync-core/src/config_lint.rs    # doctor warning for organise.movies
putio-sync-telemetry/src/labels.rs    # OrganiseMoveKind::Movie
putio-sync-download/src/organise.rs   # kind mapping (no new paths)
putio-sync-api/src/routes/system/organise.rs  # live fields
putio-sync-api/ui/organise.js, organise_text.js  # toggle, folder, kind column
putio-sync-download/tests/organise.rs, putio-sync-api/tests/organise.rs
config.example.toml, README.md, CHANGELOG.md, ROADMAP.md, en.json
```

## Complexity Tracking

None.
