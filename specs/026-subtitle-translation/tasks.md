# Tasks: Subtitle Translation (026)

**Input**: `specs/026-subtitle-translation/` (spec, plan, research, data-model, contracts, quickstart)
**Base**: 038 (implemented) and 025 (designed in `specs/025-subtitles/`, implemented on another branch). Start this work
from a branch that has 025 merged; tasks that touch 025's code say so (gaps G1 to G3 in research R1).
**Tests**: included (Constitution III; no real LLM, callback service or network).
**Format**: `- [ ] T### [P] [US#] description with file path`. [P] = different files, no dependency on an unfinished task.

## Phase 1: Setup

- [x] T001 Create crate `putio-sync-translation` (`Cargo.toml`, `src/lib.rs`) and add it to workspace members in `Cargo.toml`; deps per research R15 (`putio-sync-core`, `putio-sync-subtitles`, `putio-sync-telemetry`, `tokio`, `reqwest`, `serde`, `serde_json`, `tracing`, `thiserror`, `sha2`, `uuid` v4 from the lockfile; dev `mockito`, `tempfile`)
- [x] T002 [P] Add the new crate's `Cargo.toml` COPY, `src` mkdir and stub echo lines to `Dockerfile` (layer-cache stubs)

## Phase 2: Foundational (blocks all stories)

