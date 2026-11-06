# Research: Subtitle Translation

Evidence base: the 038 pipeline as implemented (`putio-sync-pipeline/src/{runner,plan,store,model}.rs`,
`putio-sync-db/src/schema.rs`), the API router (`putio-sync-api/src/lib.rs`, `middleware/auth.rs`,
`middleware/ratelimit.rs`) and 025's design (`specs/025-subtitles/*`). Items marked (unconfirmed) cannot be checked
without a live server and are verified by the first manual run (task T074).

## R1. How 026 plugs into 038 and 025

**Decision**

1. `Stage::Translating` already exists (model.rs, telemetry `STAGES`). `StepKind` gains `TranslateScan`
   (`translate_scan`) and `Translate` (`translate`). No new `SkipReason`: "not configured" and "not for this
   category" reuse `NotConfigured` and `Category`; 025's `NoVideo` is reused when the item has no video.
2. `plan::plan` adds one `TranslateScan` step (name `videos`, optional, `changes_files = true`) to
   `Stage::Translating` when `translation.kind != none`, `subtitles.languages` is non-empty and the category is not
   in `translation.excluded_categories`. Otherwise the stage gets no steps and shows as skipped `not_configured`
   or `category` (acceptance US3.1).
3. `TranslateScan` runs at run time: it locates the item's videos with 025's scan routine (video extension, size and
   sample rules, factored into a reusable function by T025), finds the existing sidecars with 025's `naming`, and
   for each video and each wanted language with no sidecar inserts one optional `Translate` step through
   025's `store.insert_steps(item, stage, branch, steps)`, **branch = language code**, name = the video's path
   relative to the item, position in video order. The scan step ends `done` with the message "N subtitles to
   translate" (or "nothing to translate"). The runner already runs different branches of a stage together and the
   steps of one branch in order, so US1 acceptance 2 (two languages in parallel, each with its own progress) needs no
   new scheduler code, and the timeline already draws branches side by side.
4. The `Translate` step does one translation: pick the source (R10), translate (R2 to R5, or R6), write the file
   with 025's no-overwrite writer, record provenance, pass. A target that already exists by then (another tool wrote
   it) passes with "already exists".
5. `finalize()` and `refresh_gate()` already extend to "any failed optional step" and to 025's two kinds; 026 adds its
   two kinds to both lists (T036). `Settings::staging_on` (Auto) is also true when translation is configured, so
   translated files are written in the staging folder and arrive with the video in the single rename.
6. Telemetry `steps::KINDS` grows by `translate_scan`, `translate`.
7. The daemon builds one `Arc<Translator>` (settings + in-flight semaphore) and gives it to `Pipeline`, as 025 does
   with its `SubtitleSource`. Tests inject a fake `ChatBackend`/mock server.

**Rationale**: Same expansion pattern as 025 (R9 there), so the timeline, retries, cancel, gate, staging and metrics
come for free. The step list is only known at run time (which videos lack which languages).

**Alternatives**: One step per language with a loop over videos (one failing video retries all; the timeline cannot
show a line per video). Planning steps at plan time (the file list is unknown until the download ends). Putting the
translation inside 025's `subtitles` step (it is a separate stage in 038 and the spec asks for its own branches).

### What 026 needs from 025 that its design lacks

| # | Gap in 025's design | What 026 does (task) |
|---|---|---|
| G1 | No provenance: after a save, only the attempt message says a file was transcribed. FR-003 asks for "the transcribed subtitle" as a source, and a machine translation must not be used as a source for another language. | New table `subtitle_provenance`; 026 writes `translated` rows, and a small hook in 025's per-video step runner writes the `SavedSubtitle.source` it already holds (T023, T024). Missing rows mean "unknown" (ranked last). |
| G2 | The scan routine (find videos, size/sample rules) lives inside `pipeline/src/subtitles.rs` as scan-step logic. | Factor it into `pub(crate) scan_videos(item) -> Vec<VideoFile>` shared by both scans (T025). |
| G3 | `start_manual_subtitles` registers a `manual:` item for one step kind. | Factor `register_manual_item(path, stage, steps)` and reuse it (T025, T065). |
| G4 | `pipeline_steps` CHECK rebuild is written for exactly 025's two kinds. | Make the rebuild take the kind list, add 026's two (T008). If 026 lands first or together, one rebuild. |
| G5 | `srt::Cue` has `start_ms`, `end_ms`, `text`, no number; `render` renumbers 1..n. | Numbering is positional: "same numbers" (FR-004) means 1..n in order. A source file with gaps in its numbering is renumbered by 025's parser on read; the output follows the normalised source. Recorded as an assumption (open question Q2). |
| G6 | `insert_steps` takes one branch per call. | 026 calls it once per language. No change needed. |
| G7 | 025's `languages` mapper returns canonical codes and names; 026 needs the English name for the prompt. | Use the mapper's name table (`known_languages` already carries `name`); add `lang::name(code)` if absent (T018). |
| G8 | `Existing { language, kind, path }` scan returns forced/SDH files too. | 026 treats only `kind == Main` as present and as a source (forced files are partial). No change. |
| G9 | 025's Settings preview/test routes and groups list are per feature. | 026 registers its own group the same way (T040). No gap. |
| G10 | 025's `applies_to` skip is per `subtitles.excluded_categories`. | 026 has its own `translation.excluded_categories`; same helper. |

