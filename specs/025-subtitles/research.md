# Research: Subtitles (025)

Each entry: Decision, Rationale, Alternatives. "Confirmed" means seen in a source (named). "Unconfirmed" items are
listed again in the Open questions at the end, and each has a fallback that keeps the feature working.

## R1. Put.io subtitle API

**Decision**: Add two methods to `putio-sync-client`:

- `list_subtitles(file_id, languages: &[String]) -> SubtitleList` calling `GET /v2/files/{id}/subtitles`, with
  `?languages=en,es` (CSV, URL-encoded) when languages are given.
- `download_subtitle(file_id, key) -> String` calling `GET /v2/files/{id}/subtitles/{key}?format=srt` and
  returning the body as text (new raw-text GET next to the JSON `get::<T>`).

List response (confirmed): `{status:"OK", default: string|null, subtitles:[{format, key, language, language_code,
name, source, url}]}`, all seven fields strings. Fixture values seen: format `srt`, key `sub-1`, language
`English`, language_code `en`, name `English`, source `opensubtitles`.

Parsing is lenient: unknown fields ignored, every field except `key` optional (`#[serde(default)]`), so an API
change degrades to "fewer candidates" instead of a hard failure. Statuses 404 (no such file, file gone), 402 and
401 `invalid_scope` mean "Put.io cannot help": the Put.io part ends with `putio: unavailable` and the step moves
on to transcription. 5xx and 429 use the client's existing retry.

**Rationale**: Evidence: putdotio/putio-sdk-typescript `src/domains/files.ts` and `files.spec.ts`, and the
putio-sdk-swift `Subtitles` model (list shape, `languages` CSV, error statuses 401/404/402). The Go SDK doc
(pkg.go.dev `DownloadSubtitle`) says an empty key means `default` and the lookup order is: subtitle file in the same
folder with the same name, then a subtitle extracted from the MKV, then the first OpenSubtitles.org match. This
matches FR-005's ranking (folder and embedded before OpenSubtitles). The client crate already owns Put.io HTTP,
retries, auth and `with_base_url` for mockito, so the new calls need no new dependency.

**Alternatives**: Calling OpenSubtitles directly (needs a key, accounts and quota, and Put.io already did the search:
rejected, Out of scope per spec). Parsing `url` from the list instead of building the download path (kept as a
fallback: if the download path 404s and `url` is an absolute `https://api.put.io/...` URL, try it with the same
auth header).

**Unconfirmed** (fallbacks in brackets): the exact download path and `format=srt` parameter (taken from the
brief; Go SDK confirms a key-based download exists) [fallback: `url` field]; the `source` strings for folder and
embedded tracks (only `opensubtitles` seen) [map unknown to the lowest rank, count it as `opensubtitles` in
metrics, log a warn with the raw value]; whether Put.io marks forced or SDH (no flag field exists) [detect from
`name` text]; whether `languages=` also filters folder/mkv sources [client side filter as well, so it does not
matter]; whether non-UTF-8 text is normalised by Put.io [R6 handles it].

## R2. Audio for transcription: ffmpeg or the whole file

**Decision**: Extract audio with `ffmpeg` and send only audio. `ffmpeg` is installed in the Docker runtime image
(`apk add --no-cache ffmpeg`). Path detection: config `transcription.ffmpeg_path`; when empty, search `PATH`
(std only, no `which` crate). `ffprobe` is looked up next to the ffmpeg binary, then on `PATH`. The settings page
shows `ffmpeg: found at <path>` or `not found`.

When ffmpeg is missing (typical on a macOS desktop install without Homebrew ffmpeg): the settings save is allowed
and shows a warning, the Test button reports `ffmpeg not found` (and still tests the server with the in-process
sample), and a transcription step fails with the message `ffmpeg not found: set transcription.ffmpeg_path or
install ffmpeg`, after which 038's optional-step retry and give-up apply. Put.io subtitles (US1) never need ffmpeg.