- [x] T003 [P] Write failing tests for `TranslationConfig` validation in `putio-sync-core/src/translation.rs`: kind rules (chat needs `base_url` http/https with host and non-empty `model`; callback needs `url` and `callback_base_url` with "no user info, no query or fragment, trailing `/` removed"), ranges (`max_in_flight` 1..=8, `timeout_secs` 10..=1800, `chunk_cues` 5..=200, callback `timeout_secs` 60..=86400, `instruction` at most 500 characters, headers at most 20), mask/unmask of `api_key` and header values, a leftover mask for a new header is an error, `Debug` prints `<redacted>`
- [x] T004 Implement `TranslationConfig`, `ChatConfig`, `CallbackConfig`, `TranslatorKind`, `validate`, mask helpers in `putio-sync-core/src/translation.rs`; add the `translation` field and validation call in `putio-sync-core/src/config.rs`
- [x] T005 [P] Document `[translation]`, `[translation.chat]`, `[translation.callback]` and its headers in `config.example.toml`
- [x] T006 [P] Write failing migration tests in `putio-sync-db/tests/pipeline_migration.rs`: `translation_callbacks` (state CHECK `waiting|done|failed|expired`, `token_hash` PRIMARY KEY, `job_id` UNIQUE, cascade from `pipeline_steps`) and `subtitle_provenance` (source CHECK) exist; the `kind` CHECK rebuild adds `translate_scan` and `translate` on a DB that already has 025's rebuild, keeps `pipeline_attempts` rows, and is idempotent
- [x] T007 Add the `CREATE` text for `translation_callbacks` (with indexes `(state, expires_at)` and `step_id`) and `subtitle_provenance` in `putio-sync-db/src/schema.rs` and register them in `putio-sync-db/src/lib.rs` (columns exactly as data-model.md)
- [x] T008 Make 025's `pipeline_steps` CHECK rebuild take the kind list, add `translate_scan` and `translate`, and update the new-install `CREATE TABLE` text in `putio-sync-db/src/schema.rs` and `putio-sync-db/src/lib.rs` (gap G4)
- [x] T009 [P] Add `StepKind::{TranslateScan,Translate}` with storage names in `putio-sync-pipeline/src/model.rs`; extend the round-trip tests
- [x] T010 [P] Extend `KINDS` with `translate_scan`, `translate` in `putio-sync-telemetry/src/steps.rs`; add `putio-sync-telemetry/src/translation.rs` with the metrics of research R12 (`translations_total{kind,result,language}`, `chunks_total{result}`, `chunk_retries_total`, `rate_limited_total`, `seconds{kind}`, `callbacks_waiting`, `callbacks_refused_total{reason}`), language label limited to configured languages or `other`; tests for names and bounded labels
- [x] T011 [P] Write failing tests for the prompt and reply format in `putio-sync-translation/src/prompt.rs`: cue to `N| text` with `<br>`, system prompt text per research R3 with and without `instruction`, context block of up to 3 source cues, reply parse (numbered lines, `<think>` removal, code fence, commentary before and after dropped, continuation line joined with `<br>`, `<br/>` and `<BR />` to newline), validation (exact numbers and order, empty text refused), `restore_wrapping` of whole-cue tags `<i>..</i>` and `{\an8}`, no-letter cues (`♪`, `...`) excluded and copied
- [x] T012 Implement `prompt.rs` (encode, system/user prompt, parse, validate, `restore_wrapping`, translatable-cue test)
- [x] T013 [P] Write failing tests for the chunk plan and the retry/split driver in `putio-sync-translation/src/chunk.rs` using a fake `ChatCall` trait: chunks of `chunk_cues` with 3 context cues, attempt 1 fails then retry, attempt 2 fails then split in half, depth limit 3, single cue failing twice fails, fatal errors skip retries, results placed by cue number, at most 30 calls per chunk, concurrency never above the limit
- [x] T014 Implement `chunk.rs` (plan, driver with `JoinSet`, depth and attempt rules of research R4/R5)
- [x] T015 [P] Write failing tests for re-alignment in `putio-sync-translation/src/align.rs`: equal times accepted, count mismatch refused with both counts, equal count and different times take the source times and report `realigned`, empty result text for a non-empty source cue refused, `verify_same_timing` failure, a property test over random cue lists (SC-001)
- [x] T016 Implement `align.rs` (`realign`, `verify_same_timing`)
- [x] T017 [P] Write failing tests for source selection in `putio-sync-translation/src/source.rs`: first wanted language present, then provenance `transcribed`, then any other in language-code order, `translated` rank last, size-mismatch provenance counts as unknown, forced/SDH files ignored, target language excluded, none gives "no source subtitle"
- [x] T018 Implement `source.rs`; add `lang::name(code)` to `putio-sync-subtitles/src/lang.rs` if 025's table does not expose it (gap G7)
- [x] T019 [P] Write failing tests for tokens and callback bodies in `putio-sync-translation/src/callback.rs`: token is 64 hex characters and differs per call, hash is SHA-256 hex of the token, URL builder (`<base>/translate/callback/<token>`, path prefix, trailing slash), outbound reply classification (200 text, 200 JSON `srt`, 200 JSON `error`, 200 JSON without srt, 202, other 2xx, 3xx, 4xx/5xx with scrubbed message), inbound body parse (text, JSON `srt`, JSON `error`)
- [x] T020 Implement the token, hash, URL builder, reply classifier and inbound body parser in `putio-sync-translation/src/callback.rs`
- [x] T021 [P] Write failing tests in `putio-sync-core/src/log_redact.rs`: the segment after `/translate/callback/` is masked in a message, in a field value and inside a longer URL; other paths are unchanged
- [x] T022 Add the callback-path rule to `putio-sync-core/src/log_redact.rs`
- [x] T023 [P] Write failing tests then implement provenance queries (`upsert`, `get`, `rekey_prefix`, prune older than 90 days) in `putio-sync-pipeline/src/store.rs` and `putio-sync-pipeline/tests/provenance.rs`
- [x] T024 Record provenance for each file 025's per-video step saves (the `SavedSubtitle.source` it already holds) in `putio-sync-pipeline/src/subtitles.rs`, with a test in `putio-sync-pipeline/tests/subtitles.rs` (gap G1; edits 025 code)
- [x] T025 Factor 025's video scan into `scan_videos(item)` and its manual-item registration into `register_manual_item(path, stage, steps)` in `putio-sync-pipeline/src/subtitles.rs` (gaps G2, G3); 025's tests must stay green

**Checkpoint**: foundation ready.

## Phase 3: User Story 1 - Translate with my local model (P1) MVP

