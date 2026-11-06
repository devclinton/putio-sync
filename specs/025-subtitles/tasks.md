# Tasks: Subtitles (025)

**Input**: `specs/025-subtitles/` (spec, plan, research, data-model, contracts, quickstart)
**Tests**: included (Constitution III; no real Put.io or Whisper).
**Format**: `- [ ] T### [P] [US#] description with file path`. [P] = different files, no dependency on an unfinished task.

## Phase 1: Setup

- [x] T001 Create crate `putio-sync-subtitles` (`Cargo.toml`, `src/lib.rs`) and add it to workspace members in `Cargo.toml`; deps per research R14 (`encoding_rs` from the lockfile version)
- [x] T002 [P] Add the new crate's `Cargo.toml` COPY, `src` mkdir and stub echo lines to `Dockerfile` (layer-cache stubs)
- [x] T003 [P] Add `ffmpeg` to the runtime stage `apk add` in `Dockerfile`

## Phase 2: Foundational (blocks all stories)

- [x] T004 [P] Write failing tests for `SubtitlesConfig` validation (language mapping/dedupe/cap, url, ranges, mask/unmask, `Debug` redaction) in `putio-sync-core/src/subtitles.rs`
- [x] T005 Implement `SubtitlesConfig`/`TranscriptionConfig`, `validate`, mask helpers in `putio-sync-core/src/subtitles.rs`; add `subtitles` field and validation call in `putio-sync-core/src/config.rs`
- [x] T006 [P] Document `[subtitles]` and `[subtitles.transcription]` in `config.example.toml`
- [x] T007 [P] Write failing table tests for the language mapper (639-1/2T/2B, names, `pt-br`, unknown) in `putio-sync-subtitles/src/lang.rs`
- [x] T008 Implement the language table and mapper in `putio-sync-subtitles/src/lang.rs`
- [x] T009 [P] Write failing tests for SRT/VTT parse, render, BOM/CRLF, encoding fallbacks, bad-output detection in `putio-sync-subtitles/src/srt.rs`
- [x] T010 Implement `srt.rs` (parse, render, `decode_bytes` with `encoding_rs`)
- [x] T011 [P] Write failing tests for naming, existing-sidecar detection and the no-overwrite `.part` write in `putio-sync-subtitles/src/naming.rs`
- [x] T012 Implement `naming.rs` (stem + lang + forced/sdh names, sidecar scan, atomic no-overwrite write)
- [x] T013 [P] Write failing tests for forced/SDH detection, source ranking, unknown-source mapping and selection order in `putio-sync-subtitles/src/candidates.rs`
- [x] T014 Implement `candidates.rs`
- [x] T015 [P] Write failing mockito tests for `list_subtitles` (languages CSV, 404/402/401 mapping, lenient fields) and `download_subtitle` (text body, `url` fallback) in `putio-sync-client/tests/subtitles.rs`
- [x] T016 Implement `list_subtitles`, `download_subtitle`, raw-text GET and models in `putio-sync-client/src/subtitles.rs` (+ `lib.rs` export)
- [x] T017 Define the `SubtitleSource` trait and the `PutioApiClient` impl in `putio-sync-subtitles/src/lib.rs`
- [x] T018 [P] Write failing migration test seeding old-schema `pipeline_steps` and `pipeline_attempts` rows (attempts must survive; idempotent re-run) in `putio-sync-db/tests/pipeline_migration.rs`
- [x] T019 Implement the `pipeline_steps` CHECK rebuild migration and new `CREATE` text (kinds `subtitle_scan`,`subtitles`; skip reason `no_video`) in `putio-sync-db/src/schema.rs` and `putio-sync-db/src/lib.rs`
- [x] T020 [P] Add `pipeline_deferred_deletes` table plus store queries (insert, list, delete) and `downloads.find_by_local_path` with tests in `putio-sync-db/src/schema.rs`, `putio-sync-db/src/lib.rs`
- [x] T021 Add `StepKind::{SubtitleScan,Subtitles}`, `SkipReason::NoVideo`, and storage names in `putio-sync-pipeline/src/model.rs`
- [x] T022 [P] Write failing tests for `store.insert_steps` (order, branch, idempotence) in `putio-sync-pipeline/tests/stages.rs`
- [x] T023 Implement `store.insert_steps` in `putio-sync-pipeline/src/store.rs`
- [x] T024 [P] Extend `KINDS` with `subtitle_scan`,`subtitles` in `putio-sync-telemetry/src/steps.rs` and add `subtitles_saved(source,language)` / `transcribed_seconds` counters in `putio-sync-telemetry/src/subtitles.rs` with tests
- [x] T025 Refactor `run_step` in `putio-sync-pipeline/src/runner.rs`: dispatch by kind, factor the shared attempt-finish/retry code, drop the unused `_hook` param of `failed()`; existing tests stay green

