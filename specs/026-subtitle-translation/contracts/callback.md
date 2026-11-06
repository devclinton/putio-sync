# Contract: Custom API (callback) translator

## Outbound: putio-sync to the service

`POST <callback.url>`, `Content-Type: application/json`, plus the configured headers, `User-Agent: putio-sync/<version>`.
Redirects are not followed. Timeout `translation.timeout_secs`.

```json
{
  "job_id": "5b1c0f5e-7b3b-4e6c-9d0f-2a8f1c2d9e11",
  "source_lang": "en",
  "target_lang": "es",
  "video_name": "Movie.2024.mkv",
  "srt": "1\n00:00:01,000 --> 00:00:02,500\nHello.\n\n2\n...",
  "callback_url": "http://nas.lan:8088/translate/callback/<64 hex characters>"
}
```

`source_lang` and `target_lang` are canonical codes (025's mapper). `srt` is UTF-8 SRT. `video_name` has no directories.

| Reply | Result |
|---|---|
| `200`, JSON body (content type has `json`, or body starts with `{`) | `{"srt": "..."}` result; `{"error": "..."}` fails the job; else fails |
| `200`, any other body | the body is the SRT |
| `202` | accepted: wait for the callback until `callback.timeout_secs` |
| other `2xx`, `3xx`, `4xx`, `5xx`, timeout, connect error | the attempt fails (step retry policy); the token dies |

## Inbound: the service to putio-sync

`POST <callback_base_url>/translate/callback/<token>`

- No `Authorization` header is needed or checked. The token is the credential: random, 256 bits, valid for one job and one
  use, until `expires_at`.
- Body (4 MiB limit): the SRT or VTT as text (`Content-Type: text/plain` or `application/x-subrip`, or any non-JSON
  type); or JSON `{"srt": "..."}`; or JSON `{"error": "..."}` to report failure.
- `200 {"ok": true}`: accepted, validated, re-aligned (research R9) and saved; the translation step is done.
- `422 {"error": "result has 118 cues, the source has 120"}`: the body is not usable. The token is used up and the
  step fails (and is retried by the pipeline with a new job).
- `404 {"error": "unknown or expired callback"}`: unknown, expired, already used or malformed token; the same body
  for every cause. Counts toward the failed-authentication limiter for the client IP.
- `429` (with `Retry-After`): that IP is rate limited by the failed-authentication limiter.
- `413`: body over 4 MiB.

The route is public (not behind the API token middleware). Its path is redacted in the HTTP trace and in the log viewer.

## Persistence and lifetime

One `translation_callbacks` row per job (data-model.md). The token is kept only as a SHA-256 hash. The row survives
restarts. At `expires_at` the pipeline tick marks it `expired`, ends the attempt `timeout` and fails the step without an
automatic retry.

## Example service (for the quickstart): accepts, then calls back

```text
POST /translate        -> reply 202, remember job.callback_url
...later...
POST <job.callback_url>  body: the translated SRT (text/plain)   -> 200 {"ok": true}
```
