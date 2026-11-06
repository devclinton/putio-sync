# Research: Subtitle Matching and Source Modes (050)

"Confirmed" means seen in a named source. "Unconfirmed" items have a fallback that keeps the feature working,
and are collected at the end. Web sources were checked on 2026-11-05; the OpenSubtitles Stoplight pages render
only partly for automated readers, so only names of fields were readable there, not their descriptions.

## R1. Is Put.io's subtitle list hash-based? Is `opensubtitles_hash` needed?

**Decision**: Keep Put.io's list (025) as the first stop, unchanged. Do not use Put.io's `opensubtitles_hash` for
anything. Compute the OpenSubtitles hash locally from the video file and send it to the OpenSubtitles API
(`moviehash`) as the first OpenSubtitles level.

**Evidence (confirmed)**: `putdotio/putio-sdk-typescript` `src/domains/files.ts`: `listFileSubtitles(fileId,
{languages})` is `GET /v2/files/{fileId}/subtitles`; the only identifier sent is the file ID. `opensubtitles_hash` is
a nullable string on `FileBaseSchema` and is used nowhere else. The Go SDK documents the lookup order inside Put.io
(subtitle file in the folder, then a track in the MKV, then the first OpenSubtitles.org match) without saying what
it matches on. 025 research R1 recorded the same.

