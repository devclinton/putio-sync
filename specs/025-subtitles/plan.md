# Implementation Plan: Subtitles

**Branch**: `025-subtitles` | **Date**: 2026-11-05 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/025-subtitles/spec.md`. Builds on 038 (processing pipeline).

## Summary

Fetch subtitles for each downloaded video as the built-in Subtitles stage of the 038 pipeline. For each video, ask
Put.io for its subtitles in the wanted languages, save the best candidate per language as `<video>.<lang>.srt`
(folder or embedded first, then OpenSubtitles), never overwrite an existing file, and, when no wanted-language
subtitle exists and transcription is on, extract audio with ffmpeg, send it in chunks to an OpenAI-style Whisper
server, and save the result in the detected language. A new crate `putio-sync-subtitles` owns the logic; the pipeline
gets two step kinds (`subtitle_scan`, `subtitles`) that expand to one step per video at run time. Post-download
delete waits for the stage through a persisted deferred-delete set. Settings get a Subtitles card with a Test
button. The Local files page gets a "Fetch subtitles" action that registers a `manual:` pipeline item.
Details and evidence: [research.md](research.md).

## Technical Context

**Language/Version**: Rust stable, edition 2021, tokio, reqwest (multipart already enabled), sqlx + SQLite, tracing
**Primary Dependencies**: existing workspace crates; one new direct dependency, `encoding_rs` (already in
`Cargo.lock` via reqwest), see research R6/R14. System tool: `ffmpeg`/`ffprobe` (Docker image; optional on desktop).
**Storage**: SQLite: rebuilt `pipeline_steps` CHECKs, new `pipeline_deferred_deletes`, new `downloads` query.
Subtitle files are written next to the video.
**Testing**: `cargo test`; mockito for Put.io and the Whisper server, fake `SubtitleSource` and fake ffmpeg shell
script for the engine, `tempfile` dirs; UI logic tests as `putio-sync-api/tests/ui/*.test.mjs`
**Target Platform**: Docker on a NAS (ffmpeg included); desktop installs work without ffmpeg (no transcription)
**Project Type**: Cargo workspace, daemon + HTTP API + CLI
**Performance Goals**: Put.io fetch adds under 10 s per video (SC-003); transcription is background work
**Constraints**: no real Put.io or Whisper in tests; no overwrite of existing subtitle files; API key never logged
**Scale/Scope**: a handful of videos per item (season packs up to ~30); transcription serialised globally

## Constitution Check

| Principle | Status | How |
|---|---|---|
| I. Crate boundaries | Pass | New `putio-sync-subtitles` owns the concern. Put.io HTTP stays in `client`. Pipeline depends on subtitles, not the reverse. Config structs in `core`. `api` only routes. Delete gate trait lives in `download`, implemented in `pipeline`. |
| II. Local-first | Pass | No new CI. `make ci` must pass; ffmpeg is not needed by tests (a fake script). |
| III. Tests without Put.io | Pass | mockito for Put.io and the Whisper server; fake `SubtitleSource`; a fake `ffmpeg` script; synthetic WAV. Migration test seeds old-schema rows. Bug fixes red first. |
| IV. Download integrity | Pass | Deferred delete re-checks the `downloads` row is `completed` with a local path before deleting. Subtitle writes use a `.part` file and a no-overwrite rename. Dry-run records no deferrals. |
| V. Observable | Pass | New metrics and structured logs (research R11); pipeline step metrics include the new kinds. |
| VI. Secrets | Pass | `api_key` masked in API, redacted `Debug`, scrubbed from errors and logs; new routes sit behind the existing API auth; secrets sweep test extended. |
| VII. Simplicity | Pass with notes | One new dependency (justified R6, already locked). No knobs for size or sample rules. Language table instead of a crate. One crate, justified R14. ffmpeg system dependency justified R2. |

## Project Structure

### Documentation (this feature)

```text
specs/025-subtitles/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── config.md              # [subtitles] TOML
│   ├── api.md                 # Settings routes and Local files routes
│   ├── putio-subtitles.md     # Put.io list/download as used
│   └── transcription.md       # OpenAI-style request/response as used
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-subtitles/                 # NEW crate
├── Cargo.toml
└── src/
    ├── lib.rs                        # re-exports, SubtitleSource trait
    ├── lang.rs                       # code table and mapper
    ├── srt.rs                        # parse SRT/VTT, render SRT, decode bytes
    ├── naming.rs                     # sidecar names, existing-sidecar detection, atomic no-overwrite write
    ├── candidates.rs                 # forced/SDH detection, source rank, selection
    ├── audio.rs                      # ffmpeg/ffprobe discovery, chunk extraction, test tone WAV
    ├── transcribe.rs                 # multipart client, verbose_json/SRT parse, chunk join
    └── engine.rs                     # fetch_for_video (Put.io pass, transcription pass, results)
putio-sync-core/src/subtitles.rs      # SubtitlesConfig, TranscriptionConfig, validate, mask
putio-sync-client/src/subtitles.rs    # list_subtitles, download_subtitle, models
putio-sync-db/src/{schema.rs,lib.rs}  # CHECK rebuild migration, deferred deletes, find_by_local_path
putio-sync-download/src/              # RemoteDeleteGate trait, process_queue deferral, delete_remote
putio-sync-pipeline/src/              # model.rs (kinds), plan.rs, runner.rs, store.rs (insert_steps),
                                      #   gate.rs, subtitles.rs (step runner), deletes.rs (releasable)
putio-sync-telemetry/src/             # steps.rs KINDS, subtitles.rs metrics
putio-sync-daemon/src/daemon.rs       # inject source, release deferred deletes each tick
putio-sync-api/src/routes/system/subtitles.rs     # settings routes + test
putio-sync-api/src/routes/local/subtitles.rs      # preflight + fetch
putio-sync-api/ui/                    # settings card, files.js action, assistant list
Dockerfile, Cargo.toml, config.example.toml, README.md, CHANGELOG.md
```

**Structure Decision**: One new crate plus additions to existing ones, per the file list above.

## Complexity Tracking

| Item | Why needed | Simpler alternative rejected because |
|---|---|---|
| New crate | SRT, language, ffmpeg and Whisper client are a separate concern from scheduling | In `pipeline` it mixes codecs and HTTP into a scheduler; in `client` it is not Put.io |
| `encoding_rs` direct dependency | Non-UTF-8 subtitle files are common | Hand-written codepage tables for ~10 encodings (more code than the dependency) |
| ffmpeg in the image | Audio extraction from any container/codec | Sending the file (size/format limits), `symphonia` (no AC-3/DTS) |
| Table rebuild migration | CHECK constraints reject the new kinds | Free-text kinds lose validation |
| Persisted deferred deletes | Restart must not lose or skip a delete | In-memory set (loses deletes or subtitles on restart) |
| Run-time step insertion | Video count is unknown at plan time | One step per item cannot show one line per video (acceptance 1.5) |