Command per chunk (argv, no shell):
`ffmpeg -nostdin -v error -ss <start> -t <len> -i <video> -map 0:a:0 -vn -ac 1 -ar 16000 -c:a pcm_s16le -f wav
<tmp>.wav`. 16 kHz mono PCM is what Whisper uses internally, and a 600 s chunk is about 19 MB, under the 25 MB
OpenAI limit that speaches and others mirror. Temp files go in the item's temp directory (the staging folder's
parent is not used) and are deleted after each chunk, on error and on cancel.

**Rationale**: Movies are 2 to 60 GB, the audio is a few tens of MB. A 25 MB request limit makes sending the
file impossible for OpenAI-style servers. whisper.cpp's server expects WAV unless started with `--convert`, which
itself needs ffmpeg on the server. ffmpeg handles every audio codec seen in movies (AC-3, E-AC-3, DTS, TrueHD,
AAC, Opus), so the NAS, not the server, does the demux.

Image size: `apk add ffmpeg` on alpine 3.19 is expected to add roughly 100 MB. **Unmeasured**; the task list has a
step to measure it with `docker build` on the maintainer's machine and record it in the quickstart. If it is
unacceptable, the fallback is a build arg `WITH_FFMPEG=0` (not built now: Principle VII).

**Alternatives**: Sending the whole video (above: size limit, multi-GB upload, whisper.cpp needs WAV).
Decoding in process with `symphonia` (no AC-3/DTS/TrueHD, so most movies fail; also a big dependency). Linking
libav bindings (`ffmpeg-next`: heavy build, system lib coupling, breaks the macOS/Windows packages). Using the
Whisper server's own file handling for video (speaches decodes containers on its side but the upload is still
huge, and behaviour differs per server).

## R3. OpenAI-style transcription request, chunking, joining

**Decision**: `POST <url>` with `multipart/form-data`:

| Field | Value |
|---|---|
| `file` | `audio.wav` (the chunk) |
| `model` | config `model`, omitted when empty |
| `response_format` | `verbose_json` |
| `language` | config hint if set; else omitted for chunk 1, then the language the server detected in chunk 1 for all later chunks |
| `timestamp_granularities[]` | not sent (segment level is the default) |

Header `Authorization: Bearer <api_key>` only when a key is set. Timeout is `timeout_secs` per request (default
900, so a slow CPU can do a 10-minute chunk).

Endpoint rule: if the URL path is empty or `/`, append `/v1/audio/transcriptions`; otherwise use the URL exactly
as typed. So `http://nas:8000` works for speaches, and a whisper.cpp user enters `http://nas:8080/inference`.

Response handling: JSON `{language, duration, segments:[{start,end,text}], text}`. We build SRT ourselves from
`segments` (start and end are seconds, so we control the format and the join). If the body is not JSON but parses
as SRT (some servers ignore `response_format`), the SRT is parsed and used. `language` in the response is a name
(`english`) or code (`en`); both go through the R5 mapper. If no language comes back and no hint is set, the step
fails with `server did not report a language: set transcription.language`, because the file name needs one.
An empty segment list (no speech) saves nothing and the step ends as done with "no speech found".

Chunking: duration from `ffprobe -v error -show_entries format=duration -of csv=p=0`. Chunks of `chunk_secs`
(default 600, range 60 to 1800). Each chunk's segment times are shifted by the chunk start. Joining rules: chunks are
cut on exact boundaries with no overlap, so there is no overlap to dedupe; a segment that ends past the chunk end is
clamped to the chunk end, and a cue that would start before the previous cue's end is moved to it. Cues are
renumbered 1..n. Progress per chunk is written with `store.set_progress`. A global `Semaphore(1)` serialises
transcription across items so one NAS CPU is not asked for six at once. Cancelling the item kills ffmpeg
(`kill_on_drop`) and drops the HTTP request.