**Conclusion**: the client cannot ask Put.io for "a subtitle by this hash"; Put.io may or may not use the hash
internally (**unconfirmed**). The roadmap's level 1 is met as follows: Put.io's list stays first (it is what the
roadmap calls Put.io's lookup), and the exact-file match against OpenSubtitles is the new fingerprint search.

**Rationale for computing locally**: the OpenSubtitles hash is the file size plus the 64-bit little-endian sums of the
first and last 64 KiB (the standard OpenSubtitles algorithm, from general knowledge, not re-fetched; a known-answer
unit test with a published sample vector is part of T012). Computing it costs two 64 KiB reads, needs no Put.io call (the existing
`FileEntry` would need a `GET /files/{id}` per video, and the file is often deleted), works for Local files
(044) and for files outside Put.io, and the result equals Put.io's value for the same bytes. Files under 128 KiB are
skipped.

**Alternatives**: Reading `opensubtitles_hash` from Put.io (an extra call, absent after delete, no help for local
files). Hash both and compare (no value; they are the same bytes).

## R2. OpenSubtitles REST API shape (api.opensubtitles.com)

**Decision**: Add a small client `opensubtitles.rs` in `putio-sync-subtitles` using the existing `reqwest`. Base URL
`https://api.opensubtitles.com/api/v1` (configurable only in tests via a constructor, as `PutioApiClient::with_base_url`).

**Confirmed (names)**: Stoplight "Search for Subtitles" lists the query parameters `imdb_id`, `tmdb_id`,
`parent_imdb_id`, `parent_tmdb_id`, `season_number`, `episode_number`, `moviehash`, `moviehash_match`, `languages`,
`type`, and the response attributes `release`, `files`, `file_id`, `moviehash_match`, `download_count`, `ratings`,
`hearing_impaired`, `feature_details`. Secondary sources (a developer guide on apidog.com, an IINA plugin issue)
confirm: header `Api-Key` on every request, a required `User-Agent`, optional `POST /login` returning a JWT that is
sent as `Authorization: Bearer`.

**Taken from general knowledge of the API, not re-read on a page, so unconfirmed**:

- Search: `GET /subtitles?...` returns `{total_count, data:[{id, attributes:{language, release, hearing_impaired,
  foreign_parts_only, machine_translated, ai_translated, moviehash_match, download_count, ratings, from_trusted,
  feature_details:{feature_type, year, title, imdb_id, tmdb_id, season_number, episode_number, parent_imdb_id,
  parent_tmdb_id}, files:[{file_id, file_name}]}}]}`. The `imdb_id` parameter accepts the number without `tt`.
  For an episode the series IDs are sent as `parent_imdb_id` / `parent_tmdb_id` with `season_number` and
  `episode_number`; for a movie, `imdb_id` / `tmdb_id`. Parameters are sent in lowercase alphabetical order and the
  API redirects when they are not (a client that follows redirects copes).
- Filter parameters: `languages` (comma list, ISO 639-1), `type` (`movie`|`episode`), `hearing_impaired`
  (`include|exclude|only`), `foreign_parts_only`, `machine_translated` and `ai_translated` (`include|exclude`),
  `query` (free text).
- Download: `POST /download` with JSON `{file_id, sub_format:"srt"}` and the `Api-Key` header, plus the JWT for a
  logged-in user; returns `{link, file_name, requests, remaining, message, reset_time, reset_time_utc}`. `link` is
  a short-lived URL fetched with a plain GET (no auth). A used-up quota answers HTTP 406 with `remaining: 0`.
- Login: `POST /login` `{username, password}` returns `{token, user:{allowed_downloads, level, ...}, base_url}`;
  the JWT is valid about 24 hours; `base_url` may name a different host (VIP) that later calls should use.
- Rate: about 5 requests per second per IP, `429` with `Retry-After` when exceeded.

**Quota (confirmed that figures conflict)**: sources disagree (5 per day without login in a 2024 thread; 20 and
200 per day for free logged-in accounts in a forum post and in an integration guide; 100 per day in a February 2026
IINA issue; VIP 1000). Decision: **make no assumption in code**: read `remaining` and `reset_time` from every
download answer, store them in memory, stop OpenSubtitles downloads when `remaining` is 0 until the reset. The
Test button shows what the service says.

**Fallbacks**: every response field except the file ID is optional in the parser (`#[serde(default)]`), so a
renamed field gives fewer signals and a lower score, not a failure. The `contracts/opensubtitles-api.md` fixtures are
the single place to correct once the real shape is seen against a live key (task T070).

**Alternatives**: The legacy `rest.opensubtitles.org` API (deprecated, user-agent registration, no per-user
quota, being retired). A third-party crate for the API (small, unmaintained, would still need the above; the
constitution asks for justification and the client is about 200 lines). Scraping (no).

## R3. Where the IDs come from (Radarr/Sonarr)

**Decision**: A trait `MediaLookup` (in `putio-sync-core`, since two crates share it) and an implementation in
`putio-sync-arr`, injected by the daemon (the pattern of `SubtitleSource`). The lookup key is the Put.io file ID the
pipeline already resolves for the video (`putio_id_of`), plus the category. The arr crate maps the file ID to the
tracked item (`arr_items` has `get_by_file_id`, which also covers files inside a tracked folder through
`arr_item_files`) and so to its info hash, then calls the *arr that owns the category:

- Radarr: `GET /api/v3/history?downloadId=<HASH>&includeMovie=true&pageSize=...` (the grab event carries `movieId`,
  `movie.imdbId`, `movie.tmdbId`, `movie.year`, `movie.title`).
- Sonarr: `GET /api/v3/history?downloadId=<HASH>&includeSeries=true&includeEpisode=true` (records carry
  `series.imdbId`, `series.tmdbId`, `series.tvdbId`, `series.title`, `episode.seasonNumber`, `episode.episodeNumber`).

`downloadId` is the torrent info hash in upper case. putio-sync is the download client, so the hash is the one
the *arr recorded. **Unconfirmed on a live instance**: the exact parameter names and that history keeps the grab
event after import. The web search did confirm that Radarr's history endpoint takes `downloadId` and
`includeMovie` and that its records carry `downloadId` (generated clients on docs.rs). `includeSeries` and
`includeEpisode` for Sonarr, and the `imdbId`/`tmdbId` fields in the embedded objects, are from general knowledge.
**Fallback**: `GET /api/v3/movie/{movieId}` and `GET /api/v3/series/{seriesId}`, `GET /api/v3/episode/{id}`
using the IDs on the history record; and if everything fails, the name level.

What putio-sync already has: `putio-sync-arr` reads only `GET /api/v3/downloadclient` (categories and path
checks) and knows each server's URL and API key (`ArrServer`, `ArrApi`); it fetches no media information today.
`arr_items` stores hash, name, category, transfer ID and Put.io file IDs. A new method `ArrApi::history_by_download`
is the only new call. Results are cached in memory by hash for the life of the process and for 1 hour (an item's
pipeline run), including "not found".

Season packs: history has one record per episode; the video's own episode is found by parsing its file name
(`SxxEyy`, R5) and matching it to the records' season and episode. No parse, or no matching record: the
IDs level is skipped for that video (name level still applies).

Category to server: the *arr whose `ClientInfo.categories` contains the item's category; otherwise every configured
server of any kind is tried, Radarr's answer is used for a movie-like (no `SxxEyy`) video and Sonarr's for the rest.
At most one lookup per server per item.

**Rationale**: History by `downloadId` is a single call per item and works for movies and for every episode
of a pack. Alternatives ask the user for IDs (no) or parse the name only (loses the IDs, which are the roadmap's
level 2).

**Alternatives**: `GET /api/v3/queue` (the item has left the queue by the time the subtitle stage runs after
import? Not so in Transmission mode, where the *arr waits, but the queue entry can be gone for completed
downloads; history is more durable). Reading the *arr database (no). Webhooks from the *arr into putio-sync
(a new inbound integration and per-user setup; rejected for simplicity).

## R4. Moviehash first, then IDs, then name: ordering and early exit

**Decision**: Per video, after Put.io's pass (025, unchanged), for each language still missing and whose
mode downloads, one OpenSubtitles pass in this order, stopping per language at the first level that yields a
candidate at or above the threshold:

1. Fingerprint: one search with `moviehash` and all missing languages (`languages=en,es`). Only results with
   `moviehash_match: true` count (confidence 100).
2. IDs: one search with the *arr IDs (and season and episode), all missing languages.
3. Name: one search with the parsed title (as `query`), year or season and episode, `type`, all missing languages.

A level is skipped when its inputs are missing. One search request per level covers every missing language
(results carry `language`), so a video costs at most three searches plus the downloads. Searches are paced to at
most 4 per second process-wide.

**Rationale**: Fewer requests (the API is rate limited and the quota counts downloads, not searches), and a
fingerprint hit stops the more uncertain levels.

**Alternatives**: One search per language per level (3x the requests). Merge all levels into one scored list
(the roadmap says "in order of confidence").

## R5. Release-name parser and confidence score

**Decision**: A rule-based parser `release.rs`, std only (no `regex` crate; the workspace should not grow for a
dozen token rules). It lowercases, splits the file name stem on `.`, `_`, ` `, `-`, `[`, `]`, `(`, `)` into
tokens and reads:

| Field | Rule |
|---|---|
| season, episode | first token matching `s\d{1,2}e\d{1,3}` (more `e\d+` tokens ignored), or `\d{1,2}x\d{2,3}` |
| year | first token of four digits 1900 to 2099 that is not the first token (so `2012.2009.mkv` is "2012" the title) |
| resolution | `2160p 4k uhd`, `1080p`, `720p`, `576p`, `480p` |
| source | `remux`, `bluray bdrip brrip bd`, `web-dl webdl` (kept as `webdl`), `webrip`, `hdtv`, `dvdrip dvd`, `hdrip`, `cam`... (joined `web dl` tokens handled) |
| codec | `x264 h264 avc`, `x265 h265 hevc`, `av1`, `xvid` |
| group | the text after the last `-` in the stem when it has no spaces, else a leading `[Group]` |
| title | tokens before the first of season/episode/year/resolution/source marker, joined with spaces |

The same parser reads a candidate's `release` string. Two infos are compared field by field.

**Score** (0 to 100, integers, constants in one place, unit-tested table):

| Part | Points |
|---|---|
| Fingerprint match (`moviehash_match`) | the whole score is 100 |
| Basis: IDs (and season and episode when an episode) agree | 60 |
| Basis: name query; title similarity (normalised tokens, Dice coefficient) times 40, plus 0 to 10 when year (movie) or season and episode (episode) agree; any disagreement of year, season or episode is 0 | 0 to 50 |
| Release group equal | +15 |
| Source equal | +10 |
| Resolution equal | +10 |
| Codec equal | +5 |
| Download count tie-break (log-scaled) | up to +2 (never decides a threshold) |
| Penalty: hearing impaired or foreign parts only when not wanted | candidate excluded earlier |

Maxima: IDs 100 (60+40), name 100 (50+40+... capped at 100). Default threshold `min_confidence = 65`, range 0
to 100: an ID match alone (60) is skipped, an ID match with a matching resolution or source (70, 75) is saved; a
name match needs agreement on the title, the year or episode and at least some release agreement (about 65+).
**These numbers are a first guess**, chosen so that an exact episode with the wrong source and group is not saved
at the default and the right release is; they are constants plus the one setting, and the quickstart tells how to
read the score from the log to tune it.

**Rationale**: The roadmap fixes "group, source, resolution" and "no guessing model". Group is the strongest
signal that timing fits (same encode); resolution the weakest, as the weights show.

**Alternatives**: Machine-learned or fuzzy-matching crates (refused by the roadmap and Principle VII).
Using the service's own `ratings`/`from_trusted` as score input (kept only as a tie-break: they do not say the
timing fits this file).

