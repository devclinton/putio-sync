# Data Model: Subtitle Translation (026)

## Config (core, `putio-sync-core/src/translation.rs`)

`TranslationConfig`
- `kind: TranslatorKind` (`none` default, `chat`, `callback`)
- `excluded_categories: Vec<String>` (case-insensitive match; at most 50)
- `max_in_flight: u32` (2, 1..=8)
- `timeout_secs: u64` (180, 10..=1800): per HTTP request, both kinds
- `chunk_cues: u32` (40, 5..=200)
- `instruction: String` (empty, at most 500 characters)
- `chat: ChatConfig`
- `callback: CallbackConfig`

`ChatConfig`: `base_url: String`, `model: String`, `api_key: String` (masked `********`, `Debug` prints `<redacted>`).

`CallbackConfig`: `url: String`, `headers: BTreeMap<String, String>` (values masked; at most 20; names are valid header names),
`callback_base_url: String`, `timeout_secs: u64` (3600, 60..=86400).

Validation per kind: see [contracts/config.md](contracts/config.md). Target languages are not config here: they are
`[subtitles] languages` (025).

## Engine types (`putio-sync-translation`)

- `Line { number: u32, text: String }`: a cue as sent (text uses `<br>` for line breaks).
- `ChunkPlan { first: usize, last: usize, context: Range<usize>, depth: u8 }`: indexes into the translatable cue list.
- `ReplyError`: `WrongLineCount{expected, got}`, `BadNumbering{at}`, `EmptyText{number}`, `Truncated`, `NoContent`.
- `AttemptError`: `Invalid(ReplyError)`, `Http{status, message}`, `Timeout`, `Unreachable(String)`, `RateLimitedOut`.
- `Fatal`: `Key`, `ModelNotFound(String)`, `EndpointNotFound(String)`, `BadRequest(String)`.
- `TranslateError`: `Fatal(Fatal)`, `Chunk{reason: ReplyError|AttemptError, depth}`, `Source(String)`, `Refused(String)`.
- `Translated { cues: Vec<Cue>, chunks: u32, retries: u32, realigned: u32 }`: output cues share the source's
  timings (invariant `verify_same_timing`).
- `SourcePick { path, language, provenance: Option<Provenance> }`.
- `CallbackJob { job_id, token (memory only), token_hash, callback_url (memory only), expires_at }`.
- `CallbackReply`: `Immediate(String)`, `Accepted`, `Failed(String)`.
- `Cue` (025): `start_ms`, `end_ms`, `text`. Numbering is positional (G5 in research).

State of a translation (one `translate` step):
`waiting -> running -> done | failed(retry via next_attempt_at) | given up (item done_with_warnings)`;
callback mode adds a **parked** period while `running`: (step `running`) + (job `waiting`).

```
job: waiting --accept (valid)--> done
job: waiting --accept (bad body / error body)--> failed
job: waiting --expires_at passes--> expired
job: waiting --restart of an interrupted step / item cancelled--> expired
```

## Database

### `translation_callbacks` (new)

| Column | Type | Notes |
|---|---|---|
| `token_hash` | TEXT PRIMARY KEY | hex SHA-256 of the token; the token is never stored |
| `job_id` | TEXT NOT NULL UNIQUE | UUID, also sent to the service; used in logs |
| `step_id` | INTEGER NOT NULL REFERENCES pipeline_steps(id) ON DELETE CASCADE | the parked step |
| `attempt_id` | INTEGER NOT NULL | the open `pipeline_attempts` row |
| `source_lang` | TEXT NOT NULL | canonical code |
| `target_lang` | TEXT NOT NULL | canonical code |
| `source_path` | TEXT NOT NULL | the subtitle that was sent |
| `source_sha256` | TEXT NOT NULL | the result is refused when the source changed |
| `target_path` | TEXT NOT NULL | where the result is written (staging path while staging) |
| `state` | TEXT NOT NULL CHECK (state IN ('waiting','done','failed','expired')) | |
| `created_at` | INTEGER NOT NULL | unix seconds |
| `expires_at` | INTEGER NOT NULL | `created_at + callback.timeout_secs` at creation |
| `finished_at` | INTEGER | |

Index on `(state, expires_at)` for the expiry scan and on `step_id`.

### `subtitle_provenance` (new, used by 025 and 026, see G1)

| Column | Type | Notes |
|---|---|---|
| `path` | TEXT PRIMARY KEY | absolute path of the saved sidecar (final path, re-keyed by the staging rename: see T036) |
| `language` | TEXT NOT NULL | canonical code |
| `source` | TEXT NOT NULL CHECK (source IN ('putio_embedded','putio_folder','opensubtitles','transcribed','translated')) | |
| `bytes` | INTEGER NOT NULL | a file whose size differs is treated as unknown |
| `created_at` | INTEGER NOT NULL | |

Rows older than 90 days are pruned with the pipeline prune; a missing row is never an error.

### `pipeline_steps` (changed)

`kind` CHECK gains `translate_scan`, `translate` (second rebuild after 025's, same idempotent method). Translate steps
use `branch` = target language code, `name` = the video's path relative to the item (the file path for manual items),
`required = 0`, `changes_files = 1`. The scan step is named `videos`. `skip_reason` needs no change.

### `pipeline_items.source`

Reuses 025's `manual` value; key `manual:<n>`.

## Metrics

See research R12. `kind` is `chat` or `callback`; `language` is a configured target or `other`.
