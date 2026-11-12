# API contract: `/api/update` (055)

All routes sit behind the existing auth middleware (Principle VI). They are mounted next to
`/whats-new` in `putio-sync-api/src/lib.rs`.

## GET /update

```json
{
  "current": "0.3.0",
  "channel": {
    "kind": "container",             // see research R10
    "label": "Docker (dhi image)",
    "mode": "instructions",          // "swap" | "installer" | "instructions"
    "image": "ghcr.io/devclinton/putio-sync", "variant": "dhi"   // container only
  },
  "check": {
    "enabled": true,
    "last_attempt_at": "2026-11-10T09:00:00Z",
    "last_success_at": "2026-11-10T09:00:00Z",
    "error": null
  },
  "available": {                     // null when up to date or unknown
    "version": "0.4.0", "date": "2026-11-20",
    "url": "https://github.com/devclinton/putio-sync/releases/tag/v0.4.0",
    "dismissed": false,
    "releases": [ /* ReleaseView, same shape as GET /whats-new, newest first */ ],
    "instruction": {                 // mode "instructions" (and the fallback text otherwise)
      "commands": ["docker pull ghcr.io/devclinton/putio-sync:dhi"],
      "alternatives": ["docker pull ghcr.io/devclinton/putio-sync:dhi-0.4.0"],
      "steps": "Recreate the container after the pull.",
      "link": null
    },
    "patch": true,                   // mode "swap": a patch from the running version exists
    "download_bytes": 4194304
  },
  "auto_install": false,             // shown only when mode == "swap"
  "busy": { "active_transfers": 2 },
  "attempt": null,                   // or { "to", "kind", "state", "error" } (data-model.md)
  "rolled_back": null                // or { "version", "reason" }, cleared by POST /update/dismiss
}
```

## POST /update/check

Runs a check now. `200` with the same body as GET. If a check ran less than 60 s ago it answers
`409 {"error": "checked_recently"}` (not 429: the web UI treats a 429 as the sign-in rate limit) and makes no request. Allowed even when `check` is off (an
explicit user action).

## POST /update/dismiss

`{"version": "0.4.0"}` → `204`. Hides the notice for that version. `{"rolled_back": true}`
clears the rollback message.

## POST /update/apply

`{"interrupt": false}`

| Answer | When |
|--------|------|
| `202` with the body of GET | started (`attempt.state` moves on, poll GET every 2 s) |
| `409 {"error": "busy", "active_transfers": n}` | transfers running and `interrupt` is false |
| `409 {"error": "not_supported", "mode": "instructions"}` | managed channel |
| `409 {"error": "in_progress"}` | another attempt is running |
| `409 {"error": "not_writable", "path": "…"}` | the binary's folder can't be written (US3 scenario 6) |
| `404 {"error": "no_update"}` | nothing newer known |

During `restarting` the API goes away. The UI polls `GET /status` until `version` changes or
the rollback shows up.

## Settings

`[updates] check` and `auto_install` save through the existing Settings save route, as a new
"Updates" section with the same validation and `saved_message` pattern as the other sections
in `routes/system/settings.rs`.

## GET /status (existing)

Gains `"install_channel": "<kind>"` (FR-012), so a wrong detection shows up in support output.

## Metrics (Principle V)

| Name | Type | Labels |
|------|------|--------|
| `putio_sync_update_checks_total` | counter | `result` = `ok` \| `not_modified` \| `error` \| `rate_limited` |
| `putio_sync_update_available` | gauge | none (0 or 1) |
| `putio_sync_update_attempts_total` | counter | `kind` = `patch` \| `full` \| `installer`, `result` = `done` \| `failed` \| `rolled_back` |
| `putio_sync_update_verify_failures_total` | counter | `stage` = `signature` \| `download` \| `binary` |