## R6. Modes and how they plug into the 025 engine

**Decision**: `SubtitleMode` (`download_only`, `download_then_whisper`, `whisper_only`, `both`) in
`putio-sync-core/src/subtitles.rs`. Config:

```toml
[subtitles]
mode = "download_then_whisper"            # global; absent keeps 025's meaning (see below)
[subtitles.category_modes]
anime = "whisper_only"
[subtitles.language_modes]
ja = "both"
```

`SubtitlesConfig::mode_for(category, language)` returns language mode, else category mode (case-insensitive
category), else global. **Compatibility**: `mode` is `Option`; `None` resolves to `download_then_whisper`, which
with `transcription.enabled = false` already behaves as Download only because the engine's transcription gate
(`wants_transcription`) requires `transcription.enabled`. So no migration and an old file round-trips unchanged
(the serializer skips `None` and empty maps). `excluded_categories` still wins. Validation: category and language
keys are mapped through the language/category rules (`lang` table for languages; categories free text), and any
mode that includes Whisper (`whisper_only`, `both`), anywhere in the config, requires `transcription.enabled` and
a valid URL (FR-005); `download_then_whisper` does not (it degrades to Download only as today).

Engine changes (extend, not replace):

- `VideoRequest` gains `modes: &ModePlan` computed once per video: for each wanted language the effective mode,
  plus the mode for "other" (a detected language that is not wanted: category mode, else global).