**Rationale**: `verbose_json` gives language and timestamps in one call on OpenAI, speaches (supports the OpenAI
transcription API) and, per its OpenAI-compatible mode, faster-whisper servers. Building SRT locally avoids
depending on each server's SRT formatting and makes chunk joining exact.

**Alternatives**: `response_format=srt` (no language in the response, so no language code for the file name; also
each chunk would need its own re-timing by parsing). One request for the whole audio (hits the 25 MB limit above
about 13 minutes of WAV; could use compressed audio, but compression adds ffmpeg time and servers differ).
Overlapping chunks with dedupe (more code, better at word cuts; Whisper cuts mid-word rarely enough for a first
release, listed as a possible later change).

**Unconfirmed**: speaches' accepted `response_format` list and its language field; whether whisper.cpp's
`/inference` returns `language` in `verbose_json` (its README shows only `json`, and a `--inference-path` option);
whether whisper.cpp serves `/v1/audio/transcriptions` at all. [Fallbacks above: SRT body parse, language hint,
explicit URL. The Test button exists to find this out on the user's own server, and the quickstart tells people to
press it.]

## R4. Sample and small-file skipping, which files are videos

**Decision**: A file in an item is a subtitle target when: its extension is one of `mkv mp4 m4v avi mov wmv ts
m2ts webm mpg mpeg` (case-insensitive), its size is at least 100 MB (100 * 1024 * 1024), and neither the file name
nor any parent directory name (relative to the item root) contains `sample` (case-insensitive). Both numbers are
constants, no config. A manual fetch (US3) bypasses the size and sample rules (the user pointed at the file) but
not the extension rule. Skipped files are counted in the scan step's message ("3 videos, 1 sample skipped").

**Rationale**: Spec edge case. `locate::list_files(dir, ids)` already gives `relative_path` and `size`.

**Alternatives**: Config knob for the size (Principle VII: no current need). ffprobe duration check (needs ffmpeg
for US1; rejected).

## R5. Language codes, forced and SDH naming

**Decision**: A static table in `putio-sync-subtitles/src/lang.rs` maps ISO 639-2/T, 639-2/B, 639-1 codes and English
names (as Put.io's `language` and `language_code` and Whisper's names give them) to one canonical code: the
two-letter ISO 639-1 code where one exists, else the three-letter code. Roughly 90 languages (Whisper's list plus
the OpenSubtitles list); unknown inputs map to the lowercased input if it is 2 or 3 ASCII letters, else the language
is `und` and is never saved by name. Special cases kept: `pt-br` and `pt-pt` stay as written lowercase, `zh-cn`/
`zh-tw` the same, since Radarr/Sonarr parse them. Config languages go through the same mapper, are de-duplicated
keeping order, and capped at 10 entries.

Name: `<video stem>.<lang>.srt`, `<video stem>.<lang>.forced.srt`, `<video stem>.<lang>.sdh.srt`, in the video's
directory. Forced/SDH detection, from the candidate `name` (no flag field exists): word match, case-insensitive, on
`forced`, `foreign parts only`, `sdh`, `hearing impaired`, `hi` (as a whole token only), `cc`. Forced and SDH
candidates never fill a language's main slot. The best forced and best SDH candidate per wanted language are saved
under their own names (so Radarr/Sonarr can import them); this is cheap and matches the spec's "when Put.io marks
them".

Existing-file detection (FR-003): a sidecar counts as the language's subtitle when it is in the video's directory,
its name starts with the video stem followed by `.`, its extension is `srt ass ssa vtt sub`, and the dot-separated
tokens after the stem include a code the mapper resolves to that language and none of `forced`, `sdh`, `hi`. Forced
and SDH sidecars count only for their own slot.

**Rationale**: Spec says two-letter codes "since that is what Radarr/Sonarr parse". A table is smaller and more
predictable than a crate (`isolang`, `iso639-1`) and needs no dependency.

