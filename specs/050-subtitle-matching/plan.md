# Implementation Plan: Subtitle Matching and Source Modes

**Branch**: `050-subtitle-matching` | **Date**: 2026-11-05 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/050-subtitle-matching/spec.md`. Extends 025 (`specs/025-subtitles/`).

## Summary

Add the unbuilt part of the 025 roadmap entry on top of the shipped Subtitles stage. (1) Four modes (Download
only, Download then Whisper, Whisper only, Both) resolved per language, category and globally, with `.whisper.srt`
for Both. (2) An OpenSubtitles REST client used when Put.io's list leaves a wanted language missing: search by file
fingerprint (hash computed locally), then by IMDb/TMDb ID with season and episode read from Radarr/Sonarr history,
then by a rule-based file-name parse; candidates are scored on release group, source and resolution, and anything
below a confidence threshold is not saved. The 025 engine, pipeline steps, naming, no-overwrite writes and
`[subtitles]` config are extended, not replaced; an unchanged config behaves as before. Details and sources:
[research.md](research.md).

## Technical Context

**Language/Version**: Rust stable, edition 2021, tokio, reqwest, sqlx + SQLite (no schema change), tracing
**Primary Dependencies**: existing workspace crates only. No new dependency (research R10).
**Storage**: none new. Settings in the TOML config. OpenSubtitles token and quota in memory. Arr lookup cache in memory.
**Testing**: `cargo test`; mockito for OpenSubtitles, Put.io, Whisper and the *arr history endpoint; a fake `MediaLookup`
and fake `SubtitleSource`; table tests for the release parser and the score; a known-answer test for the hash;
`putio-sync-api/tests/ui/*.test.mjs` for UI logic
**Target Platform**: Docker on a NAS; desktop installs (no ffmpeg: Whisper modes degrade as in 025)
**Project Type**: Cargo workspace, daemon + HTTP API + CLI
**Performance Goals**: OpenSubtitles levels add under 15 s per video (SC-005); searches paced to 4/s process-wide
**Constraints**: no real OpenSubtitles, Put.io, *arr or Whisper in tests; no overwrite of existing subtitle files;
key, login and password never logged; existing `[subtitles]` files load and save unchanged
**Scale/Scope**: a handful of videos per item (packs up to ~30); at most 3 searches per video

## Constitution Check

| Principle | Status | How |
|---|---|---|
| I. Crate boundaries | Pass | OpenSubtitles client, scoring, parser, hash and mode planning live in `putio-sync-subtitles`. The `*arr` history call lives in `putio-sync-arr`. `MediaLookup` and `MediaInfo` are in `core` because subtitles and arr both use them; arr does not depend on subtitles and subtitles does not depend on arr. The daemon injects the adapter. `api` only routes. |
| II. Local-first | Pass | No CI changes; `make ci` is the gate. |
| III. Tests without real services | Pass | Mock servers for OpenSubtitles (search, download, login, 406, 429), *arr history and Put.io; fake traits for the engine; synthetic files for the hash. Bug fixes red first. |
| IV. Download integrity | Pass | No change to downloads or deletes. Subtitle writes keep the `.part` and no-overwrite rename. Dry-run records nothing and calls no OpenSubtitles download. |
| V. Observable | Pass | New metrics with fixed label sets (R11); structured logs per level, score and skip reason. |
| VI. Secrets | Pass | OpenSubtitles key and password masked in the API, redacted in `Debug`, scrubbed from errors (the JWT too); env overrides; secrets sweep test extended; new routes behind existing API auth. |
| VII. Simplicity | Pass with notes | No new dependency (hand-written parser and hash, justified R5, R10). One new setting group. Quota state is in memory. Score weights are constants with one tunable threshold. A trait seam (`MediaLookup`) is needed for I and III. |

Post-design re-check: unchanged. The new contracts add no dependency and no upward crate dependency.

## Project Structure

### Documentation (this feature)

```text
specs/050-subtitle-matching/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── checklists/requirements.md
├── contracts/
│   ├── config.md              # [subtitles] additions
│   ├── opensubtitles-api.md   # request/response as used (fixtures)
│   ├── media-lookup.md        # MediaLookup trait + *arr history calls
│   └── api.md                 # Settings routes
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/src/subtitles.rs      # + SubtitleMode, ModePlan, mode_for, OpenSubtitlesConfig, validation, mask
putio-sync-core/src/media.rs          # NEW: MediaInfo, MediaLookup trait (+ BoxFut already in core? else local alias)
putio-sync-subtitles/src/
├── hash.rs                           # NEW: OpenSubtitles file hash
├── release.rs                        # NEW: file/release-name parser
├── score.rs                          # NEW: confidence score, threshold
├── opensubtitles.rs                  # NEW: REST client (search, download, login, quota, pacing)
├── matching.rs                       # NEW: the three levels, per-language selection
├── naming.rs                         # + SlotKind::Whisper, is_transcription, whisper token excluded from main
├── candidates.rs                     # + conversion of OpenSubtitles results to scored candidates
├── engine.rs                         # + ModePlan in VideoRequest, mode-aware trigger and naming, OS pass
└── lib.rs                            # + exports, SubtitleSearch trait (test seam)
putio-sync-arr/src/{arr_api.rs,media.rs}   # history_by_download; MediaLookup impl over arr_items
putio-sync-pipeline/src/subtitles.rs  # build ModePlan; pass MediaLookup + OpenSubtitles client; step messages
putio-sync-telemetry/src/subtitles.rs # new sources and counters
putio-sync-daemon/src/daemon.rs       # build the client and adapter; apply on settings change
putio-sync-api/src/routes/system/subtitles.rs   # fields + opensubtitles test route
putio-sync-api/ui/settings/subtitles.js, ui/…   # mode controls, OpenSubtitles section
config.example.toml, README.md, CHANGELOG.md
```

**Structure Decision**: No new crate; new modules in `putio-sync-subtitles` (it already owns naming, selection and
the engine), one small module each in `core` and `arr`.

## Complexity Tracking

| Item | Why needed | Simpler alternative rejected because |
|---|---|---|
| `MediaLookup` trait in `core` | Subtitles needs IDs held by arr; neither crate may depend on the other | Direct dependency breaks Principle I; passing IDs through the pipeline store would need a schema change |
| Hand-written release parser | The roadmap asks for rule-based, no model | `regex` or a release-parsing crate adds a dependency for a dozen rules |
| Local hash computation | Works after Put.io delete and for local files | Put.io's `opensubtitles_hash` needs an extra call and is absent after delete |
| In-memory quota state | Service reports remaining and reset per download | Persisting is more code for a restart edge |