**Goal**: a missing wanted language is translated through an OpenAI-compatible chat server, same cue count and timings.
**Independent test**: a mock chat server echoes each line with a prefix; a 300-cue English SRT to Spanish gives 300 cues with
the same numbers and timestamps, in more than one request.

### Tests for US1 (write first, must fail)

- [x] T026 [P] [US1] Chat client tests with mockito in `putio-sync-translation/tests/chat.rs`: endpoint rule (empty path, `/v1`, `/chat/completions`), request body fields, Bearer only when the key is set, content parse and `<think>` strip, `finish_reason: length`, `401`/`403` fatal, `404` model-not-found (Ollama and OpenAI error shapes and a plain-string `error`) versus wrong URL, `400` context message splits, `429` with `Retry-After` does not use an attempt and releases the permit, `5xx` retried, request timeout, key scrubbed from errors
- [x] T027 [P] [US1] Engine tests in `putio-sync-translation/tests/chat_engine.rs`: 300 cues keep numbers and timestamps (SC-001), more than one request, context lines present, wrong line count retries then splits then succeeds, always-bad reply fails and writes nothing, commentary stripped, no-letter cues copied and not sent, tags restored, RTL text and UTF-8, `max_in_flight` respected by counting concurrent mock requests, progress callback reports the share done
- [x] T028 [P] [US1] Pipeline planning and scan tests in `putio-sync-pipeline/tests/translate.rs`: kind `none` gives stage skipped `not_configured`; no wanted languages gives `not_configured`; scan expands to one `translate` step per (video, missing language) with branch = language code; two languages run as two branches in parallel; an existing sidecar for a wanted language is skipped; source order (US1 acceptance 6); a season pack gives steps per video; zero videos gives `no_video`; nothing to translate passes the scan
- [x] T029 [P] [US1] Pipeline run tests in `putio-sync-pipeline/tests/translate.rs`: a successful step writes `<video>.<lang>.srt` through the no-overwrite writer and records provenance `translated`; one language failing does not stop the other; failure retries per `optional_retries` then the item is `done_with_warnings` and no file exists; gate keeps Radarr/Sonarr pending until the stage ends; staging mode writes in the staging folder and the provenance rows follow the rename; a target that appeared meanwhile passes "already exists"
- [ ] T030 [P] [US1] API tests for `GET`/`PUT /settings/translation` (validation, masked key and header values, applies now, warning when no wanted languages), and secrets sweep covering the key, in `putio-sync-api/tests/translation_settings.rs` and `putio-sync-api/tests/secrets_sweep.rs`
- [ ] T031 [P] [US1] API tests for `POST /settings/translation/test` chat mode in `putio-sync-api/tests/translation_test_route.rs`: ok with sample lines, wrong model (`problem: model` with `models`), wrong key, unreachable, `404` endpoint, bad format with raw reply, timeout within 15 s (SC-003), masked key keeps the stored one, key not in the response

### Implementation for US1

