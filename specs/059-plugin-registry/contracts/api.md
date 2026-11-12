# Contract: plugins API additions

All routes are under the existing authenticated `/plugins` router (FR-027). Errors use the API's
usual `{error}` body. Request bodies are JSON.

## `GET /plugins` (changed)

Each `PluginStatus` gains:

```json
"links": {
  "author": "devclinton", "license": "MIT",
  "docs": "https://…", "repository": "https://…", "issues": "https://…"
},
"registry": {
  "version": "1.0.0", "installed_at": "2026-11-10T16:00:00Z",
  "state": "update", "changed": false,
  "update": { "version": "1.1.0", "notes": "https://…",
              "hosts_added": ["api.example.com"], "hosts_removed": [],
              "events_added": [], "events_removed": [] },
  "withdrawn": null
}
```

`links` leaves out fields not set. `registry` is `null` for plugins without an install record. Its
`state` and `update` come from the cached index only; `GET /plugins` never contacts the registry.

## `GET /plugins/registry?refresh=false`

Fetches the index when the cache is stale (6 h) or `refresh=true`, unless the registry is off.

```json
{
  "enabled": true,
  "url": "https://raw.githubusercontent.com/…/index.json",
  "fetched_at": "2026-11-10T16:00:00Z",
  "error": null,
  "skipped": 0,
  "plugins": [
    {
      "name": "webhook-notifier", "description": "…", "author": "…", "license": "MIT",
      "repository": "https://…", "docs": "https://…", "homepage": null, "issues": "https://…",
      "events": ["download.finished"], "hosts": { "fixed": [], "from_config": ["url"] },
      "version": "1.0.0", "notes": "https://…",
      "state": "available", "needs": null, "withdrawn": null
    }
  ]
}
```

- `enabled: false` → `plugins: []`, no request made (FR-021, SC-004).
- Registry unreachable or index invalid → `error` holds why; `plugins` and `fetched_at` are from
  the cached index when there is one, else empty. Status is still 200.
- `version` is the release offered (the newest this putio-sync supports); `needs` is the lowest
  `min_putio_sync` when none is supported (`state: "needs_newer"`).
- Withdrawn entries are listed only when installed.

## `POST /plugins/registry/preview` `{ "name": "webhook-notifier" }`

Downloads and checks the offered release (all FR-023 checks) without writing anything, and returns
what installing or updating would change:

```json
{ "version": "1.1.0", "sha256": "…",
  "hosts_added": [], "hosts_removed": [], "settings_added": ["priority"],
  "settings_removed": [], "events_added": [], "events_removed": [], "changed": false }
```

For a plugin not installed, everything in the manifest counts as added. `409` when the state is
`name_in_use` or `needs_newer`; `422` with the failed check's text when the archive is refused.

## `POST /plugins/registry/install` `{ "name": "webhook-notifier" }`

Installs the offered release. `200 {message, plugin: PluginStatus}`; the plugin is off.
`409` for `name_in_use`, `needs_newer`, `withdrawn` or an install of that name in progress; `422`
for a refused archive; `500 {error}` when the plugins folder cannot be written (for example
read-only). Writes nothing on any failure.

## `POST /plugins/registry/update` `{ "name": "…", "sha256": "…", "replace_changed": false }`

`sha256` must equal the preview's, so the update installs what the person saw. `409` when
`changed` is true and `replace_changed` is false, or the offered release differs from `sha256`.
On a new version that fails to load: the previous one is put back and `422 {error}` says why.
Settings, secrets, approvals and store are kept; new hosts make the plugin **Needs approval**.

## `POST /plugins/registry/remove` `{ "name": "…" }`

Deletes `plugins/<name>/` when it has an install record; `409` otherwise. Settings are kept.
Then plugins reload. The UI asks first.

## Settings

`GET/PUT /settings` gains `plugin_registry: {enabled, url}`, checked with the link rule.
