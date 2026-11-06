# Implementation Plan: Subtitle Translation

**Branch**: `026-subtitle-translation` | **Date**: 2026-11-05 | **Spec**: [spec.md](spec.md)

**Input**: `specs/026-subtitle-translation/spec.md`. Builds on 038 (processing pipeline, implemented) and 025
(subtitles, designed in `specs/025-subtitles/`, being implemented on another branch).

## Summary

Translate a video's subtitle into each wanted language that has none, as the built-in Translating stage of the 038
pipeline. A new crate `putio-sync-translation` owns the work: the prompt and reply format, chunking with retry and
split, the OpenAI-compatible chat client, the outbound callback request, token handling and timing re-alignment. It
reads and writes SRT through 025's `putio-sync-subtitles` crate (parser, renderer, language mapper, sidecar naming,
no-overwrite write) and takes its target languages from 025's `[subtitles] languages`; nothing is duplicated.

The pipeline gets two step kinds, `translate_scan` and `translate`. The scan step expands at run time to one
`translate` step per (video, missing language), with the language as the **branch**, so languages run in parallel and
videos of one language run in order, the same expansion 025 uses for `subtitles`. A chat translation runs inside the
step. A callback translation POSTs the job and, on `202`, **parks**: the step stays `running`, the attempt stays open,
the item's task ends (no permit held), and a row in the new table `translation_callbacks` carries the job across
restarts. The unauthenticated route `POST /translate/callback/{token}` completes the step; the pipeline tick expires
late jobs. Settings get a Translation card with a Test button. The Local files page gets a "Translate subtitles"
action that registers a `manual:` pipeline item. Evidence and decisions: [research.md](research.md).

## Technical Context

**Language/Version**: Rust stable, edition 2021, tokio, reqwest, sqlx + SQLite, tracing

**Primary Dependencies**: existing workspace crates and crates already in the lockfile (`reqwest`, `serde_json`,
`sha2`, `uuid` v4). **No new dependency.** Depends on 025's `putio-sync-subtitles`.

**Storage**: SQLite: new `translation_callbacks` and `subtitle_provenance` tables; `pipeline_steps` CHECK rebuilt a
second time for two more kinds. Subtitle files are written next to the video (or in the 038 staging folder).

**Testing**: `cargo test`; a mockito chat server (echo with prefix, bad replies, 429, 404, slow), a mockito callback
service, fake clocks where expiry matters, `tempfile` dirs, UI logic tests in `putio-sync-api/tests/ui/*.test.mjs`

**Target Platform**: the NAS container and desktop installs; Ollama, LM Studio or Open WebUI on the LAN, or a hosted API

**Project Type**: Cargo workspace, daemon + HTTP API + CLI

**Performance Goals**: a 1,000-cue subtitle in under 10 minutes on a local 14B model (a guide, SC-002); Test answers
within 15 seconds (SC-003)

**Constraints**: no real LLM or callback service in tests; cue count and timestamps identical to the source (SC-001);
API key, header values and callback tokens never logged or returned

**Scale/Scope**: a handful of videos per item, up to 10 languages, 1,000 to 3,000 cues per subtitle, `max_in_flight`
2 requests at once

## Constitution Check

| Principle | Status | How |
|---|---|---|
| I. Crate boundaries | Pass | New `putio-sync-translation` owns the concern; it depends on `subtitles` and `core`, never on `pipeline`, `daemon` or `api`. `pipeline` depends on it and owns scheduling, parking and the callback table queries. Config structs in `core`. `api` only routes. 025's crate is reused, not extended with HTTP clients. |
| II. Local-first | Pass | No new CI. `make ci` must pass; nothing needs Ollama or network. |
| III. Tests without real services | Pass | mockito for the chat server and the callback service; a fake translator trait seam for pipeline tests; synthetic SRT; no real LLM. |
| IV. Download integrity | Pass | Files are written with 025's `.part` + no-overwrite rename; a result that fails the cue/timing check is never written (FR-004, an invariant checked again just before the write). No Put.io deletes are involved, so 025's deferred-delete path is untouched. |
| V. Observable | Pass | Metrics and structured logs in research R12; pipeline step metrics get the new kinds. |
| VI. Secrets | Pass | `api_key` and header values masked in the API, `Debug` redacted, scrubbed from errors. Callback tokens: random, only the SHA-256 is stored, never logged, redacted from the HTTP trace span and from the log redactor (R7). The one unauthenticated route is justified in research R6 against the constitution's wording ("only health and metrics MAY be unauthenticated"): see Complexity Tracking. |
| VII. Simplicity | Pass with notes | No new dependency (`sha2`, `uuid` already locked). One crate, justified R15. Knobs each answer a spec requirement (FR-010, FR-012, FR-013, FR-016). |

