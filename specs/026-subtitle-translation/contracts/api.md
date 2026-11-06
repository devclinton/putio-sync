# Contract: HTTP API (026)

All routes need the API token like every other protected route, except the callback route, which is documented in
[callback.md](callback.md). Errors use `{error: "..."}`.

## Settings

### `GET /settings/translation`

```json
{
  "kind": "chat",
  "excluded_categories": ["radarr"],
  "max_in_flight": 2, "timeout_secs": 180, "chunk_cues": 40, "instruction": "",
  "chat": {"base_url": "http://ollama.lan:11434/v1", "model": "qwen2.5:14b", "api_key": "********"},
  "callback": {"url": "", "callback_base_url": "", "timeout_secs": 3600, "headers": {"Authorization": "********"}},
  "target_languages": ["en", "es"],
  "resolved_endpoint": "http://ollama.lan:11434/v1/chat/completions",
  "callback_url_form": "http://nas.lan:8088/translate/callback/<token>",
  "warnings": ["no wanted languages in Subtitles settings: nothing will be translated"],
  "live": ["kind", "excluded_categories", "max_in_flight", "timeout_secs", "chunk_cues", "instruction", "chat", "callback"],
  "applies_now": "New items and waiting steps use these settings; running translations finish as they are."
}
```

An empty key or header value is `""`; a stored one is `"********"`. `resolved_endpoint` is null unless `kind` is `chat`;
`callback_url_form` is null unless `callback_base_url` is set.

### `PUT /settings/translation`

Body: the same shape (`target_languages`, `resolved_endpoint`, `callback_url_form`, `warnings`, `live`, `applies_now`
ignored). `"********"` keeps the stored key or header value. Returns the saved view as GET. `400` with the offending
field for a validation error, including `callback.callback_base_url: required when kind is callback`.

### `POST /settings/translation/test`

Body (optional): `{"translation": {...unsaved settings...}, "sample_target": "es"}`; masked values use the stored ones.

Chat:

```json
{
  "ok": true, "kind": "chat",
  "endpoint": "http://ollama.lan:11434/v1/chat/completions",
  "took_ms": 2310, "problem": null, "error": null,
  "sample": [{"number": 1, "source": "Good morning.", "translation": "Buenos días."},
             {"number": 2, "source": "Where are you going?\nIt is late.", "translation": "¿Adónde vas?\nEs tarde."}],
  "models": null
}
```

On failure: `ok: false`, `problem` is one of `url` (unreachable or `404` on the endpoint), `key` (`401`/`403`), `model`
(model not found; `models` lists up to 20 ids from `GET <base>/models` when it answered), `timeout`, `format` (the
server answered but the reply failed validation; `sample` carries the raw reply lines), `server` (other status);
`error` is the server's message with the key scrubbed. Time limit 15 s total.

Callback:

```json
{
  "ok": true, "kind": "callback",
  "result": "will_call_back",
  "service": {"status": 202, "took_ms": 420, "error": null},
  "base_url_reachable_from_here": {"checked": "http://nas.lan:8088/health", "status": 200, "error": null},
  "problem": null, "error": null
}
```

`result` is `immediate` (200 with a parseable subtitle) or `will_call_back` (202). `problem` is `callback_base_url`
(not set), `url`, `headers`/`key` (`401`/`403`), `timeout`, `format` (200 but not a subtitle), `server`.
`base_url_reachable_from_here` says only that this host can reach its own base URL. The sample job's `callback_url` has
no valid token. Always HTTP 200 for a completed test; `400` only for invalid settings for that kind.

## Active timeline (038), extended

A step waiting for a callback gets `waiting`:

```json
{"id": 42, "name": "Movie.2024.mkv", "kind": "translate", "branch": "es", "state": "running",
 "waiting": {"for": "callback", "since": "2026-11-05T10:00:00Z", "expires_at": "2026-11-05T11:00:00Z"}}
```

`waiting` is absent otherwise. The Active row shows "Translating: Movie.2024.mkv (es), waiting for callback". The step's
last attempt message after success is `es: saved Movie.es.srt (1,204 cues, 31 chunks, 2 retries)`, or for a callback
`es: saved Movie.es.srt (1,204 cues, realigned 12)`.

## Local files (044 extension)

### `GET /local/files/{root}/translate?path=<relative>`

```json
{
  "eligible": true, "reason": null,
  "translator": {"kind": "chat", "configured": true},
  "sources": [{"language": "en", "name": "Movie.en.srt"}],
  "targets": [{"language": "es", "name": "Spanish", "exists": false, "wanted": true},
              {"language": "en", "name": "English", "exists": true, "wanted": true}]
}
```

`eligible` is false with a `reason` when: not a video extension, `kind` is `none` or incomplete, no `Main` sidecar
subtitle to translate from, or no target without a file. `404` for a missing file, `400` for a path outside the root.

### `POST /local/files/{root}/translate`

Body: `{"path": "Movies/Movie (2024)/Movie.mkv", "targets": ["es", "ja"]}`. Response `202 {"key": "manual:13"}`. `409` with
the existing key when a non-terminal manual item exists for the path. `400` for a non-video, bad path, unknown or
existing target, or more than 10 targets. The work runs as a pipeline item (source `manual`, one `translate` step per
target, branch = language); it is visible on the Active page and in the item timeline.
