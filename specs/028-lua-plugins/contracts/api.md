# Contract: HTTP API

All routes need the API token and answer 503 in setup mode.

## GET /plugins

```json
{
  "dir": "/config/plugins",
  "plugins": [{
    "name": "ntfy", "version": "1.0.0", "description": "…",
    "source": "bundled", "path": null,
    "state": "enabled",            // disabled | enabled | broken | needs_approval | missing_config
    "error": null,
    "events": ["download.finished"],
    "permissions": { "http": { "hosts": [], "host_from_config": ["server"] }, "store": false },
    "config_fields": [{ "key": "topic", "type": "string", "label": "Topic", "help": "",
                        "default": null, "required": true, "secret": false }],
    "hosts_needed": ["ntfy.sh"],
    "approved_hosts": ["ntfy.sh"],
    "has_tick": true,
    "queued": 0,
    "runs": [{ "event_type": "download.finished", "event_id": "…", "started_at": "…",
               "duration_ms": 120, "result": "success", "error": null, "logs": [] }]
  }]
}
```

No secret value appears.

## POST /plugins/test `{"name": "ntfy"}`

200 with a run (above, `logs` filled) even when the plugin failed; 404 unknown name;
409 a test of it is running; 422 the plugin is broken (with its error).

## POST /plugins/reload

200 `{"message": "Reloaded 5 plugins (1 broken)"}`.

## GET /settings/plugins, PUT /settings/plugins, POST /settings/plugins/preview

Body: `{"plugins_dir": null | "...", "plugins": [PluginSettings]}` with `secrets` values
masked in GET. PUT validates against the manifests in the folder being saved (400 naming plugin and field, like the other groups), sets
`approved_hosts` of enabled plugins to their needed hosts, writes the config and applies it
now. A changed `plugins_dir` reloads from the new folder.