**Alternatives**: `isolang` crate (adds a dependency for ~90 rows). Using Put.io's `language_code` raw (fixture shows
2-letter, but nothing says it always is, and Put.io's own spec text refers to three-letter codes).

## R6. SRT parse, normalise and encoding

**Decision**: `putio-sync-subtitles/src/srt.rs` parses SRT and VTT into cues (`start_ms`, `end_ms`, `text`),
tolerating BOM, CRLF, missing numbers, `.`/`,` millisecond separators, VTT headers, `NOTE` blocks, cue settings
after the time range, and HTML-ish tags (kept). It renders canonical SRT: cues renumbered, `HH:MM:SS,mmm --> ...`,
LF endings, UTF-8 without BOM. Bytes to text: strict UTF-8, else UTF-8 with BOM stripped, else UTF-16 by BOM, else
`encoding_rs` WINDOWS_1252 as the fallback (the usual encoding for Western subtitles; the language-specific
legacy codepages, such as 1250/1251/1253/1254/1255/1256/1257 and CJK, are chosen from a small per-language table
using the already-mapped language). A result with no cues, or with more than 5% replacement characters, is a failed
candidate (FR-007: next candidate).

`encoding_rs` is the one new direct dependency of the subtitles crate. It is already in `Cargo.lock` (0.8.x,
transitive through reqwest), so no new crate is downloaded. Everything else is std.

**Rationale**: Put.io is not documented to normalise encoding, and files from the folder source are whatever the
release shipped. FR-007 and the spec edge case require UTF-8 SRT output.

**Alternatives**: `chardetng` charset detection (new crate; a per-language table covers the common cases;
revisit if bad output is reported). Using the `srtlib`/`subtp` crates (small, weakly maintained, and the VTT and
leniency rules above are the whole point).

## R7. Candidate selection

**Decision**: Per wanted language (config order), among unflagged candidates whose mapped `language_code` equals it:
rank by source: folder or embedded (0), `opensubtitles` (1), anything else (2); ties keep Put.io's list order.
Try them in order; a download, decode or parse failure moves to the next (FR-007); each failure is logged with the
key and reason. The first success is written. Source label in the timeline and metrics: embedded to
`putio_embedded`, folder to `putio_folder`, opensubtitles to `opensubtitles`, and an unknown source to
`opensubtitles` with a warn log (cardinality stays fixed).

Source string detection is by substring, case-insensitive: contains `mkv`, `embedded`, `track`, `stream` is
embedded; contains `folder`, `file`, `sidecar` is folder; contains `opensubtitles` is OpenSubtitles. These strings
are unconfirmed (R1), hence substring matching and the unknown path.

**Rationale**: FR-005 and acceptance scenario 1.2.

**Alternatives**: Using Put.io's `default` field as the pick (it is one subtitle in the user's preferred language,
not per wanted language; ignored except as the final tie-break when it is a candidate of the wanted language).

## R8. When transcription runs

**Decision**: Transcribe a video only when: transcription is enabled; the video has no unflagged subtitle in any
wanted language after the Put.io pass (existing sidecar or just fetched); and ffmpeg is found. The result is saved as
`<video>.<detected>.srt` even if that language is not wanted (acceptance 2.2); if a file with that name already
exists it is kept and the step ends "already present". No existing file is ever overwritten (SC-005): every write is
to `.<name>.part` in the same directory, then `link`-style rename that fails if the target exists (checked right
before, using `create_new` on the final path is not possible with rename, so the step re-checks existence
immediately before `rename` and, on a race, deletes its part file).

**Rationale**: FR-009: "no wanted-language subtitle was found". With two wanted languages and one found, the video
already has a subtitle for translation (026) to start from, so a second transcription would cost minutes for
little. This reading is stated in the spec's own scenario 2.2 and needs no spec change.

**Alternatives**: Transcribe once per missing language (wasteful: Whisper outputs the spoken language only).
Transcribe when any wanted language is missing (would run on nearly every video with two languages configured).

