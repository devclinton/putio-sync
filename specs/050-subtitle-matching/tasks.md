# Tasks: Subtitle Matching and Source Modes (050)

**Input**: `specs/050-subtitle-matching/` (spec, plan, research, data-model, contracts, quickstart). Extends the built 025.
**Tests**: included (Constitution III; no real OpenSubtitles, Put.io, *arr or Whisper). Write each test task first and see it fail.
**Format**: `- [ ] T### [P] [US#] description with file path`. [P] = different files, no dependency on an unfinished task.
**Rule**: an unchanged 025 config must behave and round-trip as before; the 025 tests stay green throughout.

## Phase 1: Setup

- [X] T001 [P] Add module `media` (empty `MediaInfo`, `MediaKind`, `MediaRequest`, `MediaLookup` skeleton) to `putio-sync-core/src/media.rs` and export it in `putio-sync-core/src/lib.rs`; move or re-export the `BoxFut` alias there if `core` has none
- [X] T002 [P] Create fixture folder `putio-sync-subtitles/tests/fixtures/opensubtitles/` with `search_hash.json`, `search_ids.json`, `search_empty.json`, `download.json`, `login.json` copied from `contracts/opensubtitles-api.md`

## Phase 2: Foundational (blocks all stories)

- [X] T003 [P] Write failing tests in `putio-sync-core/src/subtitles.rs` for `SubtitleMode` serde (snake_case), `mode_for` precedence (language over category over global over default; category case-insensitive), validation (unknown mode, unknown language key, `whisper_only`/`both` anywhere needs `transcription.enabled` + URL, `min_confidence` 0..=100), and that a 025 TOML (no new keys) loads and serializes byte-identical
- [X] T004 Implement `SubtitleMode`, `category_modes`, `language_modes`, `mode: Option<SubtitleMode>` (skipped in output when `None`), `min_confidence` (default 65) and `mode_for` in `putio-sync-core/src/subtitles.rs`; extend `normalize`/`validate`/`masked`/`unmasked`
- [X] T005 [P] Write failing tests in `putio-sync-subtitles/src/naming.rs`: `<stem>.<lang>.whisper.srt` name, `is_transcription`, and `existing_sidecars` NOT counting `movie.en.whisper.srt` as the main `en` slot (a `.whisper` file plus `.forced` and `.sdh` files all independent)
- [X] T006 Add `SlotKind::Whisper`, name/path building, `is_transcription`, `language_of`, and the `whisper` token exclusion in `putio-sync-subtitles/src/naming.rs`; update `Outcome::message` in `engine.rs` for the new kind
- [X] T007 [P] Write failing tests in `putio-sync-telemetry/src/subtitles.rs` for the new `source` values (`os_hash`, `os_ids`, `os_name`), `subtitles_skipped_total{reason}` and `opensubtitles_requests_total{endpoint,result}` with bounded labels
- [X] T008 Implement those counters in `putio-sync-telemetry/src/subtitles.rs`; extend `SourceKind::as_metric` labels in `putio-sync-subtitles/src/candidates.rs`
- [X] T009 Define `ModePlan` (per-language modes, `other`, `downloads(lang)`, `always_transcribe()`, `fallback_transcribe()`, `whisper_token(detected)`) with table tests in `putio-sync-subtitles/src/modes.rs` and export from `lib.rs`

**Checkpoint**: modes resolve, `.whisper.srt` is a distinct slot, metrics exist.

## Phase 3: User Story 1 - Four modes (P1) MVP

**Goal**: Download only, Download then Whisper, Whisper only, Both, globally, per category and per language; `.whisper.srt` in Both.
**Independent Test**: mock Put.io (English subtitle) + mock Whisper: Both gives `movie.en.srt` and `movie.en.whisper.srt`; Whisper only never calls Put.io and writes `movie.en.srt`; Download only never calls Whisper; Download then Whisper with a download present never calls Whisper; no-mode config behaves as 025.

### Tests for User Story 1

