# Contract: Whisper server (OpenAI-style transcription) as used

## Request

`POST <endpoint>` where endpoint = the configured URL, with `/v1/audio/transcriptions` appended when the URL path
is empty or `/`.

Headers: `Authorization: Bearer <api_key>` only when set. Body `multipart/form-data`:

| Field | Value |
|---|---|
| `file` | chunk WAV (16 kHz mono PCM s16le), file name `audio.wav` |
| `model` | configured model; omitted if empty |
| `response_format` | `verbose_json` |
| `language` | hint, or chunk 1's detected language for later chunks; omitted otherwise |

Timeout: `timeout_secs` per request. One request in flight at a time across the daemon.

## Response (accepted shapes)

1. JSON: `{"language": "english"|"en", "duration": 600.0, "text": "...", "segments": [{"start": 0.0, "end": 4.2, "text": " Hello."}]}`.
   SRT is built from `segments`. Language goes through the code mapper.
2. Body that is not JSON but parses as SRT (server ignored `response_format`): used as is; the language must then come
   from the config hint, else the step fails with `server did not report a language: set transcription.language`.

Empty `segments` = no speech: nothing saved, step passes with "no speech found".
Non-2xx: the step fails with `HTTP <status>: <server message, truncated to 300 chars, key scrubbed>`.

## Chunk join

Chunk i covers `[i*chunk_secs, min((i+1)*chunk_secs, duration))`, no overlap. Segment times are shifted by the chunk start,
end times clamped to the chunk end, a start before the previous end is moved to it, cues renumbered.

## Compatibility notes (unconfirmed, tested by the Test button on the user's server)

- speaches / faster-whisper-server: OpenAI transcription API with `model` as `owner/name`.
- whisper.cpp server: native endpoint is `/inference` (enter the full URL); its README shows `json` only, so the SRT
  and language fallbacks above may apply.
