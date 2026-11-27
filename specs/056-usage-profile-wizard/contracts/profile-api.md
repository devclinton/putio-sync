# Contract: Usage profile routes

All setup routes need the setup code (013). Settings routes need the API token.

## GET /setup  (setup mode, public)

The web UI, as `/`. Used by the printed link `http://host:port/setup#code=…`.

## GET /setup/profile/detect

```json
{ "found": { "movies": ["Movies"], "tv": ["TV Shows"] },
  "folders": ["Movies", "TV Shows", "Other"],
  "watch_dir": "/watches", "tailscale": false }
```
400 when Put.io isn't connected.

## POST /setup/profile

Body `{ "answers": Answers | null, "overrides": { id: Override } }`. `null` answers clear the
profile (manual path). Returns the review:
```json
{ "recommendations": [Recommendation], "risks": [Risk], "notes": ["dlna", "tailscale"] }
```
400 for no kinds or an unusable watch folder.

## POST /setup/finish

As 013; with a profile in the session the plan is applied to the saved config and `[profile]`
is written.

## GET /settings/profile

`{ "answers": Answers | null, "found": {...}, "folders": [...], "language": "de" }`

## POST /settings/profile/preview

Body as `POST /setup/profile`. Returns the review plus
`{ "changes": [{key, before, after, restart}], "kept": [{key, current, profile}] }`.

## PUT /settings/profile

Same body; saves through the settings save (hot reload), returns `{changes, kept}`.
