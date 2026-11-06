# Data Model: Subtitles (025)

## Config (core, `putio-sync-core/src/subtitles.rs`)

`SubtitlesConfig`
- `languages: Vec<String>`: canonical codes, ordered, unique, at most 10. Empty = stage off.
- `excluded_categories: Vec<String>`: category names (case-insensitive match).
- `transcription: TranscriptionConfig`

`TranscriptionConfig`: `enabled: bool` (false), `url: String`, `model: String`, `api_key: String` (masked
`********`, `Debug` prints `<redacted>`), `timeout_secs: u64` (900, 10..=7200), `language: String` (hint, mapped),
`chunk_secs: u64` (600, 60..=1800), `ffmpeg_path: String`.

## Put.io (client, `putio-sync-client/src/subtitles.rs`)

`SubtitleList { default: Option<String>, subtitles: Vec<PutioSubtitle> }`
`PutioSubtitle { key, format, language, language_code, name, source, url }`: only `key` is required when parsing.

## Engine types (`putio-sync-subtitles`)

- `Candidate { key, language (canonical), name, source: SourceKind, forced: bool, sdh: bool, order: usize }`
- `SourceKind`: `Embedded | Folder | OpenSubtitles | Unknown`; metrics label via `as_metric()`:
  `putio_embedded | putio_folder | opensubtitles | transcribed`.
- `Slot { language, kind: Main | Forced | Sdh }` names the file: `<stem>.<language>[.forced|.sdh].srt`
- `Existing { language, kind, path }` for sidecars found (FR-003)
- `Cue { start_ms, end_ms, text }`
- `Outcome` per video:
  `Saved(Vec<SavedSubtitle>)`, `Present` (all wanted slots already exist), `NoneFound`, plus `warnings`.
  `SavedSubtitle { path, language, kind, source, bytes, wanted: bool }`.
- `VideoRequest { video_path, putio_file_id: Option<u64>, languages, transcription: Option<TranscriptionConfig> }`

State transitions per video step: `waiting -> running -> passed | failed(retry via next_attempt_at) -> given up
(item done_with_warnings)`. `passed` carries a result message ("en: opensubtitles; es: missing") stored as the
attempt message and shown in the timeline.

## Database

`pipeline_steps`: `kind` now in (`hook`,`download`,`notify`,`subtitle_scan`,`subtitles`); `skip_reason` now in
(`not_configured`,`category`,`operator`,`hook`,`no_video`). Subtitle steps use `name` = `videos` (scan) or the
video's path relative to the item (per video), `required = 0`, `changes_files = 1`. No new columns.

`pipeline_items.source`: gains the value `manual` (no CHECK exists). Manual item key: `manual:<n>`.

`pipeline_deferred_deletes`:

| Column | Type | Notes |
|---|---|---|
| `file_id` | INTEGER PRIMARY KEY | Put.io file id awaiting delete |
| `item_key` | TEXT NOT NULL | `transfer:<id>`, `folder:<path>` or `file:<id>` |
| `created_at` | INTEGER NOT NULL | unix seconds; 24 h failsafe |

`downloads`: new read query `find_by_local_path(path) -> Option<Download>`.

## Metrics

| Name | Type | Labels |
|---|---|---|
| `putio_sync_subtitles_saved_total` | counter | `source` (4 values), `language` (configured or `other`) |
| `putio_sync_subtitles_transcribed_seconds_total` | counter | none |
| `putio_sync_pipeline_step_*` (existing) | | `kind` gains `subtitle_scan`, `subtitles` |
