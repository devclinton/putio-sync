# Contract: API additions

## `GET /plugins` (changed)

Each plugin gains `actions: [{id, label, help}]`, and each config field gains `choices`
(action id or null). `path_map` values are lists of `{from, to}`.

## `POST /plugins/action` (new)

Request:

```json
{ "name": "plex", "action": "sign_in", "input": {}, "config": { "server": "http://…" } }
```

`config` holds unsaved non-secret form values (secret keys are ignored); they override the
saved values for this call only.

Answer 200:

```json
{
  "message": "Enter this code at plex.tv/link", "code": "AB12",
  "open_url": "https://app.plex.tv/auth#?…", "choices": null,
  "next": { "action": "sign_in_poll", "input": { "pin": 123 }, "after_seconds": 2 },
  "done": false,
  "saved": ["token"], "config": { "server": "…" },
  "run": { "outcome": "ok", "duration_ms": 120, "error": null }
}
```

`saved`/`config` appear only when the action saved values: `saved` names every saved key,
`config` holds the saved non-secret values. Saved secrets are never returned.

Errors: 404 unknown plugin or action; 409 a call for this plugin is already running; 422
the plugin is broken, the action failed (`run.error`, scrubbed) or `save` was refused (the
field is named); 503 in setup mode.