## R9. How 025 plugs into the 038 pipeline

**Decision** (against the real pipeline code):

1. `StepKind` gets two variants, `SubtitleScan` and `Subtitles` (storage names `subtitle_scan`, `subtitles`).
   `SkipReason` gets `NoVideo` (`no_video`). Stage is unchanged: `Stage::Subtitles` already exists and is in the
   telemetry STAGES list.
2. `plan::plan` adds one `SubtitleScan` step (name `videos`, optional, `changes_files = true`) to
   `Stage::Subtitles` when `subtitles.languages` is non-empty and the item's category is not excluded. Otherwise the
   stage gets no steps and shows as skipped `not_configured` or `category` (the existing planner skip reasons;
   `plan::applies_to` is the documented per-category hook point). The step list is fixed at plan time, so the scan
   step is a placeholder that is expanded at run time (below).
3. The runner's `run_step` today handles only Hook steps. It gains a dispatch on kind:
   `SubtitleScan` and `Subtitles` call into a new `putio-sync-pipeline/src/subtitles.rs`. The attempt-finish code
   (record attempt, retry or give up) is factored out of the Hook path so both share it; the unused `_hook` param of
   `failed()` is dropped.
4. `SubtitleScan` runs: locate the item's files (`locate::list_files`), apply R4, and for each video call a new
   `store.insert_steps(item, stage, branch, steps)` which inserts one optional `Subtitles` step per video in the
   same line (branch), after the scan step, with name = relative path. The scan step ends `passed` with the message
   "N videos". With zero videos the stage ends with the scan step `skipped` / `no_video`. The line runs steps one
   after the other, which gives sequential videos for free (and `Semaphore(1)` makes it global).
5. `Subtitles` step: R7/R8 flow for one video. Optional, so 038's `[pipeline] optional_retries` apply (60, 300,
   600 s, then given up and the item ends `done_with_warnings`). Because steps are inserted once, a restart in the
   middle re-runs only the unfinished videos, and each is idempotent (FR-003, SC-005).
6. `finalize()` counts warnings for Hook-kind failed steps only; it is extended to any failed optional step.
   `refresh_gate()` marks an item pending for Hook-kind unsettled steps; it is extended to the two new kinds, since
   they change files and Radarr/Sonarr must wait (FR-004). `Settings::staging_on` (Auto) is true only when an
   enabled hook exists; it also becomes true when subtitles are configured, so subtitles are written in the staging
   folder and arrive with the video in the single final rename. Writes go to the staging path via the existing
   `PathRedirect` / `locate` helpers.
7. Telemetry: `steps::KINDS` grows to `["hook","download","notify","subtitle_scan","subtitles"]`.
8. The daemon injects an `Arc<dyn SubtitleSource>` (list + download, implemented for `PutioApiClient` in the
   subtitles crate) and a `TranscriberFactory`-free plain config into `Pipeline` at construction, as it does for the
   download manager. Tests inject a fake source.

**Rationale**: Honors "plug in as the built-in Subtitles stage, not a parallel mechanism". Timeline, retries,
cancel, gate, staging, metrics and the Active page all come with it. Step insertion at run time is needed because
the number of videos is unknown at plan time (a season pack); the only missing store API is `insert_steps`.

**Alternatives**: One step per item that loops over videos (simpler, but one failing video would retry all, the
timeline cannot show one line per video, which acceptance 1.5 requires). Planning one step per video at plan time
(possible only when the file list is known, which for transfers it is not until the download ends).
A separate subtitles task in the daemon (parallel mechanism: explicitly refused).

## R10. Post-download delete waits for subtitles (FR-006)

**Decision**. Real code: `putio-sync-download/src/manager.rs` `process_queue` deletes the Put.io file immediately
after `download_file` succeeds when `config.effective(&relative_path).delete_after_download` is set, via
`self.client.delete_files(vec![file_id])`. There is no wait for the pipeline. Design:

1. New trait `RemoteDeleteGate` in the download crate (set with `DownloadManager::set_delete_gate`, the same pattern
   as `set_redirect`/`PathRedirect`). `process_queue` asks `gate.defer(file_id, &relative_path)` before deleting. A
   `Some(item_key)` answer means: do not delete now, record a row in the new table
   `pipeline_deferred_deletes(file_id PRIMARY KEY, item_key, created_at)`, log it and continue. `None` deletes as
   today. The CLI one-shot `download` command has no pipeline and no gate: unchanged.
2. The pipeline implements the gate: defer when subtitles are configured for the item's category and the file's
   extension is a video or a subtitle extension (so a season pack's non-video files are not held for nothing). The
   item key is the one the pipeline already uses (`transfer:<id>`, `folder:<path>`, `file:<id>`).
3. The pipeline exposes `releasable_deletes()`: rows whose item is terminal, or past `Stage::Subtitles`, or older
   than a fixed 24 hour failsafe. The daemon's `pipeline_tick_with` calls it each tick, then
   `DownloadManager::delete_remote(ids)`, which re-checks that the `downloads` row for that file id is still
   `completed` with a local path (Principle IV) and calls `client.delete_files`, then the pipeline `mark_deleted`
   removes the row. Persisted in SQLite, so a restart does not forget a deferred delete or lose it.
4. A required hook earlier in the pipeline that fails or blocks delays the delete too (the item does not reach the
   Subtitles stage). Documented in the README; the failsafe bounds it.
5. A dry run (`--dry-run`) records nothing.

**Rationale**: Spec FR-006 and acceptance 1.6; Put.io's subtitle list exists only while the file does. A
persisted deferred set is needed because the daemon can restart between download end and subtitle end, and the
manual one-shot path must stay untouched.

**Alternatives**: Skip the delete and let the pipeline delete at the end (moves destructive logic into the pipeline
crate, Principle I). Holding the delete in memory (lost on restart, and a lost delete is a silent disk leak on the
user's Put.io, while a lost wait is a lost subtitle). Making the download wait for the pipeline (blocks the
download queue behind a minutes-long transcription).

## R11. Config, Settings routes, Test button, metrics, logs

**Decision**:

Config (TOML `[subtitles]`, new `putio-sync-core/src/subtitles.rs`, `Config.subtitles`):

```toml
[subtitles]
languages = []                  # ordered, e.g. ["en", "es"]; empty turns the stage off
excluded_categories = []        # e.g. ["radarr"]

[subtitles.transcription]
enabled = false
url = ""                        # http://host:8000  or  http://host:8080/inference
model = ""
api_key = ""                    # shown as ******** in the API; never logged
timeout_secs = 900
language = ""                   # optional hint, e.g. "en"
chunk_secs = 600
ffmpeg_path = ""                # empty: look on PATH
```

Validation: languages mapped, deduped, at most 10, unknown codes rejected with the offending value; `url` must be
`http`/`https` with a host when enabled; `timeout_secs` 10 to 7200; `chunk_secs` 60 to 1800; `ffmpeg_path` must
exist if set (warning, not error, at save time). `api_key` uses a custom `Debug` that prints `<redacted>` and the
`********` mask/unmask pair already used for hooks (`core/src/hooks.rs`). Env override follows the existing TOML+env
mechanism.

Settings routes (pattern of `settings/pipeline`):

- `GET /settings/subtitles` returns the config (key masked) plus `ffmpeg: {found, path}`, `known_languages`, and the
  live/applies-now text.
- `PUT /settings/subtitles` validates, saves, calls `configure_subtitles(&state)` so the pipeline picks it up
  without restart ("Applies now" for new items; running items keep their plan, steps read config at run time so
  language and transcription changes apply to waiting steps too).
- `POST /settings/subtitles/preview` returns what would be written for a sample video name (stem and languages) so
  the operator sees `Movie.en.srt`, `Movie.es.srt` names and the mapping of their codes.
- `POST /settings/subtitles/test` (the Test button, FR-011), body optional unsaved settings (a masked key means
  "use the stored one"). It sends a 3 second sine-tone WAV generated in process (no ffmpeg) to the server and
  returns `{ok, ffmpeg:{found,path,version}, server:{status, took_ms, language, text, error}}`. A tone has no
  speech, so an empty `text` with HTTP 200 is a pass ("server answered, no speech in the sample"). The
  response's `error` is the server's own message, with the key scrubbed.
- Registered in the settings groups list and `assistant.rs` ("subtitles"), UI card in
  `ui/settings/integrations.js`, registered in `ui/settings/sections.js`.

Metrics (Principle V; bounded labels):

- `putio_sync_subtitles_saved_total{source,language}`: `source` in {putio_embedded, putio_folder, opensubtitles,
  transcribed}; `language` is a configured language or `other`.
- `putio_sync_subtitles_transcribed_seconds_total`: audio seconds sent.
- The existing `putio_sync_pipeline_step_*` metrics get the two new kinds (R9.7). Put.io API calls for the two new
  endpoints use the existing `putio_sync_api_calls_total` instrumentation inside the client.

Logs (`tracing`, structured): `subtitle saved` with `key`, `video` (relative path), `language`, `source`, `bytes`,
`forced`/`sdh`; `subtitle candidate failed` with `key`, `candidate`, `reason`; `transcription chunk` with `key`,
`video`, `chunk`, `of`, `took_ms`. The API key is never a field; error strings from the server go through a scrubber
that replaces the key.

**Rationale**: Constitution V, VI, VII; the same shapes as `[pipeline]` and `[hooks]` mean the UI and tests copy
known-good patterns.

**Alternatives**: A knob for 100 MB/sample (R4). Per-language transcription config (none needed). Storing the key in
a separate secrets file (Constitution says config files that hold tokens stay gitignored; hooks already do this).

## R12. US3: "Fetch subtitles" on the Local files page (044)

**Decision**:

- `GET /local/files/{root}/subtitles?path=<relative>`: a preflight for the UI. Returns `{eligible, reason, putio:
  "available"|"gone"|"unknown", transcription: {enabled, ffmpeg}, languages, existing: [{language, name}]}`.
  `putio` is `available` when a downloads row with that local path exists and `GET /files/{id}` still works,
  `gone` on 404, `unknown` when there is no row (a file that did not come from putio-sync).
- `POST /local/files/{root}/subtitles` with `{path}`: validates the root and path with
  `putio_sync_file_manager::local_files::resolve` (the same guard the other routes use), requires a video
  extension, returns `202 {key}` where the key is `manual:<n>`. It registers an ad-hoc pipeline item (source
  `manual`, one `Subtitles` step with the file path as its name; no scan step) so the timeline, retries, metrics,
  cancel and the Active page apply unchanged. A second request for a path that has a non-terminal item returns 409.
  Size and "sample" rules are bypassed; the extension rule and the file-exists check are not.
- `downloads` gets `find_by_local_path` to turn the path into a Put.io id (a new query only).
- UI `ui/pages/files.js`: a "Fetch subtitles" row action on video rows. It calls the preflight first; when
  `putio` is `gone` or `unknown` and transcription is on, it asks "Put.io no longer has this file. Only
  transcription is possible and takes minutes. Continue?" (acceptance 3.2); when neither source is possible it
  shows the reason and does not start. After the POST it links to the Active page.
- Tests in `putio-sync-api/tests/local_files.rs` style, the contract is added to
  `specs/025-subtitles/contracts/api.md` (and a one-line pointer in the 044 contract).

**Rationale**: Reuses the pipeline for retries and visibility, and the 044 route guard for path safety.

**Alternatives**: A synchronous POST that blocks until done (transcription takes minutes; HTTP timeouts). A
separate in-memory job list (a parallel mechanism).

## R13. Database changes

**Decision**: Three changes in `putio-sync-db`:

1. `pipeline_steps` has `CHECK (kind IN ('hook','download','notify'))` and `CHECK (skip_reason IN
   ('not_configured','category','operator','hook'))`. SQLite cannot alter a CHECK, so a migration rebuilds the table
   with the extended lists (`subtitle_scan`, `subtitles`; `no_video`), copying rows, inside a transaction with
   `PRAGMA foreign_keys = OFF` so `pipeline_attempts` is not cascade-deleted, then `foreign_key_check`. 038 is
   Unreleased but the maintainer's edge DB may already have the tables, so the migration is idempotent (checks the
   stored `CREATE TABLE` text) and has a test that seeds old-schema rows and verifies attempts survive.