- [X] T010 [P] [US1] Write failing engine tests in `putio-sync-subtitles/tests/modes.rs` (fake `SubtitleSource` counting calls, mock Whisper server, fake ffmpeg script as in 025 tests): the four modes, Both with an existing `.en.srt` (kept, `.whisper.srt` written), Whisper only with an existing `.en.srt` ("already present", nothing replaced), detected language not wanted in Both (`.xx.whisper.srt`), per-language override beating category and global
- [X] T011 [P] [US1] Write failing pipeline test in `putio-sync-pipeline/tests/subtitles.rs`: category mode applied to an item, `excluded_categories` wins over a mode, Whisper-only item makes no `source.list` call, step message lists `en: transcribed (whisper)`; an unchanged 025 settings value gives the 025 messages
- [X] T012 [P] [US1] Write failing API tests in `putio-sync-api/tests/` for `GET/PUT /settings/subtitles` new fields, masked round trip, and the 400 for a Whisper mode with transcription off (message names the key)
- [X] T013 [P] [US1] Write failing UI logic tests in `putio-sync-api/tests/ui/subtitles_modes.test.mjs` for the mode view model (resolved label, override rows, save payload omits empty maps)

### Implementation for User Story 1

- [X] T014 [US1] Add `modes: &ModePlan` to `VideoRequest` and make `fetch_for_video` skip languages that do not download (and not call `source.list` when none do) in `putio-sync-subtitles/src/engine.rs`
- [X] T015 [US1] Replace `wants_transcription` with the mode-aware trigger (always for `whisper_only`/`both`; 025 rule for `download_then_whisper`; never for `download_only`) and pass the `whisper` slot in `transcribe_video` when `ModePlan::whisper_token(detected)` in `putio-sync-subtitles/src/engine.rs`; keep the no-overwrite behaviour and the "already present" warning
- [X] T016 [US1] Build the `ModePlan` per video from `SubtitlesConfig::mode_for(category, lang)` in `putio-sync-pipeline/src/subtitles.rs` (`run_subtitles`, `subtitles_off`, manual item path) and show the mode in the step message
- [X] T017 [US1] Extend `GET/PUT /settings/subtitles` with `mode`, `category_modes`, `language_modes`, `min_confidence` and `modes` in `putio-sync-api/src/routes/system/subtitles.rs`; include the new fields in settings import/export and the config assistant list where 025 listed subtitles
- [X] T018 [P] [US1] Add the mode select, category override rows and per-language override control to the Subtitles card in `putio-sync-api/ui/settings/subtitles.js` with the helper in `putio-sync-api/ui/subtitle_modes.js`; update the Local files preflight text for Whisper only in `putio-sync-api/src/routes/local/subtitles.rs`
- [X] T019 [P] [US1] Document `mode`, `category_modes`, `language_modes` and `min_confidence` in `config.example.toml`
- [X] T020 [US1] Run `cargo test -p putio-sync-subtitles -p putio-sync-core -p putio-sync-pipeline -p putio-sync-api` and fix 025 regressions (SC-001)

**Checkpoint**: US1 ships alone.

## Phase 4: User Story 2 - OpenSubtitles by fingerprint and IDs (P2)

**Goal**: for languages still missing after Put.io, search OpenSubtitles by file fingerprint, then by IMDb/TMDb IDs with season and episode from Radarr/Sonarr; rank by release; skip below the threshold; respect the quota.
**Independent Test**: mock OpenSubtitles + fake `MediaLookup`: fingerprint hit saved at 100; with no hit, ID search saves the result whose group, source and resolution match; a result at 60 is not saved; quota 406 stops downloads without failing the item.

### Tests for User Story 2

- [X] T021 [P] [US2] Write failing tests for the OpenSubtitles file hash in `putio-sync-subtitles/src/hash.rs`: known-answer vector from a published sample, file under 128 KiB returns `None`, size wraparound, last block shorter than 64 KiB handled
- [X] T022 [P] [US2] Write failing table tests for the release parser in `putio-sync-subtitles/src/release.rs` (title, year, `SxxEyy`, `NxNN`, resolution, source incl. `WEB-DL`/`web.dl`, codec, group suffix and `[Group]`, a title that contains a year, `S01E01E02`, junk names)
- [X] T023 [P] [US2] Write failing score tests in `putio-sync-subtitles/src/score.rs`: fingerprint = 100; IDs alone = 60; +15/+10/+10/+5 for group/source/resolution/codec; default threshold 65 accepts 70 and rejects 60; download-count tie-break never moves a score across the threshold
- [X] T024 [P] [US2] Write failing mockito tests in `putio-sync-subtitles/tests/opensubtitles.rs`: headers (`Api-Key`, `User-Agent`), sorted lowercase query, search parsing (lenient fields), download two-step (`/download` then link), login and token reuse, 401 key rejected, 406 quota exhausted (no further requests until reset), 429 waits `Retry-After`, no secret in any error string
- [ ] T025 [P] [US2] Write failing mockito tests in `putio-sync-arr/tests/media_lookup.rs`: Radarr and Sonarr history by `downloadId`, season pack picks all episodes, cache hit makes no second call, unreachable server and empty history return `None`, `/movie/{id}` fallback, API key not logged
- [X] T026 [P] [US2] Write failing engine tests in `putio-sync-subtitles/tests/matching.rs`: levels in order and early stop, one search per level for all missing languages, skip when inputs missing, threshold skip recorded as `below_threshold(score)`, quota gives `quota` skip then Whisper fallback in Download then Whisper, machine-translated dropped, hearing-impaired not used as main, existing subtitle means no request

