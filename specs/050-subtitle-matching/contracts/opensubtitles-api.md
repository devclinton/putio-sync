# Contract: OpenSubtitles REST API as used

Base: `https://api.opensubtitles.com/api/v1`. Every request sends `Api-Key: <key>`, `User-Agent: <ua>`,
`Accept: application/json`, `Content-Type: application/json` on POST. With a login, also
`Authorization: Bearer <jwt>`.

Marks: **[C]** field or parameter name seen in the Stoplight reference page; **[U]** unconfirmed (general knowledge or
secondary sources), to be checked with a live key (task T070). The parser treats everything but the file ID as optional.

## Search: `GET /subtitles`

| Param | Used for | |
|---|---|---|
| `moviehash` **[C]** | fingerprint level (16 lowercase hex chars) | |
| `moviehash_match` **[C]** | not sent; read from results (`only` could be used later) | |
| `imdb_id` **[C]** | movie, digits without `tt` | [U] without `tt` |
| `tmdb_id` **[C]** | movie | |
| `parent_imdb_id`, `parent_tmdb_id` **[C]** | series of an episode | |
| `season_number`, `episode_number` **[C]** | episode | |
| `languages` **[C]** | comma list of missing languages, 639-1 | |
| `type` **[C]** | `movie` or `episode` (name level) | |
| `query` | name level title, lowercase | [U] |
| `hearing_impaired` | `exclude` unless a SDH slot is wanted | [U] |
| `machine_translated`, `ai_translated` | `exclude` | [U] |

Parameters are sent sorted by name and lowercase.

Response (relevant fields):

```json
{ "total_count": 2,
  "data": [ { "id": "1234",
    "attributes": {
      "language": "en", "release": "Show.Name.S02E05.1080p.WEB-DL.x264-GRP",     // release [C]
      "hearing_impaired": false, "foreign_parts_only": false,                      // hearing_impaired [C]
      "machine_translated": false, "ai_translated": false,                         // [U]
      "moviehash_match": true, "download_count": 4200, "ratings": 8.0,             // [C]
      "feature_details": { "feature_type": "Episode", "year": 2019, "title": "...",// feature_details [C]
                           "season_number": 2, "episode_number": 5, "imdb_id": 0, "tmdb_id": 0,
                           "parent_imdb_id": 0, "parent_tmdb_id": 0 },
      "files": [ { "file_id": 98765, "file_name": "..." } ] } } ] }              // files, file_id [C]
```

Use: candidate = `data[i]` with `files[0].file_id` (no file, no candidate). `language` is mapped by the 025 language table.
Candidates with `hearing_impaired`/`foreign_parts_only` go to their own slots only if those slots are wanted; with
`machine_translated` or `ai_translated` true are dropped (FR-018).

Errors: 401/403 bad key or token -> `Failed("opensubtitles: key rejected")`, not retried by the OpenSubtitles pass
until the settings change; 429 -> wait `Retry-After` (cap 60 s), once; 5xx and network -> transient `Failed`.
Anything else -> `no_result`.

## Download: `POST /download` [U body and response, **[C]** nothing]

Request `{"file_id": 98765, "sub_format": "srt"}`. Response
`{"link":"https://...","file_name":"...","requests":3,"remaining":197,"message":"...","reset_time":"23h 12m","reset_time_utc":"2026-11-06T00:00:00.000Z"}`.
Then `GET link` (no auth) returns the subtitle bytes, passed to the 025 converter (`srt::convert`).
HTTP 406 (or `remaining` 0 with no `link`) = quota used up -> `QuotaState` exhausted, `Skipped(quota)`.

## Login: `POST /login` [U]

`{"username":"...","password":"..."}` -> `{"token":"<jwt>","base_url":"api.opensubtitles.com","user":{"allowed_downloads":200,...}}`.
401 -> `login failed` (message does not echo the password).

## Test button

1. If login set: `POST /login`. 2. `GET /subtitles?query=test&type=movie` (or a fixed IMDb id) to check the key.
3. Report `ok`, `allowed_downloads` and `remaining` when known. Never shows or logs the key, password or token.

## Fixtures

`putio-sync-subtitles/tests/fixtures/opensubtitles/*.json` hold the sample responses above. They are the single
place to edit if a live check differs.