2. `pipeline_deferred_deletes(file_id INTEGER PRIMARY KEY, item_key TEXT NOT NULL, created_at INTEGER NOT NULL)`.
3. `downloads` query `find_by_local_path(path)`; `pipeline_items.source` has no CHECK, so `manual` needs no
   migration.

**Rationale**: Verified in `schema.rs` (`CREATE_PIPELINE_STEPS_TABLE`) and `lib.rs migrate()`.

**Alternatives**: Dropping the CHECKs forever by storing kind as free text (loses validation). Encoding the new kinds
as `hook` (the schema and code would lie; `hook` steps carry hook ids).

## R14. New crate, dependencies and Dockerfile

**Decision**: New crate `putio-sync-subtitles` (Principle I: it owns the subtitle concern; the pipeline crate
depends on it, it does not depend on the pipeline). Contents: `srt.rs`, `lang.rs`, `naming.rs`, `candidates.rs`,
`transcribe.rs` (HTTP client and chunk join), `audio.rs` (ffmpeg/ffprobe), `engine.rs` (`fetch_for_video`), and the
`SubtitleSource` trait with the `PutioApiClient` implementation. Dependencies: `putio-sync-core`,
`putio-sync-client`, `putio-sync-telemetry` (workspace), `tokio`, `reqwest` (multipart already enabled in the
workspace), `serde`, `serde_json`, `tracing`, `thiserror`, `encoding_rs` (justified in R6; already locked). Dev
only: `mockito` (the client crate's choice), `tempfile` (already used). No new crate is downloaded. The Dockerfile
lists each crate's `Cargo.toml` and a `src` stub for layer caching, so the new crate needs the matching COPY/mkdir/
echo lines, plus `ffmpeg` in the runtime `apk add`.

**Rationale**: The engine is testable without the pipeline (fake `SubtitleSource`, mock transcription server), and
the pipeline stays a scheduler.

**Alternatives**: Putting it in `putio-sync-pipeline` (grows a crate whose job is scheduling and mixes in HTTP and
codecs). Putting it in `putio-sync-client` (that crate is Put.io only; the Whisper client is not Put.io).

## Open questions (not settled)

1. Put.io subtitle download path and `format=srt` parameter (R1): taken from the brief; fallback is the list item's `url`.
2. Exact `source` strings for folder and embedded subtitles (R1, R7): only `opensubtitles` is seen in a fixture.
3. Whether Put.io marks forced or SDH anywhere other than `name` (R5).
4. Whether `languages=` filters folder and embedded sources, and what the list returns without it (Go SDK says it is
   limited to the account's preferred languages, so we always pass `languages=`).
5. Whether Put.io normalises subtitle encoding (R6; handled either way).
6. Whisper servers: speaches `response_format` values and language field; whisper.cpp `/v1/audio/transcriptions`
   support and `verbose_json` language field (R3; fallbacks and the Test button).
7. ffmpeg's size in the alpine 3.19 image (R2), to be measured when the Dockerfile change is built.
8. The Put.io documentation site could not be fetched (SwaggerHub 404); everything above is from the official
   SDKs, not the live API. A first manual run against the real API is in the quickstart's checklist.