- [x] T032 [US1] Implement `chat.rs` (endpoint rule, request, reply, error classes, 429 wait with permit release, `think` strip, scrubber)
- [x] T033 [US1] Implement `Translator` (settings snapshot, in-flight semaphore, `configure`) in `putio-sync-translation/src/lib.rs` and `translate_with_chat` (chunk driver, re-assembly, `verify_same_timing`, `restore_wrapping`, progress callback) in `putio-sync-translation/src/engine.rs`
- [x] T034 [US1] Plan the Translating stage (`TranslateScan` step, `not_configured` and `category` skips through `applies_to`) in `putio-sync-pipeline/src/plan.rs`
- [x] T035 [US1] Implement the scan (uses `scan_videos`, sidecars, `insert_steps` per language) and the `translate` step runner for chat in `putio-sync-pipeline/src/translate.rs`; add the kind dispatch in `putio-sync-pipeline/src/runner.rs` through the shared attempt-finish code
- [x] T036 [US1] Count failed `translate`/`translate_scan` steps as warnings in `finalize()`, add the kinds to `refresh_gate()` in `putio-sync-pipeline/src/gate.rs`, make `Settings::staging_on` (Auto) true when translation is configured, and re-key provenance rows after the staging rename in `putio-sync-pipeline/src/staging.rs`
- [x] T037 [US1] Inject `Arc<Translator>` and live settings into `Pipeline`, with `configure_translation`, in `putio-sync-pipeline/src/lib.rs`
- [ ] T038 [US1] Build the `Translator` and pass it to the pipeline in `putio-sync-daemon/src/daemon.rs`
- [x] T039 [US1] Record metrics and structured logs (`translation done`, `translation chunk retry`, `translation failed`) in `putio-sync-translation/src/engine.rs` and `putio-sync-pipeline/src/translate.rs`; the key is registered with `log_redact::set_secrets` and never a field
- [ ] T040 [US1] Settings routes `GET`/`PUT /settings/translation`, `configure_translation`, group registration in `putio-sync-api/src/routes/system/translation.rs`, `settings.rs`, `assistant.rs`
- [ ] T041 [US1] Implement `POST /settings/translation/test` chat mode (sample, problem classes, `GET <base>/models` on model errors, 15 s limit) in `putio-sync-api/src/routes/system/translation.rs`
- [ ] T042 [P] [US1] Translation settings card (kind selector, chat fields, extra instruction, in-flight, chunk size, Test button with the named problem) in `putio-sync-api/ui/settings/integrations.js`, register in `putio-sync-api/ui/settings/sections.js`; extend `putio-sync-api/tests/ui/settings_sections.test.mjs`
- [ ] T043 [P] [US1] Show `translate` step names, language branches and the step message in the timeline UI; extend `putio-sync-api/tests/ui/timeline.test.mjs`

**Checkpoint**: US1 works alone against mocks.

## Phase 4: User Story 2 - Translate through my own API (callback) (P2)

**Goal**: POST the subtitle to a custom service; take an immediate `200` or park on `202` and finish on a one-use callback.
**Independent test**: a mock service answers SRT at once in one case, and `202` then calls the callback URL in the other; a
wrong or reused token is refused.

### Tests for US2 (write first, must fail)

- [ ] T044 [P] [US2] Outbound client tests with mockito in `putio-sync-translation/tests/callback_client.rs`: body fields (`job_id`, `source_lang`, `target_lang`, `video_name` without directories, `srt`, `callback_url`), configured headers sent, `200` text, `200` JSON, `202`, `3xx` not followed, `4xx`/`5xx` with scrubbed message, timeout, header values never in errors
- [ ] T045 [P] [US2] Engine tests in `putio-sync-translation/tests/callback_engine.rs`: immediate `200` result checked and saved; equal count with different times is re-aligned from the source; different count refused; empty cue refused; the result is never written when `verify_same_timing` fails
- [ ] T046 [P] [US2] Pipeline tests in `putio-sync-pipeline/tests/translate_callback.rs`: `202` parks the step (state `running`, attempt open, job row `waiting`, item permit released, other branches continue); a valid callback completes the step and the item goes on; unknown, used, expired and wrong-job tokens refused and nothing saved; `{"error"}` body fails the step; result for a changed source refused; restart keeps a waiting job and does not re-send it (`interrupt_running` carve-out) while a chat step mid-run becomes `interrupted`; timeout fails the step without an automatic retry, event `pipeline.step_failed` fired, item `done_with_warnings`; callback arriving before the `202` reply is read still completes once; cancelled item expires its jobs; single use under two concurrent callbacks
- [ ] T047 [P] [US2] API tests for `POST /translate/callback/{token}` in `putio-sync-api/tests/translate_callback.rs`: works without the API token, text and JSON bodies, `422` for a bad subtitle, identical `404` body for unknown, expired, used and malformed tokens, failures counted by the limiter and `429` with `Retry-After`, `413` over 4 MiB, a captured-log test proving the token and callback URL never appear in logs or the trace span
- [ ] T048 [P] [US2] API tests in `putio-sync-api/tests/translation_test_route.rs` and `translation_settings.rs`: `PUT` with kind callback and no `callback_base_url` is refused with the field named (FR-016), Test callback mode (`immediate`, `will_call_back`, `base_url_reachable_from_here`, `problem: callback_base_url`, header values not echoed, no `translation_callbacks` row created)
- [ ] T049 [P] [US2] UI logic tests for the callback section (base URL required, "Use this address" fills from the page origin, header value masking) and the "waiting for callback" label in `putio-sync-api/tests/ui/settings_sections.test.mjs` and `timeline.test.mjs`