Post-design re-check: passes; the callback route is the only item recorded below.

## Project Structure

### Documentation (this feature)

```text
specs/026-subtitle-translation/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── config.md       # [translation] TOML
│   ├── api.md          # Settings routes, Test, Local files routes
│   ├── chat.md         # OpenAI-compatible request/reply and the line format
│   └── callback.md     # outbound request, accepted replies, inbound route
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-translation/                 # NEW crate
├── Cargo.toml
└── src/
    ├── lib.rs                          # Translator (settings, in-flight limit), TranslateError, traits
    ├── prompt.rs                       # system/user prompt, cue <-> line encoding, reply parse, tag restore
    ├── chunk.rs                        # chunk plan, validate, retry and split driver
    ├── chat.rs                         # /chat/completions client, error classes, 429 handling, think-strip
    ├── callback.rs                     # token make/hash/url, outbound POST, reply classes, inbound body parse
    ├── align.rs                        # re-align timings to the source, final timing invariant
    ├── source.rs                       # source subtitle pick (FR-003) from sidecars + provenance
    └── engine.rs                       # translate_with_chat, submit_callback, Test helpers
putio-sync-core/src/translation.rs      # TranslationConfig, validate, mask/unmask
putio-sync-core/src/log_redact.rs       # rule for /translate/callback/<token>
putio-sync-db/src/{schema.rs,lib.rs}    # translation_callbacks, subtitle_provenance, CHECK rebuild #2
putio-sync-pipeline/src/                # model.rs (kinds), plan.rs, runner.rs (dispatch, park, outcome),
                                        #   translate.rs (scan, step runner, accept/expire callbacks),
                                        #   store.rs (callback + provenance queries, interrupt_running), gate.rs
putio-sync-telemetry/src/               # steps.rs KINDS, translation.rs metrics
putio-sync-daemon/src/daemon.rs         # build Translator, expire callbacks each pipeline tick
putio-sync-api/src/routes/system/translation.rs   # settings routes + test
putio-sync-api/src/routes/translate_callback.rs   # public callback route
putio-sync-api/src/routes/local/translate.rs      # preflight + translate
putio-sync-api/src/lib.rs               # route registration, trace span without the callback token
putio-sync-api/ui/                      # settings card, files.js action, timeline "waiting for callback"
Dockerfile, Cargo.toml, config.example.toml, README.md, CHANGELOG.md
```

**Structure Decision**: one new crate plus additions to existing ones, as listed.

## Complexity Tracking

| Item | Why needed | Simpler alternative rejected because |
|---|---|---|
| New crate | Chat client, prompt/validation and callback client are a separate concern; 025's crate is in flight on another branch | Adding to `putio-sync-subtitles` couples the two branches and puts two more HTTP clients in a codec crate; in `pipeline` it mixes HTTP into a scheduler |
| Unauthenticated `/translate/callback/{token}` (allowed by constitution 1.1.0, Principle VI) | FR-015: the service cannot hold the API token | Requiring the API token would put the operator's master credential in a third-party service. Mitigation: 256-bit random single-use token, hash only stored, expiry, rate limit on failures, constant refusal body |
| `translation_callbacks` table | FR-017: jobs survive restarts | In-memory map loses the job and the file (the service has already done the work) |
| Second `pipeline_steps` CHECK rebuild | New kinds `translate_scan`, `translate` | Free-text kinds lose validation (same reasoning as 025 R13) |
| `subtitle_provenance` table | FR-003 needs to know which sidecar was transcribed, and a machine translation must not become the source for another language; 025's design does not record provenance | Guessing from the language list is wrong when wanted languages overlap with what was transcribed |
| Parking a step (no new step state) | A callback can take an hour; holding a task and an item permit that long starves the pipeline | A new `waiting_callback` step state needs a third CHECK rebuild and every state match in 038 gets a new arm |
