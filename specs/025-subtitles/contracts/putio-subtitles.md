# Contract: Put.io subtitle API as used

Source of the shapes: putio-sdk-typescript (`files.ts`, `files.spec.ts`), putio-sdk-swift, Go SDK doc. The live API
reference could not be fetched; items marked (unconfirmed) are verified by the first manual run.

## List

`GET https://api.put.io/v2/files/{id}/subtitles?languages=en,es` (the CSV is URL-encoded `en%2Ces`)

```json
{"status": "OK", "default": "sub-1",
 "subtitles": [{"format": "srt", "key": "sub-1", "language": "English", "language_code": "en",
                "name": "English", "source": "opensubtitles", "url": "https://api.put.io/subtitles/1"}]}
```

Parsing: `key` required; all other fields optional strings; unknown fields ignored. `default` may be null.
Status handling: 401 (invalid scope), 402, 404 map to "Put.io unavailable for this file" (not a step failure);
429/5xx use the client retry then fail the Put.io pass (step fails, retried by the pipeline).

## Download

`GET https://api.put.io/v2/files/{id}/subtitles/{key}?format=srt` (unconfirmed path and parameter; fallback: the list
item's `url` with the same auth header). Body is subtitle text, not JSON; decoded per research R6. An empty body or no
cues is a failed candidate.

## Auth and calls

Same bearer token and base URL override (`with_base_url`) as the rest of the client, so mockito tests point at the mock.
Calls count in the existing API-call metrics. The token is never logged.

## Source strings (unconfirmed)

Seen: `opensubtitles`. Matched by substring, case-insensitive: embedded = `mkv`/`embedded`/`track`/`stream`,
folder = `folder`/`file`/`sidecar`; anything else ranks last and is counted as `opensubtitles` with a warn log.