### Implementation for User Story 2

- [X] T027 [P] [US2] Implement `opensubtitles_hash(path) -> io::Result<Option<String>>` in `putio-sync-subtitles/src/hash.rs` (std only, two 64 KiB reads)
- [X] T028 [P] [US2] Implement `ReleaseInfo::parse` and its enums in `putio-sync-subtitles/src/release.rs` (std only, rules in research R5)
- [X] T029 [US2] Implement `Score`, constants, `score_ids`, `score_hash` and the threshold check in `putio-sync-subtitles/src/score.rs` (depends on T028)
- [X] T030 [P] [US2] Add `OpenSubtitlesConfig` to `putio-sync-core/src/subtitles.rs` (`api_key`, `username`, `password`, `user_agent`; `is_active`, env overrides `PUTIO_SYNC_OPENSUBTITLES_API_KEY` and `..._PASSWORD`, redacted `Debug`, mask/unmask, "password needs username"); extend its tests
- [X] T031 [US2] Implement the REST client in `putio-sync-subtitles/src/opensubtitles.rs`: `search`, `download`, `login`, `QuotaState`, pacing (4 per second), 429 handling, redacted errors; trait `SubtitleSearch` for the engine seam (depends on T030)
- [X] T032 [P] [US2] Fill `MediaInfo`, `MediaKind`, `MediaRequest`, `MediaLookup` in `putio-sync-core/src/media.rs` per `contracts/media-lookup.md`
- [ ] T033 [US2] Add `ArrApi::history_by_download` (Radarr and Sonarr variants) in `putio-sync-arr/src/arr_api.rs` and `ArrMedia: MediaLookup` with the hash cache in `putio-sync-arr/src/media.rs` (uses `arr_items.get_by_file_id`)
- [X] T034 [US2] Implement the matching levels (fingerprint, IDs) in `putio-sync-subtitles/src/matching.rs`: build the search, convert results to scored candidates in `candidates.rs`, pick the best at or above the threshold per language, try the next on a bad download, record `SkipNote`s (depends on T027-T029, T031-T032)
- [X] T035 [US2] Hook the OpenSubtitles pass into `fetch_for_video` after the Put.io pass for languages that download and are still missing, honouring `Skipped(quota)`, in `putio-sync-subtitles/src/engine.rs`; saved files use sources `os_hash` and `os_ids`; step message shows level and score
- [ ] T036 [US2] Pass the `SubtitleSearch` client and `Arc<dyn MediaLookup>` into the pipeline and the engine request (file ID, category, hash path) in `putio-sync-pipeline/src/subtitles.rs` and `putio-sync-pipeline/src/lib.rs`
- [ ] T037 [US2] Build the client and the `ArrMedia` adapter and rebuild them on settings change in `putio-sync-daemon/src/daemon.rs`
- [ ] T038 [US2] Add the `opensubtitles` fields and `POST /settings/subtitles/opensubtitles/test` (unsaved input allowed, masked values keep stored ones) in `putio-sync-api/src/routes/system/subtitles.rs`; extend the secrets sweep test to the new fields and the JWT
- [ ] T039 [P] [US2] Add the OpenSubtitles section (key, login, threshold, Test, quota text) to `putio-sync-api/ui/settings/subtitles.js` and logic tests in `putio-sync-api/tests/ui/subtitles_opensubtitles.test.mjs`
- [ ] T040 [P] [US2] Document `[subtitles.opensubtitles]` in `config.example.toml`
- [ ] T041 [US2] Verify US2 acceptance 1-8 against the mocks; run `cargo test -p putio-sync-subtitles -p putio-sync-arr -p putio-sync-pipeline -p putio-sync-api`

**Checkpoint**: US1 and US2 work; OpenSubtitles needs only a key.

## Phase 5: User Story 3 - Match by file name (P3)