**Checkpoint**: foundation ready.

## Phase 3: User Story 1 - Subtitles from Put.io (P1) MVP

**Goal**: wanted-language subtitles from Put.io saved next to each video before import, delete waits.
**Independent test**: mock Put.io offers en and fr; en and es wanted; `movie.en.srt` saved, es reported missing, fr not saved.

### Tests for US1 (write first, must fail)

- [x] T026 [P] [US1] Engine test: en saved from Put.io, es missing, fr ignored, in `putio-sync-subtitles/tests/putio_pass.rs`
- [x] T027 [P] [US1] Engine tests: ranking folder/embedded over opensubtitles, failed candidate falls to next, existing `.en.srt` kept and not fetched, forced/sdh saved to own names, VTT and latin-1 converted, in `putio-sync-subtitles/tests/putio_pass.rs`
- [x] T028 [P] [US1] Pipeline tests: no languages gives stage skipped `not_configured`; excluded category gives `category`; season pack gives one step per video; samples and files under 100 MB skipped; zero videos gives `no_video`, in `putio-sync-pipeline/tests/subtitles.rs`
- [x] T029 [P] [US1] Pipeline tests: Subtitles step failure retries per `optional_retries` then item is `done_with_warnings`; gate keeps Radarr/Sonarr pending until the stage ends; staging mode writes inside the staging folder, in `putio-sync-pipeline/tests/subtitles.rs`
- [x] T030 [P] [US1] Delete-wait tests: gate defers, row persisted, not released before the stage ends, released after, released by the 24 h failsafe, restart keeps the row, `delete_remote` refuses a download not completed, in `putio-sync-pipeline/tests/deferred_delete.rs`
- [ ] T031 [P] [US1] Config/API tests: GET/PUT `/settings/subtitles` (validation, masked key, applies now), preview, secrets sweep covers the key, in `putio-sync-api/tests/settings.rs` and `putio-sync-api/tests/secrets_sweep.rs`

### Implementation for US1