### Implementation for US2

- [ ] T050 [US2] Implement the outbound `submit_callback` (job id, token, request, reply classes, `Immediate`/`Accepted`/`Failed`) in `putio-sync-translation/src/callback.rs` and `engine.rs`, reusing `align.rs`
- [ ] T051 [US2] Add callback queries to `putio-sync-pipeline/src/store.rs` (`insert_job`, atomic `claim(token_hash, now)`, `get`, `expire_due(now)`, `expire_for_item`, `expire_for_step`), and change `interrupt_running` to leave steps that have a `waiting` job (and their open attempts) alone
- [ ] T052 [US2] Implement parking, `accept_callback(token, body, ip)`, `expire_callbacks(now)` (attempt `timeout`, no auto retry), the `Parked` outcome handling in `advance()` and the post-`202` re-read, and `wake()` in `putio-sync-pipeline/src/translate.rs`, `runner.rs`, `lib.rs`
- [ ] T053 [US2] Call `expire_callbacks` each tick, mark leftover `waiting` rows of interrupted steps `expired` at start, and connect `wake` to the tick loop in `putio-sync-daemon/src/daemon.rs`
- [ ] T054 [US2] Add the public route `POST /translate/callback/:token` (4 MiB `DefaultBodyLimit`, limiter `check` and `record_failure`, uniform `404`, `422`, `200 {"ok": true}`) in `putio-sync-api/src/routes/translate_callback.rs` and register it in `public_routes()` in `putio-sync-api/src/lib.rs`
- [ ] T055 [US2] Replace `TraceLayer::new_for_http()` in `router()` with a `make_span_with` that records `/translate/callback/<redacted>` for that path, in `putio-sync-api/src/lib.rs`
- [ ] T056 [US2] Callback validation (`callback_base_url` required for kind callback) in the `PUT` handler, callback mode of the Test route (base URL check, sample job without a real token), and the `callback_url_form` in GET, in `putio-sync-api/src/routes/system/translation.rs`
- [ ] T057 [US2] Add `waiting` (`for`, `since`, `expires_at`) to timeline steps in `putio-sync-pipeline/src/timeline.rs` and `putio-sync-api/src/routes/active.rs`; update the `callbacks_waiting` gauge and the refused-callback metric with reasons in `putio-sync-translation` and the route
- [ ] T058 [P] [US2] Callback section of the Translation card (URL, headers, base URL with "Use this address", timeout, callback URL form, Test result) in `putio-sync-api/ui/settings/integrations.js`
- [ ] T059 [P] [US2] Active row label "Translating: <video> (<lang>), waiting for callback" in `putio-sync-api/ui/pages/active.js`

**Checkpoint**: US1 and US2 work independently.

## Phase 5: User Story 3 - Choose per category and translate by hand (P3)

**Goal**: translation off per category, and "Translate subtitles" on the Local files page.
**Independent test**: POST a translate request for a video on disk with `x.en.srt`; a `manual:` item writes `x.es.srt`.

### Tests for US3 (write first, must fail)

- [ ] T060 [P] [US3] Planner test: an excluded category gives the stage skipped `category` ("not for this category"), matching is case-insensitive, other categories still translate, in `putio-sync-pipeline/tests/translate.rs`
- [ ] T061 [P] [US3] Manual-item tests: `manual` source, one `translate` step per target with branch = language, size and sample rules bypassed, existing target refused, duplicate gives 409 data, result message names the saved file, in `putio-sync-pipeline/tests/translate.rs`
- [ ] T062 [P] [US3] API tests: preflight (eligible, no source, no translator, not a video, existing targets flagged), POST `202`/`409`/`400`/`404`, path guard, more than 10 targets, in `putio-sync-api/tests/local_translate.rs`
- [ ] T063 [P] [US3] UI logic test for the target chooser (existing targets disabled, none selected disables the button) and the poll-until-done flow in `putio-sync-api/tests/ui/files_translate.test.mjs`