- Put.io pass (`fetch_for_video`) and the new OpenSubtitles pass run for languages whose mode downloads
  (`download_only`, `download_then_whisper`, `both`); `whisper_only` languages are skipped entirely (FR-004).
  If no language downloads, `source.list` is not called at all.
- Transcription trigger replaces `wants_transcription`: (a) any language in `whisper_only` or `both`: always (once per
  video); (b) otherwise, any `download_then_whisper` language and 025's rule (no wanted language found or already
  present). Never when only `download_only`.
- File name: the transcription takes the `whisper` token when the effective mode of the detected language is
  `both` (the mode for "other" when the language is not wanted), else it is a plain `<lang>.srt`. If the plain
  target already exists, nothing is replaced and the step reports "already present" (SC-002, FR-008).

**Rationale**: The existing gate has one natural place to change; the mapping is a pure function that can be
table-tested. Per-language modes decide what is attempted for that language; Whisper output language is whatever the
audio is, so one transcription per video.

**Alternatives**: A separate settings object per category (more UI, same result). Making `mode` non-optional with
a default (changes the serialized file for everyone; breaks "round trips unchanged").

## R7. Naming and recognizing `.whisper.srt`

**Decision**: Add `SlotKind::Whisper` to `naming.rs`: name `<stem>.<lang>.whisper.srt`. `existing_sidecars` today
excludes tokens `forced`, `sdh`, `hi` from the main slot; add `whisper`, so `movie.en.whisper.srt` is never a main
`en` subtitle (else 025's keep-existing rule would stop a download in Both mode). Public helper
`naming::is_transcription(path) -> bool` (and `language_of`) for 026 and the UI. Provenance beyond the name is not
stored: in Whisper only and Download then Whisper the file is `<lang>.srt` as the roadmap says, so a transcription
there looks like a download by name. The timeline step message and logs say `transcribed`.

**Open (see Open questions)**: whether 026 needs provenance for those too. Cheapest follow-up if so: a one-line
header cue or a `.whisper` token in all modes (a roadmap change); not done here.

**Radarr/Sonarr import**: they parse language and flags from the tokens between the video name and extension;
`whisper` becomes the "title" part of the extra file in current versions (**unconfirmed**; import with
"Import Extra Files" `srt` still works, only the displayed label may differ).

**Alternatives**: `.<lang>.ai.srt` or `.transcribed.srt` (the roadmap fixed `.whisper.srt`).

## R8. Quota and rate-limit handling

**Decision**: An in-process `QuotaState { remaining: Option<u32>, reset_at: Option<Instant> }` in the OpenSubtitles
client. Rules: a download answer's `remaining` and reset update it; HTTP 406 or `remaining == 0` sets exhausted until
`reset_at` (default 24 hours from now when no reset is given, capped); while exhausted, the OpenSubtitles pass
returns a `Skipped(quota)` result without any request (a search is also skipped, since a search without a
download is useless), the step message says `opensubtitles: quota used up until <time>`, the per-language result is
"missing" and the mode's Whisper fallback runs. It is not an `EngineError`, so no retry and no `done_with_warnings`.
`429` waits `Retry-After` (cap 60 s) once, then returns a transient failure (an `EngineError::Failed` after the
search level, retried by 038's optional-step policy). A process restart forgets the quota state; the first
download answer relearns it. The Test button and the Settings card show the last known `remaining`.

**Alternatives**: Persisting the state in SQLite (restart is rare, the first answer corrects it; Principle VII).

## R9. Login

**Decision**: Optional `username` and `password`. Without them, only the API key is used. With them, `POST /login`
when no valid token is cached; the JWT is kept in memory and refreshed on 401 or after 23 hours, and the returned
`base_url` host is used for subsequent calls when present. The password is masked in the API, redacted in
`Debug`, and has an env override like 025's key (`PUTIO_SYNC_OPENSUBTITLES_PASSWORD`, `..._API_KEY`).

**Rationale**: Quotas are tied to the account on the service's side; a key alone is the minimum.

**Alternatives**: OAuth-style flows (none offered). Storing the token on disk (secrets stay in config only).

## R10. Dependencies

**Decision**: No new dependency. `reqwest` (json, already used), `serde`, `tokio`, `tracing`, `tempfile`/`mockito`
for tests (all present). No `regex` (R5), no hashing crate (the OpenSubtitles hash is a loop over `u64`
values with `wrapping_add`), no fuzzy-match crate.

## R11. Metrics

**Decision** (in `putio-sync-telemetry/src/subtitles.rs`): the 025 counter
`subtitles_saved_total{source,language}` gets four more bounded `source` values: `os_hash`, `os_ids`, `os_name`
(OpenSubtitles API levels) and `transcribed_both` is **not** added (the file name differs but the source is still
`transcribed`). The 025 value `opensubtitles` keeps meaning "via Put.io's list". New:
`subtitles_skipped_total{reason}` with `reason` in `below_threshold|quota|no_result|no_ids|no_name|not_configured`
and `opensubtitles_requests_total{endpoint,result}` with endpoint `search|download|login` and result
`ok|rate_limited|quota|error`. Language labels stay bounded to the configured list plus `other`.

## R12. UI and API

**Decision**: Extend the existing Subtitles card (`ui/settings/subtitles.js`): a mode select (global), a table of
category overrides (categories from the known list the page already uses for exclusions) and an optional per-language
override column in the languages list; an OpenSubtitles section (key, login, threshold, Test). `GET/PUT
/settings/subtitles` add fields (contract `contracts/api.md`); new `POST /settings/subtitles/opensubtitles/test`.
The UI bundle size cap (`assets_total_under_480kb`) was raised once by 025; this feature must fit or raise it again
with a recorded reason.

## Open questions (collected unconfirmed items and decisions for the maintainer)

1. OpenSubtitles API: exact response and error shapes, download quota with and without login, `base_url` after
   login, whether `imdb_id` takes `tt` (task T070 against a live key; fixtures are the contract).
2. Radarr/Sonarr history parameters and embedded fields on current versions (R3 fallbacks exist).
3. Whether Put.io's list is matched by hash inside Put.io (R1): not needed for this feature to work.
4. Provenance of transcriptions in Download then Whisper and Whisper only (R7): name alone does not show it.
5. Default threshold 65 and the score weights (R5) are guesses to tune on a real library.
6. Whether Radarr/Sonarr label `.whisper` extra files acceptably (R7).
