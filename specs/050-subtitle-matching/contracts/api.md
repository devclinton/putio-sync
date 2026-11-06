# Contract: Settings routes (additions to 025)

Same auth as the other settings routes.

## `GET /settings/subtitles`

Adds to the 025 body:

```json
{ "mode": "download_then_whisper",
  "category_modes": {"anime": "whisper_only"},
  "language_modes": {"ja": "both"},
  "min_confidence": 65,
  "opensubtitles": { "api_key": "********", "username": "me", "password": "********", "user_agent": "", "active": true,
                     "quota": { "remaining": 197, "reset": "2026-11-06T00:00:00Z" } },
  "modes": ["download_only", "download_then_whisper", "whisper_only", "both"] }
```

`mode` is always returned resolved (`download_then_whisper` when the file has none); a PUT of that value is saved as
set (a harmless change to the file). `quota` is `null` until a download answer was seen. Secrets masked.

## `PUT /settings/subtitles`

Accepts the same fields. `********` keeps the stored `api_key` / `password`. Validation errors (400) use the 025
shape with the offending key, for example `mode "both" (category_modes.anime) needs transcription turned on`.
Applying: `configure_subtitles(&state)` rebuilds the OpenSubtitles client (token and quota reset) and the mode plan
source; takes effect for the next step, in-flight steps finish as they are.

## `POST /settings/subtitles/opensubtitles/test`

Body: optional `{api_key, username, password}` (masked values keep the stored ones), so the card tests unsaved input.
200: `{"ok": true, "message": "key accepted", "allowed_downloads": 200, "remaining": 197}`.
200 with `ok:false`: `{"ok": false, "message": "key rejected (401)"}`. Never includes the key, password or token.

## Local files (044)

`POST` Fetch subtitles is unchanged on the wire. Its preflight text gains the mode: for Whisper only, "Put.io and
OpenSubtitles are not used".