## R2. OpenAI-compatible chat request and reply

**Decision**

- **URL**: `POST <endpoint>`. The endpoint is the configured `base_url` with trailing `/` removed, then: if its path
  is empty, append `/v1/chat/completions`; else if the path already ends with `/chat/completions`, use it as is;
  else append `/chat/completions`. So `http://ollama.lan:11434/v1`, `https://api.openai.com/v1`,
  `http://host:1234` (LM Studio) and Open WebUI's `http://host:3000/api` all resolve. (Open WebUI serves
  `/api/chat/completions` (unconfirmed); the Test button shows the resolved endpoint so the operator can fix it.)
- **Headers**: `Content-Type: application/json`; `Authorization: Bearer <api_key>` only when the key is not empty.
- **Body**:

  ```json
  {"model": "qwen2.5:14b", "stream": false, "temperature": 0,
   "messages": [{"role": "system", "content": "..."}, {"role": "user", "content": "..."}]}
  ```

  No `max_tokens` (a limit too small truncates, and hosted "reasoning" models name the field differently).
  Streaming is off: the reply is validated as a whole, and chunks are small.
- **Reply**: `choices[0].message.content` (string). `choices[0].finish_reason == "length"` means truncated: treated
  as an invalid reply (so the chunk is retried and then split). Missing `choices`, `null` or empty content is an
  invalid reply. Before parsing, `<think>...</think>` blocks (qwen3, deepseek-r1) are removed.
- **Errors** (the message comes from `error.message`, or `error` when it is a string, or the first 300 characters of
  the body; the key is scrubbed):
  - connection refused, DNS failure: `unreachable: <reason>`; not retried inside a chunk beyond the chunk's own
    attempts (R4).
  - `401`, `403`: `the server rejected the API key (HTTP 401)`. Fatal for the translation: no chunk retries.
  - `404`: if the message contains "model" and ("not found" or "does not exist" or "not exist"): `model not found:
    <message>`, fatal. Otherwise `endpoint not found (HTTP 404) at <resolved URL>: check the base URL`, fatal. Ollama
    answers `{"error":{"message":"model \"x\" not found, try pulling it first",...}}`; OpenAI answers
    `{"error":{"message":"The model `x` does not exist...","code":"model_not_found"}}`. (unconfirmed for LM Studio,
    which may answer `{"error":"..."}`; both shapes are handled.)
  - `400` with a message that mentions "context", "too long", "maximum" or "token": counts as an invalid reply (the
    chunk is split). Any other `400`: fatal with the message.
  - `429`: see below. `5xx`, timeout, connection reset: a failed attempt of the chunk (R4).
- **429**: wait `Retry-After` (integer seconds; the HTTP-date form is ignored and the default is used), default
  backoff 2, 4, 8 ... up to 60 s, each wait capped at 120 s. The wait releases the in-flight permit. It does **not**
  count as a chunk attempt (spec edge case); it counts toward time: when the sum of 429 waits for one chunk passes
  300 s the translation fails with `rate limited for 300 s`.
- **Timeouts**: `[translation] timeout_secs` per HTTP request (default 180; the Test button uses 15 s), connect
  timeout fixed at 10 s. Redirects are not followed.

**Rationale**: Covers the four server families through the one route the spec names (Assumptions). `temperature 0`
gives the most repeatable replies for format-bound output. Error classes map one-to-one onto what SC-003 says the
Test button must name (URL, model, key).

**Alternatives**: Ollama's native `/api/chat` (spec says not needed). `response_format: json_schema` (Ollama and LM
Studio support differs; line format needs nothing from the server). Streaming (more code, no benefit for validated
chunks). `max_tokens` from the chunk size (truncation risk, parameter name differs by server).

## R3. Prompt and reply line format

**Decision**

- A cue becomes one line `N| text` where `N` is the cue's 1-based position in the whole subtitle (the number that
  will be rendered) and `text` is the cue text with each line break replaced by `<br>`. Cues with no letter
  (`char::is_alphabetic`) such as `♪`, `...` or `[ ]` only are not sent; they are copied as they are (R14).