- [x] T032 [US1] Implement `fetch_for_video` Put.io pass (existing sidecars, list, select, download, convert, write, outcome) in `putio-sync-subtitles/src/engine.rs`
- [x] T033 [US1] Plan the Subtitles stage (`SubtitleScan` step, `not_configured`/`category` skips via `applies_to`) in `putio-sync-pipeline/src/plan.rs`
- [x] T034 [US1] Implement scan (locate files, video/size/sample rules, `insert_steps`, `no_video`) and per-video step runner in `putio-sync-pipeline/src/subtitles.rs`; wire dispatch in `putio-sync-pipeline/src/runner.rs`
- [x] T035 [US1] Count subtitle-step failures as warnings in `finalize()`, add the new kinds to `refresh_gate()` in `putio-sync-pipeline/src/gate.rs`, make `Settings::staging_on` (Auto) true when subtitles are configured
- [x] T036 [US1] Inject `Arc<dyn SubtitleSource>` and the subtitles config into `Pipeline`; add `configure` support for live config in `putio-sync-pipeline/src/lib.rs`
- [x] T037 [US1] Add `RemoteDeleteGate` trait, `set_delete_gate`, deferral in `process_queue`, and `DownloadManager::delete_remote` (re-verifies completed row) in `putio-sync-download/src/manager.rs`
- [x] T038 [US1] Implement the gate in the pipeline plus `releasable_deletes()`/`mark_deleted` in `putio-sync-pipeline/src/deletes.rs`
- [x] T039 [US1] Wire the source, the gate and the per-tick release into `pipeline_tick_with` in `putio-sync-daemon/src/daemon.rs`
- [x] T040 [US1] Record metrics and structured logs (saved, candidate failed) in the engine; key never logged
- [ ] T041 [US1] Settings routes `GET/PUT /settings/subtitles` and `POST /settings/subtitles/preview`, `configure_subtitles`, group registration in `putio-sync-api/src/routes/system/subtitles.rs`, `settings.rs`, `assistant.rs`
- [ ] T042 [P] [US1] Subtitles settings card (languages, excluded categories) in `putio-sync-api/ui/settings/integrations.js`, register in `putio-sync-api/ui/settings/sections.js`; extend `putio-sync-api/tests/ui/settings_sections.test.mjs`
- [ ] T043 [P] [US1] Show subtitle step names, sources and "Waiting for subtitles" in the timeline UI; extend `putio-sync-api/tests/ui/timeline.test.mjs`

**Checkpoint**: US1 works alone against mocks.

## Phase 4: User Story 2 - Transcribe when Put.io has none (P2)

**Goal**: audio chunks to a Whisper server, joined SRT saved in the detected language.
**Independent test**: mock transcription server returns segments; a video with no Put.io subtitles saves `<video>.<lang>.srt`.

### Tests for US2 (write first, must fail)

- [ ] T044 [P] [US2] Audio tests with a fake `ffmpeg`/`ffprobe` shell script: discovery (config path, PATH, missing), command argv, chunk plan, temp cleanup on error, in `putio-sync-subtitles/tests/audio.rs`
- [ ] T045 [P] [US2] Transcribe tests with mockito: multipart fields, Bearer only when set, endpoint rule, verbose_json to SRT, SRT-body fallback, no-language error, empty speech, non-2xx scrubbed, timeout, in `putio-sync-subtitles/tests/transcribe.rs`
- [ ] T046 [P] [US2] Chunk-join tests: 3 chunks, shifted times, no gaps or overlaps, language reused from chunk 1, in `putio-sync-subtitles/tests/transcribe.rs`
- [ ] T047 [P] [US2] Engine tests: transcribe only when no wanted-language subtitle exists, saved under detected language even if unwanted, transcription off gives "none found", existing file never overwritten, in `putio-sync-subtitles/tests/transcription_pass.rs`
- [ ] T048 [P] [US2] Pipeline tests: server down gives retries then `done_with_warnings`; cancel kills ffmpeg; progress updates per chunk; global one-at-a-time, in `putio-sync-pipeline/tests/subtitles.rs`
- [ ] T049 [P] [US2] API tests for `POST /settings/subtitles/test` (ok, empty text ok, server error, unreachable, ffmpeg missing, masked key keeps stored key, key not in response) in `putio-sync-api/tests/subtitles_test_route.rs`

### Implementation for US2

- [ ] T050 [US2] Implement `audio.rs` (ffmpeg/ffprobe discovery, duration, chunk extraction with `kill_on_drop`, test-tone WAV generator)
- [ ] T051 [US2] Implement `transcribe.rs` (multipart client, parsing, mapper use, chunk join)
- [ ] T052 [US2] Add the transcription pass and `transcribed` outcomes to `putio-sync-subtitles/src/engine.rs`
- [ ] T053 [US2] Add global `Semaphore(1)`, `set_progress` per chunk and cancel wiring in `putio-sync-pipeline/src/subtitles.rs`; transcribed-seconds metric
- [ ] T054 [US2] Implement `POST /settings/subtitles/test` and the `ffmpeg` block of GET in `putio-sync-api/src/routes/system/subtitles.rs`
- [ ] T055 [P] [US2] Transcription section and Test button in the Subtitles card (`putio-sync-api/ui/settings/integrations.js`); extend `putio-sync-api/tests/ui/settings_sections.test.mjs`