**Goal**: for videos without IDs, search OpenSubtitles by the title, year, season and episode parsed from the name, with the same ranking and threshold.
**Independent Test**: `Show.Name.S02E05.1080p.WEB-DL.x264-GRP.mkv` queries by title/season/episode; a name with no title searches nothing; a result with a different title is never saved.

### Tests for User Story 3

- [X] T042 [P] [US3] Write failing tests for title similarity (normalised tokens, Dice) and the name-level score (title x40 + year or episode agreement up to 10 + release parts; any year/season/episode disagreement scores 0) in `putio-sync-subtitles/src/score.rs`
- [X] T043 [P] [US3] Write failing engine tests in `putio-sync-subtitles/tests/matching_name.rs`: name level used only when earlier levels found nothing or lacked inputs, query params (`query`, `type`, `year` or `season_number` and `episode_number`), unparsable name makes no request and notes `no_name`, wrong-title result rejected, `S01E01E02` uses the first episode
- [ ] T044 [P] [US3] Write failing test in `putio-sync-pipeline/tests/subtitles.rs` for the manual Fetch subtitles item using all three levels and the modes

### Implementation for User Story 3

- [X] T045 [US3] Implement `similarity` and `score_name` in `putio-sync-subtitles/src/score.rs`
- [X] T046 [US3] Add the name level (query builder from `ReleaseInfo`, `type` movie/episode) in `putio-sync-subtitles/src/matching.rs` and call it from `fetch_for_video` after IDs; source label `os_name`
- [ ] T047 [US3] Make the manual path (`start_manual_subtitles`, Local files) pass the same lookups and OpenSubtitles client in `putio-sync-pipeline/src/subtitles.rs`; bypass nothing but the size/sample rules (025)
- [ ] T048 [US3] Verify US3 acceptance 1-4 against the mocks

**Checkpoint**: all three levels and four modes work.

## Phase 6: Polish and cross-cutting

- [ ] T049 [P] Update `README.md` (modes, OpenSubtitles key and quota, `.whisper.srt`, thresholds) and `INSTALLATION.md` if it lists subtitle settings
- [ ] T050 [P] Add the `CHANGELOG.md` entry for 050 (required per change)
- [ ] T051 [P] Update `ROADMAP.md` entry 025: move built parts out of "Not built yet" (the maintainer's text; keep wording, edit only the status note)
- [ ] T052 Check the UI bundle size test (`assets_total_under_480kb`); raise the cap only with a recorded reason in `putio-sync-api/tests/` and Deviations below
- [ ] T053 Add the new metrics to the dashboard/docs list where 025 listed its metrics (search `subtitles_saved_total` in docs and `Resources/`)
- [ ] T054 Run `make ci` locally (fmt, clippy -D warnings, tests, release build)
- [ ] T055 Live check by the maintainer (not CI): a real OpenSubtitles key, one Radarr and one Sonarr history call; correct `contracts/opensubtitles-api.md`, `contracts/media-lookup.md` and the fixtures where they differ, and record the real quota figures
- [ ] T056 Run `/speckit-analyze` and record Deviations here

## Dependencies and order

- Phase 1 then Phase 2 (T003-T009) then stories. US1 needs only Phase 2. US2 needs Phase 2 (and uses US1's `ModePlan` to decide which languages download); US3 needs US2 (`matching.rs`, `score.rs`, the client).
- Within US2: T021-T026 tests first; T027, T028, T030, T032 in parallel; T029 after T028; T031 after T030; T033 after T032; T034 after T027-T029, T031, T032; T035 after T034; T036-T038 after T035.
- Stories ship in order P1, P2, P3; each is a working increment (US1 alone changes no external call).

## Parallel examples

- US1: T010, T011, T012, T013 together; then T018 and T019 beside T017.
- US2: T021-T026 together; then T027, T028, T030, T032 together; T039 and T040 beside T038.
- US3: T042, T043, T044 together.

## Implementation strategy

MVP is US1 (Phase 1, 2, 3): modes and `.whisper.srt`, no new network service. Then US2 (the main matching gain, needs an
OpenSubtitles key), then US3. Tune the default threshold (65) on a real library after US2 (T055).

## Deviations

(none yet)

## Follow-up after 026 merges

- [X] T057 Once 026 (translation) is on main, record every transcription this feature writes (all modes, `.whisper.srt` or not) in 026's `subtitle_provenance` table, so translation can pick transcribed sources without relying on the file name; add a test in `putio-sync-pipeline/tests/subtitles.rs`