- System prompt (the exact text; `{...}` are filled in):

  ```
  You translate subtitles from {source_name} ({source_code}) to {target_name} ({target_code}).
  Each line you receive looks like "N| text", where N is the line number.
  Reply with one line "N| translation" for every line you are asked to translate, in the same order, with the same
  numbers. Output nothing else: no explanations, no code fences, no blank lines.
  Never merge, split, drop or add lines. Never change a number.
  "<br>" marks a line break inside a subtitle. Keep it where the break belongs in your translation.
  Keep tags such as <i>, </i>, <b> and {\an8} exactly as they are, around the same words.
  Keep names, numbers, music symbols and sound-effect brackets; translate the words inside brackets.
  Translate for natural reading as a subtitle: short, spoken style. Do not censor.
  {instruction_line}
  ```

  `{instruction_line}` is empty, or `Additional instruction from the operator: <text>` (max 500 characters,
  newlines collapsed).
- User message:

  ```
  Context, for reference only. Do not translate or output these lines:
  101| ...
  102| ...
  103| ...

  Translate these lines:
  104| ...
  105| ...
  ```

  The "Context" block is omitted for the first chunk. Context is the previous 3 cues' **source** text (any kind),
  so chunks do not depend on each other and can run concurrently.
- Reply parsing: remove `<think>` blocks and a surrounding code fence; split on line breaks; a line matches
  `^\s*(\d+)\s*[|｜]\s?(.*)$`. Lines before the first match and after the last expected number are dropped as
  commentary ("Here is the translation:"). A non-matching, non-blank line between two matched lines is a
  continuation of the previous cue: it is joined to it with `<br>` (models that break lines despite the marker).
  `<br>`, `<br/>`, `<BR />` all become `\n`.

**Rationale**: Numbered lines are the most robust shape for small local models; the bar `|` separator avoids the
auto-renumbering and markdown-list behaviour models show with `1.` or `1)`. Source-only context removes the
sequential dependency between chunks, which is what allows parallel requests under the in-flight limit.
`<br>` is a tag models already know to keep, and a collision with a literal `<br>` in a subtitle is harmless (it is a
line break either way).

**Alternatives**: JSON in and out (small models break JSON often and cost more tokens). Whole-file translation (no
progress, context limits, no recovery). Translated context (forces sequential chunks). `⏎` as the marker (rare
token, models drop it more often). Sending only text with no numbers (cannot detect a merged or dropped line).

## R4. Validation, retry and split

**Decision**

A reply for a chunk of cues `[a..b]` (translatable cues only) is **valid** when, after parsing (R3):

1. the set of numbers is exactly the expected numbers, each once, in the expected order;
2. no text is empty (for a source cue whose text was not empty);
3. no number is outside the chunk (a reply that echoes a context line is trimmed of it first, not failed).

A line count or numbering mismatch, `finish_reason = length`, a context-length `400`, a `5xx`, a timeout or a
transport error is a **failed attempt** of the chunk. Rule:

- attempt 1 fails: retry the same chunk (same prompt);
- attempt 2 fails: if the chunk has more than one cue, **split it in half** (by cue count) and give each half its own
  two attempts; halves split again on their own failures, to a **maximum depth of 3** (40 to 20 to 10 to 5);
- a piece that has failed twice at depth 3 (or a single cue that failed twice) fails the translation with the last
  reason ("reply had 38 lines, expected 40" and so on). Nothing is written. Per chunk this is at most 30 requests.
- The step's own retries (038's `optional_retries`, 60/300/600 s) then apply to the whole translation. This is how
  "given up after the step's retries" in FR-011 is read; there is no separate chunk-level retry setting.
- Fatal errors (R2: key, model, URL, other 400) skip all of this and fail at once, since retrying cannot help.
- Pieces that succeeded are kept in memory for the duration of the attempt; a retried step starts over (chunks are
  cheap compared to a stale half file, and nothing partial is ever on disk).

**Rationale**: Matches FR-011 literally (split after the second failure). Depth 3 bounds the cost of a model that
cannot do the task.

**Alternatives**: Lenient repair (align by fuzzy match): produces wrong timings silently, violates SC-001. Unlimited
splitting down to one cue (up to 80 requests per chunk). Persisting partial chunk results across step retries (tables
and invalidation for little gain).

## R5. Chunking, context and parallelism

**Decision**

- `chunk_cues` (default 40, 5..=200) translatable cues per request, plus up to 3 context cues (fixed; FR-010 gives the
  count, not a knob).
- Parallelism has two levels: **branches** (one per target language, run by the 038 runner) and a translator-wide
  **in-flight limit**, a `tokio::sync::Semaphore` of `max_in_flight` permits (default 2, 1..=8) that every chat request
  takes. Chunks of one subtitle are issued concurrently up to that limit (a `JoinSet`), results are placed by cue
  number, so reply order does not matter. A 429 wait gives the permit back.
- The semaphore lives in the `Translator` and is replaced on settings save, as 038 does with its permits; running
  requests keep the old one.
- Progress: `pipeline_steps.progress = cues done / total cues` after each chunk, through the existing `set_progress`.
- The 038 item concurrency (2) still applies to the whole item task.

**Rationale**: A single local GPU serves one or two requests well; the limit stops a 10-language, 3-video item from
opening 30 connections. Keeping one limit across all branches is the simplest way to honour the spec's "translator
limit".