**Checkpoint**: US1 and US2 work independently.

## Phase 5: User Story 3 - Per category and by hand (P3)

**Goal**: per-category off (planner side already in T033) and "Fetch subtitles" on Local files.
**Independent test**: POST a fetch for a video on disk, a `manual:` item runs its step and writes the subtitle.

### Tests for US3 (write first, must fail)

- [ ] T056 [P] [US3] Manual-item tests: `insert_item` source `manual`, one step, size/sample rules bypassed, duplicate gives 409 data, in `putio-sync-pipeline/tests/subtitles.rs`
- [ ] T057 [P] [US3] API tests: preflight (`available`/`gone`/`unknown`, not a video, impossible), POST 202/409/400/404, path guard, in `putio-sync-api/tests/local_subtitles.rs`
- [ ] T058 [P] [US3] UI logic test for the confirm-before-transcription prompt and disabled states in `putio-sync-api/tests/ui/files_subtitles.test.mjs`

### Implementation for US3

- [ ] T059 [US3] Add manual item registration (`Pipeline::start_manual_subtitles`) in `putio-sync-pipeline/src/subtitles.rs`, `lib.rs`
- [ ] T060 [US3] Implement preflight and POST in `putio-sync-api/src/routes/local/subtitles.rs`, register next to the 044 routes in `putio-sync-api/src/routes/local/files.rs`/`mod.rs`
- [ ] T061 [US3] "Fetch subtitles" row action on video rows with the preflight prompt in `putio-sync-api/ui/pages/files.js`
- [ ] T062 [P] [US3] Add a pointer to the new routes in `specs/044-local-files/contracts/api.md`

## Phase 6: Polish

- [ ] T063 [P] README section (settings, ffmpeg, Radarr/Sonarr "Import Extra Files", Bazarr note, delete-wait behavior and the 24 h failsafe) in `README.md`
- [ ] T064 [P] CHANGELOG entry under Unreleased in `CHANGELOG.md`; add the Subtitles step events to `EVENTS.md` if the 038 docs list them
- [ ] T065 [P] Mark roadmap item 025 and note deviations in `ROADMAP.md` and a "Deviations" section at the end of this file
- [ ] T066 Run `make ci` and fix findings
- [ ] T067 Measure the ffmpeg image size increase with `docker build`, record it in `specs/025-subtitles/research.md` (R2)
- [ ] T068 Manual run per `quickstart.md` against the real Put.io API; record real `source` strings, the download path and forced/SDH names in `specs/025-subtitles/contracts/putio-subtitles.md`, and fix the candidate matcher if they differ

## Dependencies and order

- Phase 1 then Phase 2 (T004..T025). Within Phase 2, each test task precedes its implementation; [P] tests can run together.
- T019 before T020 and T021; T021 before T022/T023/T025; T016 and T017 before T032.
- US1 (T026..T043) needs Phase 2. US2 (T044..T055) needs T032 (engine) and T025; the audio/transcribe parts (T044..T046, T050, T051) can start after Phase 2. US3 (T056..T062) needs T034 and T041 (config routes) and T020.
- Within a story: tests first (must fail), then implementation. T037 needs T020; T038 needs T037; T039 needs T036 and T038.
- Polish after the stories; T068 last.

## Parallel examples

- Phase 2 tests T004, T007, T009, T011, T013, T015, T018 in parallel.
- US1 tests T026..T031 in parallel; UI tasks T042, T043 in parallel after T041.
- US2 tests T044..T049 in parallel.

## Implementation strategy

MVP is Phase 1, Phase 2 and US1 (Put.io only, no ffmpeg). Ship it, then US2, then US3.

## Counts

Setup 3, Foundational 22, US1 18, US2 12, US3 7, Polish 6. Total 68.