### Implementation for US3

- [ ] T064 [US3] Excluded-categories field in the Translation card in `putio-sync-api/ui/settings/integrations.js` (the planner side is T034)
- [ ] T065 [US3] Add `Pipeline::start_manual_translation(path, targets)` using `register_manual_item` in `putio-sync-pipeline/src/translate.rs`, `lib.rs`
- [ ] T066 [US3] Implement preflight and POST in `putio-sync-api/src/routes/local/translate.rs`, register next to the 044 and 025 routes in `putio-sync-api/src/routes/local/files.rs`/`mod.rs`
- [ ] T067 [US3] "Translate subtitles" row action with the target chooser, polling `GET /active/{key}/timeline` every 3 s, result message and listing refresh in `putio-sync-api/ui/pages/files.js`
- [ ] T068 [P] [US3] Add a pointer to the new routes in `specs/044-local-files/contracts/api.md`

## Phase 6: Polish

- [ ] T069 [P] README section (Translation card, Ollama and hosted examples, `num_ctx` note, callback mode and base URL, the unauthenticated callback route and its token rules) in `README.md`
- [ ] T070 [P] CHANGELOG entry under Unreleased in `CHANGELOG.md`; add the translation step failures to `EVENTS.md` if the 038 docs list step kinds
- [ ] T071 [P] Mark roadmap item 026 and note deviations in `ROADMAP.md` and a "Deviations" section at the end of this file (callback timeout without auto retry, no-letter cues not sent, positional numbering)
- [ ] T072 Reconcile with 025 as implemented: names of `srt`, `lang`, `naming`, `insert_steps`, `register_manual_item`, the migration helper and the settings pattern; fix the design docs or code where they differ
- [ ] T073 Run `make ci` and fix findings
- [ ] T074 Manual run per `quickstart.md` against a real Ollama (and a hosted API if available); record real error bodies and the context-length behaviour in `specs/026-subtitle-translation/contracts/chat.md` and research Q1/Q3/Q4
- [ ] T075 Manual run of the callback flow with a small local service; confirm the log viewer and `docker logs` never show a token, and that a restart keeps a waiting job

## Dependencies and order

- Phase 1, then Phase 2. Within Phase 2 each test task precedes its implementation; [P] tests can run together. T006 before T007 and T008; T018 needs T017; T023 and T024 need T007; T024 and T025 edit 025's `subtitles.rs` and need 025 merged.
- US1 (T026..T043) needs Phase 2. T033 needs T012, T014, T016, T032. T035 needs T025, T018, T033, T034. T036 needs T035. T037 needs T033. T038 needs T037. T040 needs T004. T041 needs T033, T040. UI tasks T042, T043 need T040.
- US2 (T044..T059) needs US1's engine and step runner (T033, T035). T050 needs T020, T016. T051 needs T007. T052 needs T050, T051. T053 needs T052. T054 needs T052, T022. T056 needs T040. T057 needs T052. UI T058, T059 need T056, T057.
- US3 (T060..T068) needs T034 (planner), T025 (manual helper), T040. T065 needs T025 and T035. T066 needs T065. T067 needs T066.
- Polish after the stories; T074 and T075 last.

## Parallel examples

- Phase 2 tests T003, T006, T011, T013, T015, T017, T019, T021, T023 in parallel.
- US1 tests T026..T031 in parallel; UI T042, T043 in parallel after T040.
- US2 tests T044..T049 in parallel; UI T058, T059 in parallel.

## Implementation strategy

MVP is Phase 1, Phase 2 and US1 (chat translation, Ollama). Ship it, then US2 (callback), then US3.