**Alternatives**: A per-branch limit (the total can then exceed what the server handles). Sequential chunks with
translated context (better terminology consistency, but no concurrency and a chunk failure invalidates the later
ones). The spec states no glossary or memory, so consistency across chunks is not promised.

## R6. The callback translator

### Outbound request

`POST <callback.url>` with `Content-Type: application/json` plus the configured headers. Body (fields exactly as
FR-013):

```json
{"job_id": "5b1c...", "source_lang": "en", "target_lang": "es", "video_name": "Movie.2024.mkv",
 "srt": "1\r\n00:00:01,000 --> 00:00:02,500\r\nHello.\r\n\r\n...", "callback_url": "https://nas.lan:8088/translate/callback/<token>"}
```

`video_name` is the file name only (no directories). `srt` is 025's `render` output (UTF-8, `\n` newlines). `job_id` is
a random UUID v4 and names the job in logs; it is not a credential. `callback_url` is sent on every request, even if
the service answers at once.

### Replies

| Reply | Meaning |
|---|---|
| `200` with `Content-Type` containing `json`, or a body starting with `{` | JSON: `{"srt": "..."}` is the result. `{"error": "..."}` fails the job with that message. Anything else fails with "reply had no srt". |
| `200` with any other body | the body is the SRT text |
| `202` | accepted, the result will arrive at `callback_url` (the step parks, R7) |
| other `2xx` | fail: `unexpected HTTP <n> (expected 200 or 202)` |
| `3xx` | fail: redirects are not followed (a redirected POST can leak the headers) |
| `4xx`, `5xx` | fail: `HTTP <n>: <body, 300 chars, key and header values scrubbed>` |
| timeout, connect error | fail: `unreachable: <reason>` / `timeout after <n>s` |

All failures follow the step's optional-retry policy (a re-run creates a new job and a new token). A `200` result is
checked (R9) and saved straight away.

### Token

- 32 random bytes from the OS RNG, as hex (64 characters), built from two `uuid::Uuid::new_v4()` values (122 random
  bits each, so 244 bits; `uuid` is already a workspace dependency, and `getrandom` is already in the lockfile through
  it). The plain token exists only in memory, inside `callback_url`, during the outbound request.
- **Stored hashed**: only `SHA-256(token)` (hex) is persisted (`sha2` is a workspace dependency). The token has enough
  entropy that a plain hash is sufficient (no salt or slow KDF); a database leak does not give a usable URL.
- **Single use**: the accept path is one atomic statement, `UPDATE translation_callbacks SET state='done' WHERE
  token_hash=? AND state='waiting' AND expires_at > ?`. One affected row means this caller won. A second call finds
  `state != 'waiting'` and is refused.
- **One job**: the row binds the token to one step, one source, one target language and one target path. The URL
  has no other parameters, so the token cannot be used for another job (SC-005).
- **Expiry**: `expires_at = created_at + callback.timeout_secs` (default 3600, 60..=86400).

### The unauthenticated route and the auth middleware

