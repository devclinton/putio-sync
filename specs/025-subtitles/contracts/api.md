# Contract: HTTP API (025)

All routes need the API token like every other protected route. Errors use the project's usual `{error: "..."}`.

## Settings

### `GET /settings/subtitles`

```json
{
  "languages": ["en", "es"],
  "excluded_categories": ["radarr"],
  "transcription": {"enabled": true, "url": "http://nas:8000", "model": "Systran/faster-whisper-small",
                    "api_key": "********", "timeout_secs": 900, "language": "", "chunk_secs": 600,
                    "ffmpeg_path": ""},
  "ffmpeg": {"found": true, "path": "/usr/bin/ffmpeg"},
  "known_languages": [{"code": "en", "name": "English"}],
  "live": ["languages", "excluded_categories", "transcription"],
  "applies_now": "New items and waiting steps use these settings; running steps finish as they are."
}
```

An empty key is `""`; a stored key is `"********"`.

### `PUT /settings/subtitles`

Body: the same shape (`ffmpeg`, `known_languages`, `live`, `applies_now` ignored). A `"********"` key keeps the stored
key. Returns the saved view as GET. `400` with the offending field for a validation error. A missing ffmpeg is not an
error; the response includes `"warnings": ["ffmpeg not found: transcription will fail until it is installed"]`.

### `POST /settings/subtitles/preview`

Body: `{"video": "Movie.2024.1080p.mkv", "settings": {...optional unsaved...}}`.
Returns `{"files": ["Movie.2024.1080p.en.srt", "Movie.2024.1080p.es.srt"], "forced_sdh_note": "..."}`.

### `POST /settings/subtitles/test`

Body (optional): `{"transcription": {...unsaved...}}`; a masked key uses the stored one.

```json
{
  "ok": true,
  "ffmpeg": {"found": true, "path": "/usr/bin/ffmpeg", "version": "6.1.1"},
  "server": {"status": 200, "took_ms": 840, "language": null, "text": "", "error": null}
}
```

Sends a 3 second synthetic tone WAV (made in process, no ffmpeg). `ok` is true when the server returned 2xx with a
parseable body, including empty text. On failure `ok` is false and `server.error` has the server's message (key
scrubbed) or `unreachable: <reason>` or `timeout after <n>s`. Always HTTP 200 for a completed test; `400` only if the
URL is missing or invalid.

## Local files (044 extension)

### `GET /local/files/{root}/subtitles?path=<relative>`

```json
{
  "eligible": true,
  "reason": null,
  "putio": "available",
  "transcription": {"enabled": true, "ffmpeg": true},
  "languages": ["en", "es"],
  "existing": [{"language": "en", "name": "Movie.en.srt"}]
}
```

`putio` is `available | gone | unknown`. `eligible` is false with a `reason` when: not a video extension, no wanted
languages and no transcription, or neither Put.io (`gone`/`unknown`) nor transcription (off or no ffmpeg) is possible.
`404` for a missing file, `400` for a path outside the root (same guard as the other 044 routes).

### `POST /local/files/{root}/subtitles`

Body: `{"path": "Movies/Movie (2024)/Movie.mkv"}`. Response `202 {"key": "manual:12"}`. `409` with the existing key when
a non-terminal item for the same path exists. `400` for a non-video or bad path. The work runs as a pipeline item
(source `manual`, one `subtitles` step), visible on the Active page and in the item timeline.
