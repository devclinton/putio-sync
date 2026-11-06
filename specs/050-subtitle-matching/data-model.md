# Data Model: Subtitle Matching and Source Modes (050)

No database table is added or changed. Everything below is configuration, in-memory state, or derived per run.

## SubtitleMode (core)

`download_only` | `download_then_whisper` | `whisper_only` | `both` (serialized snake_case).

| Property | Download | Whisper |
|---|---|---|
| download_only | yes | never |
| download_then_whisper | yes | only when no wanted language was found or present (025 rule) |
| whisper_only | never | always (once per video) |
| both | yes | always (once per video), name has `.whisper` |

## SubtitlesConfig additions (core)

| Field | Type | Default | Rule |
|---|---|---|---|
| `mode` | `Option<SubtitleMode>` | `None` (= download_then_whisper) | skipped in output when `None` |
| `category_modes` | map category (lowercase) to mode | empty | keys trimmed, lowercased; empty key rejected |
| `language_modes` | map language code to mode | empty | keys mapped through the language table; unknown rejected; a key not in `languages` is allowed and has no effect |
| `min_confidence` | `u8` | `65` | 0 to 100 |
| `opensubtitles` | `OpenSubtitlesConfig` | all empty | below |

Existing fields (`languages`, `excluded_categories`, `transcription`) are unchanged.
Validation (added): any `whisper_only` or `both` anywhere requires `transcription.enabled` with a valid URL.

## OpenSubtitlesConfig (core): `[subtitles.opensubtitles]`

| Field | Type | Default | Rule |
|---|---|---|---|
| `api_key` | string | `""` | masked `********`; env `PUTIO_SYNC_OPENSUBTITLES_API_KEY`; redacted `Debug` |
| `username` | string | `""` | optional |
| `password` | string | `""` | masked; env `PUTIO_SYNC_OPENSUBTITLES_PASSWORD`; redacted `Debug`; requires `username` |
| `user_agent` | string | `""` | empty means `putio-sync <version>`; the service requires one |

There is no `enabled` flag: an empty `api_key` means this part is off (`is_active()`), Principle VII.

## ModePlan (subtitles, per video, computed)

| Field | Meaning |
|---|---|
| `per_language` | for each wanted language, its effective `SubtitleMode` |
| `other` | mode used for a detected language that is not wanted (category, else global) |
| derived: `downloads(lang)` | mode is not `whisper_only` |
| derived: `always_transcribe()` | any language is `whisper_only` or `both` |
| derived: `fallback_transcribe()` | any language is `download_then_whisper` |
| derived: `whisper_token(detected)` | the mode of `detected` (or `other`) is `both` |

## ReleaseInfo (subtitles)

`title: String`, `year: Option<u16>`, `season: Option<u16>`, `episode: Option<u16>`, `resolution: Option<Resolution>`,
`source: Option<Source>`, `codec: Option<Codec>`, `group: Option<String>`. Parsed from a file stem or a candidate's `release`.
`Resolution`, `Source`, `Codec` are small enums (normalised, so `WEB-DL` and `web.dl` are equal).

## MediaInfo (core)

| Field | Meaning |
|---|---|
| `kind` | `Movie` or `Episode` |
| `title`, `year` | from the *arr, optional |
| `imdb_id` | digits, `tt` stripped; for an episode, the series |
| `tmdb_id` | movie TMDb id, or the series TMDb id for an episode |
| `season`, `episode` | for an episode: all `(season, episode)` pairs of the download |

## Candidate score (subtitles)

`Score { total: u8, level: Level, reasons: Vec<Reason> }`; `Level` = `Hash | Ids | Name`; `Reason` records which
parts matched (group, source, resolution, codec, title similarity, year, episode). Shown on the step message
(`es: opensubtitles ids 80 (group, source)`) and in logs.

## MatchSource (labels)

`putio_embedded`, `putio_folder`, `opensubtitles` (via Put.io, 025), `os_hash`, `os_ids`, `os_name`, `transcribed`.

## Quota state (in memory, OpenSubtitles client)

`remaining: Option<u32>`, `reset_at: Option<Instant>`, plus the JWT and its expiry. Lost at restart.

## Outcome additions (engine)

`Outcome.skipped: Vec<SkipNote>` with `SkipNote { language, reason: below_threshold(score) | quota | no_result | no_ids | no_name }`,
used for the step message and metrics. `SavedSubtitle.source` takes the new labels; `SlotKind` gains `Whisper`.