- Route: `POST /translate/callback/{token}`. It is registered in `public_routes()` next to `/health` and
  `/transmission/rpc`, so `require_auth` never runs for it; the token in the URL is its only credential (FR-015).
  Constitution VI says only health and metrics MAY be unauthenticated and that endpoints that "change state" MUST
  require authentication. This is recorded in the plan's Complexity Tracking: the credential here is a
  per-job capability (not the operator's API token), the route can only complete one pending translation with
  text that is validated against the source, and putio-sync already serves `/transmission/rpc` with its own auth.
  The constitution is **not** amended by this feature (open question Q5 if the maintainer wants a MINOR bump).
- Failed calls (unknown, expired, used, malformed token) count toward the existing `AuthLimiter` for the client IP
  (`state.limiter`): `check(ip)` runs first (429 with `Retry-After` while limited), `record_failure(ip)` on a refusal.
  A refusal returns the same `404 {"error":"unknown or expired callback"}` whatever the cause (no oracle); the log
  line carries the cause (`unknown`, `expired`, `used`, `bad_token`), never the token.
- Body limit: `DefaultBodyLimit` of 4 MiB on this route only (a 100 KB subtitle fits; larger gets `413`).
- Body: the SRT as text (`text/plain`, `application/x-subrip`, anything not JSON), or JSON `{"srt": "..."}`, or JSON
  `{"error": "..."}` (the service tells us it failed, so the step fails now instead of at timeout). Reply: `200
  {"ok": true}` when accepted and saved; `422 {"error": "..."}` when the body is not a usable subtitle (the token is
  consumed and the job fails, so the step's retry policy takes over rather than leaving the job hanging).
- **No token in logs**: three places could carry it. (1) The API's `TraceLayer` span records the request URI; the
  router gets a `make_span_with` that replaces the path of `/translate/callback/*` with
  `/translate/callback/<redacted>`. (2) `log_redact` gets a rule that masks the segment after `/translate/callback/`
  wherever a line contains it (a service that echoes the URL in an error message, a reverse proxy line in the log
  viewer); attempt output already passes through `log_redact`. (3) Our own logs never include the URL, only
  `job_id`. The plain token is never stored, so a database dump or the timeline cannot show it. The Test button never
  issues a real token (R11).

**Rationale**: A hashed, single-use, expiring, job-bound capability is the standard webhook pattern; it keeps the API
token out of third-party hands. Refusing uniformly avoids telling a scanner which tokens existed.

**Alternatives**: An HMAC-signed stateless URL (cannot be single-use or survive restarts without a table anyway).
Requiring the API token (FR-015 forbids; the token is an admin credential). A query-string token (more often logged by
proxies than a path segment, and the spec fixes the path form). Storing the plain token (no benefit: the URL is never
re-shown). Retrying a callback `422` with the same token (a buggy service could loop; the step retry creates a clean job).

## R7. How a waiting step is represented in the 038 runner

**Decision**

Facts from the runner today: `advance()` runs, per branch, the next step that is `due_steps()` (state `waiting` or
`interrupted` with `next_attempt_at` reached); a `running` step is never due, so an item with only running steps makes
`advance()` return; `spawn_due()` re-spawns `advance()` for every `running` item each tick; `interrupt_running()` turns
every `running` **hook** step into `interrupted` at start (restart safety).

- **Park**: on a `202` the step runner does not end the attempt and does not change the step state. It returns a new
  internal outcome `Parked`, handled like `Continue` by `advance()`. The step stays `running`, its attempt stays open,
  the item stays `running`, and `advance()` ends when nothing else is due, releasing the item's permit and `busy` flag.
  Other branches (other languages) keep going; the next video of the same language waits behind the parked step
  because a branch runs in order.
- **Representation**: no new step state (no CHECK change). "Waiting for callback" is the pair (step `running`, a
  `translation_callbacks` row in state `waiting`). The timeline route joins the table and adds
  `"waiting": {"for": "callback", "since": "...", "expires_at": "..."}` to that step; the Active row shows
  "Translating: <video> (es), waiting for callback". The step's `progress` stays null.
- **Resume when the callback lands**: the API handler calls `Pipeline::accept_callback(token, body, ip)`. It wins the
  atomic update (R6), validates and re-aligns the body (R9), writes the file, ends the open attempt `done`, sets the
  step `done`, records provenance, emits the metrics, then calls `Pipeline::wake()` which notifies the tick loop to run
  `spawn_due()` now (otherwise the next 10 s tick does). `advance()` then sees the branch's next step or finalizes.
  A rejected body ends the attempt `failed` and runs the shared `failed()` path (retry or give up).
- **Race with the 202 reply**: a service may call back before putio-sync has finished reading its `202`. The step
  runner, after the `202`, re-reads the job row; if it is no longer `waiting`, the handler already completed the step
  and the runner returns `Continue` without writing state. Only the handler writes the step's final state.
- **Timeout**: the daemon's `pipeline_tick` calls `Pipeline::expire_callbacks(now)` each tick. For each `waiting` row
  past `expires_at` (same atomic guard, state to `expired`) it ends the open attempt `timeout` with the message
  `no callback within <n> s`, and fails the step **without scheduling an automatic retry** (the retry would wait
  another full timeout, which SC-004 forbids): the step becomes `failed`, the item goes on with a warning like any
  given-up optional step, and the operator can still press Retry. This is a deliberate reading of US2.3 ("fails, like
  any optional step"). The `pipeline.step_failed` event fires.
- **Restart**: `interrupt_running()` changes to leave `running` steps that have a `waiting` job. All other running
  steps (a chat translation mid-run, a POST that never got its reply) become `interrupted` and run again, creating a new
  job; the old row, if any, is marked `expired` at startup so its token dies. Open attempts of parked steps are not
  closed by `interrupt_running()` either (its first statement is adjusted the same way). The tick that follows
  restart expires anything already past due.
- **Cancel** (item deleted or cancelled): the item's cancel path also marks that item's waiting rows `expired`.
- **Hold/pause (039)** does not freeze a parked step; a result that arrives is saved. Only new work is paused.

**Rationale**: Reuses the existing state machine untouched; the one behavioural change is a narrow carve-out in
`interrupt_running()`. No task, permit or connection is held while waiting, so a 1 hour wait costs nothing.

**Alternatives**: A task that sleeps on a `Notify` until the callback (holds an item permit for up to 24 h and dies on
restart). A new `waiting_callback` step state (third CHECK rebuild, a new arm in every match on `StepState`, the 038
UI and gate). Polling the service instead of a callback (spec asks for a callback).

## R8. Persistence

**Decision**: Two new tables (full DDL in [data-model.md](data-model.md)).

- `translation_callbacks`: one row per callback job. Primary key `token_hash`. Columns for the job id, the step and
  attempt it belongs to (`step_id` references `pipeline_steps(id)` with `ON DELETE CASCADE`, so pruning an item
  removes its rows), languages, the source file path and its SHA-256 (the source is re-read and compared when the
  result lands: a changed source refuses the result), the target path, `state` (`waiting`, `done`, `failed`,
  `expired`), `expires_at` (unix seconds) and `finished_at`. Used and expired rows stay until the item is pruned, so
  a replay can be logged as `used` and not `unknown`.
- `subtitle_provenance` (R10).
- The 038 `pipeline_steps` CHECK on `kind` is rebuilt again (025 does the first rebuild); the rebuild is idempotent and
  keeps `pipeline_attempts`.
- Chat translations need no table: their state is the step.

**Rationale**: FR-017 needs the job to survive; everything else already persists in the 038 tables.

**Alternatives**: Storing jobs as JSON in `pipeline_attempts.output` (not queryable by token, no atomic guard).

## R9. Timestamp re-alignment

**Decision**

Applies to every result that comes from outside (callback `200` bodies and callback deliveries); chat output is built
from the source's timings by construction.

1. Parse the result with 025's `srt::parse` (accepts SRT and VTT, BOM, CRLF, encodings).
2. If the cue count differs from the source: **refuse** (`result has 118 cues, the source has 120`). Nothing is saved.
3. If every cue's `(start_ms, end_ms)` equals the source's: accept.
4. Otherwise (counts match, times differ): output cue `i` = source timing `i` + result text `i`. The number of cues
   whose timing differed (by any amount) is logged as `realigned` and shown in the step message ("realigned 12
   cues"). No tolerance threshold: the spec says the source's times always win when counts match.
5. A source cue with text but an empty result text is refused (`cue 57 is empty`).
6. **Invariant before any write** (all translators): `verify_same_timing(source, output)`: same length and identical
   start/end for each index. A failure is a bug, not a user error: the step fails with `internal: timing mismatch`
   and nothing is written (FR-004, SC-001). A property test feeds random cue lists.
7. Output goes through 025's `render` (renumbered 1..n, the same as the source's normalised numbers, G5) and its
   no-overwrite writer, UTF-8, no BOM.

**Rationale**: Position-by-position replacement is the only repair that cannot drift. A count mismatch cannot be
repaired without guessing.

**Alternatives**: Fuzzy alignment by time overlap (guesses, and can shift whole blocks). A tolerance below which times
count as "same" (changes nothing in the output, so it adds no value).

## R10. Source subtitle choice and provenance

**Decision**

Candidates are the video's sidecars with kind `Main` (025 `naming`), in a language other than the target. Order:

1. the first **wanted** language (config order) whose file was not produced by translation;
2. a file with provenance `transcribed`;
3. any other file not produced by translation, in language-code order (so the choice is repeatable);
4. a file produced by translation (provenance `translated`), only when nothing else exists.

No candidate: the step passes with `no source subtitle` (nothing to translate; the item is not a failure). A file with
no provenance row, or whose size differs from the recorded `bytes`, counts as "not translated, not transcribed".

Provenance (`subtitle_provenance`: path, language, source, bytes) is written by 026 when it saves a translation
(`translated`) and by 025's step after each save (the source it already knows: `putio_embedded`, `putio_folder`,
`opensubtitles`, `transcribed`), see gap G1.

**Rationale**: FR-003's order, plus one refinement: a translation is never chosen as the source while an original
exists, so errors do not compound (a Japanese file translated from a Spanish file translated from English).

**Alternatives**: Ranking by file name or language only (cannot tell a transcript, which is the best fit for
"the one nearest the audio", from a Bazarr file). Leaving provenance out and ignoring "transcribed" (departs from
FR-003).

## R11. Config, settings, Test button, callback base URL

**Decision**

Config `[translation]` in `putio-sync-core/src/translation.rs`, field list and ranges in
[contracts/config.md](contracts/config.md): `kind` (`none` default, `chat`, `callback`), `excluded_categories`,
`max_in_flight`, `timeout_secs`, `chunk_cues`, `instruction`, then `[translation.chat]` (`base_url`, `model`,
`api_key`) and `[translation.callback]` (`url`, `headers`, `callback_base_url`, `timeout_secs`). `api_key` and header
values use the `MASK` / unmask pair from hooks, and a `Debug` that prints `<redacted>`.

Routes (pattern of `settings/subtitles`, contract in [contracts/api.md](contracts/api.md)):

- `GET /settings/translation`, `PUT /settings/translation` (validates; "applies now": new items and waiting steps; a
  running translation finishes with the settings it started with; `configure_translation(&state)` swaps the in-flight
  semaphore and settings in the `Translator`).
- `POST /settings/translation/test`: body optional unsaved settings (masked values mean "use the stored one").
  - **chat**: sends a 2-line sample (`1| Good morning.` and `2| Where are you going?<br>It is late.`) to Spanish
    (or `sample_target`) through the real engine path, 15 s timeout. `ok` is true when the reply passes R4
    validation. The result carries the resolved endpoint, `took_ms`, the translated lines, and on error a `problem`
    in {`url`, `key`, `model`, `format`, `timeout`, `server`} so the UI names what is wrong (SC-003). On
    `model` the test also asks `GET <base>/models` (5 s, best effort) and returns up to 20 model ids so the
    operator can pick the right one (Ollama and OpenAI serve it).
  - **callback**: first checks the callback base URL is set (`problem: "callback_base_url"`), then does two
    things: (a) `GET <callback_base_url>/health` from putio-sync itself, reported as `base_url_reachable_from_here`
    (it proves the address is spelled right and routed from this host, **not** that the service can reach it); (b)
    POSTs the 1-cue sample job to the service with `job_id` `test-<uuid>` and a `callback_url` of
    `<base>/translate/callback/test` which has no token behind it, so a service that really calls back is refused.
    `ok` is true when the service answers `200` with a parseable subtitle or `202`; `result` is `"immediate"` or
    `"will_call_back"`. The Test never creates a `translation_callbacks` row.
  - Always HTTP 200 for a completed test; `400` only when the settings are invalid for that kind.
- **Callback base URL (FR-016)**: `callback.callback_base_url` is required whenever `kind = callback`; `PUT` refuses
  with `400 {"error": "callback.callback_base_url: required when kind is callback"}`, so callback mode can never be
  saved without it. Rules: `http` or `https`, a host, no user info, no query or fragment, a path prefix is allowed (a
  reverse proxy), trailing `/` removed. The Settings card shows the final URL form with a placeholder token, and a
  "Use this address" button that fills the field from `window.location.origin`. It is a separate setting because
  putio-sync may listen on `0.0.0.0` and cannot know its LAN, tailnet or public name (037, 036).
- Not configurable on purpose: context cue count (3), split depth (3), the 429 budget (300 s), the 4 MiB callback body
  limit, the reply format.

**Rationale**: Same shapes as `[subtitles]`, so UI and tests copy known patterns. The Test result names the failing
part instead of echoing a raw error.

**Alternatives**: Deriving the callback base from the request's `Host` header at save time (wrong behind proxies and
stale after an address change). One shared `[translation]` flat table with kind-prefixed keys (harder to mask and
validate per kind).

## R12. Metrics and logs

**Decision** (Constitution V; bounded labels, no file names or ids):

| Name | Type | Labels |
|---|---|---|
| `putio_sync_translations_total` | counter | `kind` (`chat`,`callback`), `result` (`done`,`failed`,`timeout`,`refused`), `language` (configured target or `other`) |
| `putio_sync_translation_chunks_total` | counter | `result` (`ok`,`split`,`failed`) |
| `putio_sync_translation_chunk_retries_total` | counter | none |
| `putio_sync_translation_rate_limited_total` | counter | none (HTTP 429 answers) |
| `putio_sync_translation_seconds` | histogram | `kind` (buckets 1 s to 1 h) |
| `putio_sync_translation_callbacks_waiting` | gauge | none (rows in `waiting`) |
| `putio_sync_translation_callbacks_refused_total` | counter | `reason` (`unknown`,`expired`,`used`,`bad_token`,`bad_body`,`rate_limited`) |
| `putio_sync_pipeline_step_*` (existing) | | `kind` gains `translate_scan`, `translate` |

Logs (structured `tracing`): `translation done` (`key`, `video` relative path, `source`, `target`, `cues`, `chunks`,
`retries`, `duration_ms`, `kind`, `realigned`); `translation chunk retry` (`key`, `video`, `chunk`, `attempt`,
`reason` from a fixed set plus a 200-character scrubbed detail); `translation failed` (`key`, `video`, `target`,
`reason`); `callback sent` (`key`, `job_id`, `target`, `status`); `callback accepted` (`job_id`, `cues`, `realigned`);
`callback refused` (`reason`, `ip`, `job_id` when known). Never a key, a header value, a token or the callback URL.
The `api_key` and header values are also registered with `log_redact::set_secrets` like other secrets, and server error
text passes through a scrubber that replaces them.

**Alternatives**: A `language` label on chunk metrics (cardinality multiplies for no diagnostic gain).

## R13. US3: category switch and the manual "Translate subtitles" action

**Decision**

- Per category: `translation.excluded_categories` (same match as 025's, case-insensitive), applied by the planner (R1.2).
  The Translating stage shows skipped `category`.
- `GET /local/files/{root}/translate?path=<relative>`: preflight. Returns `{eligible, reason, translator: {kind,
  configured}, sources: [{language, name}], targets: [{language, exists}], wanted: [...]}`. `eligible` is false with a
  reason when: not a video extension, translation is `none` or not configured, no sidecar `Main` subtitle exists to
  translate from, or every wanted language already exists and no other target is possible. Targets offered: the wanted
  languages (025) first, then any known language; ones that already have a file are listed with `exists: true` and
  disabled (never overwrite).
- `POST /local/files/{root}/translate` with `{path, targets: ["es","ja"]}`: validates root and path with
  `putio_sync_file_manager::local_files::resolve` (the same guard as 025 and 044), the video extension, 1 to 10 known
  targets, none existing. Registers a `manual:<n>` item (025's `register_manual_item`, G3) with one `Translate` step per
  target (branch = language, name = the file path), returns `202 {"key": "manual:<n>"}`. `409` with the existing key when a
  non-terminal manual item exists for the path. Size and sample rules are bypassed; the extension rule is not.
- UI: a "Translate subtitles" row action on video rows in `ui/pages/files.js` (next to 025's "Fetch subtitles"). It opens
  a small chooser of target languages, posts, then polls `GET /active/{key}/timeline` every 3 s (a stop after the item is
  terminal) and shows the step message ("es: saved Movie.es.srt") and refreshes the listing, so the result "is shown on
  the page" (US3 acceptance 2) without leaving it.

**Rationale**: Same mechanics as 025's manual action; retries, cancel and the timeline come from 038.

**Alternatives**: A synchronous POST (translation takes minutes; a callback can take an hour). Letting the manual action
overwrite an existing file (FR-002 and 025's no-overwrite rule say never).

## R14. Tags, formatting and special cues

**Decision**

- Line breaks: R3. A cue that loses or gains a line break is accepted.
- Tags kept by the prompt. After a valid reply, `restore_wrapping(source_text, output_text)`: the leading run of
  tags in the source (`<...>` or `{...}` tokens, with nothing between them) is prepended to the output if the output does
  not start with it; the trailing run of tags is appended if the output does not end with it. Only whole-cue wrappers are
  restored (`<i>...</i>`, `{\an8}...`); tags in the middle of a cue are left to the model.
- A cue with no alphabetic character is copied as it is and not sent (deviation from the spec's "are sent as they are":
  the saved file is identical and tokens are saved, and the context block still shows them to the model). Sound-effect
  brackets with words (`[door slams]`) are sent and may be translated.
- Right-to-left targets: text saved as returned, UTF-8, no bidi marks added.
- Commentary and code fences are removed by the reply parser (R3); a reply with commentary between numbered lines is
  handled as continuation or fails validation and is retried.
- The subtitle text is untrusted input to the model. Output is only ever taken as text for known cue numbers after
  validation, so an injection can change a cue's words but nothing else.

## R15. New crate, dependencies, Dockerfile

**Decision**: New crate `putio-sync-translation`. Dependencies: `putio-sync-core`, `putio-sync-subtitles` (SRT, language
table, naming), `putio-sync-telemetry`, `tokio`, `reqwest` (workspace, `json`), `serde`, `serde_json`, `tracing`,
`thiserror`, `sha2`, `uuid` (v4). All are already in `Cargo.lock`; **no new dependency**. Dev: `mockito`, `tempfile`.
The Dockerfile lists each crate's `Cargo.toml` and a `src` stub, so it gets matching lines. No system tool is needed.

**Rationale**: Principle I (concern boundaries), VII (no new dependency). 025's crate is not extended because it is
under construction on another branch and is a codec/engine crate; 026 only depends on its public API (`srt`, `lang`,
`naming`).

**Alternatives**: A module inside `putio-sync-subtitles` (cross-branch conflicts, more HTTP in that crate). Hand-written
hex for `sha2` output is not needed: `format!("{:x}", ...)`.

## Open questions (not settled)

- **Q1** (unconfirmed): exact error bodies of LM Studio and Open WebUI for a missing model; both shapes are handled, the
  first manual run records the real ones in `contracts/chat.md` (T074).
- **Q2**: Numbering. 025's parser renumbers 1..n, so a source with gaps in its numbering comes out renumbered.
  FR-004 says "numbering exactly". Treat the normalised numbering as the source's? (Assumed yes.)
- **Q3**: Reasoning models that reject `temperature` (some hosted APIs answer 400 for it). Planned: none. If it shows up
  in the first run, retry once without the field on a 400 that names it.
- **Q4**: Ollama's default context window can silently truncate a 40-cue prompt (the reply then fails validation and
  splits). The quickstart tells the operator to raise `num_ctx`; the Test sample is too small to show it. Should Test
  send a 40-line probe? (Not planned: slow on small boxes.)
- **Q5**: Constitution VI says only health and metrics MAY be unauthenticated. The callback route is a third, justified in
  Complexity Tracking. Does the maintainer want a MINOR constitution amendment or is the note enough?
- **Q6**: Callback timeout failure skips the automatic optional retry (R7). If the maintainer prefers the strict
  "like any optional step" reading (retry after 60 s), it is a one-line change in `expire_callbacks`.
