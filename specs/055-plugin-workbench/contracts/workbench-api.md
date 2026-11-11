# Contract: workbench API

All routes need the API token, like the rest of `/api/v1`. Errors use the usual
`{"error": ...}` body.

## `GET /plugins/workbench`

`{editor, dir, starters: [{id, title, description}], events: [{type, data}], file}`:
whether the editor is on, the plugins folder, the starters, a sample `data` per event type, and a
sample file for sync hooks.

## `PUT /plugins/workbench/editor`

`{"enabled": true}` → `{message}`. Writes `plugin_editor` to the config.

## `POST /plugins/workbench/new`

`{starter, name}` → `{name, message}`. 403 editor off, 400 bad name, 404 unknown starter,
409 name taken (folder or plugin), 422 no plugins folder or the write failed.

## `GET /plugins/workbench/source/:name`

The plugin's source (see data model). 404 unknown plugin.

## `POST /plugins/workbench/check`

`{manifest, script}` → `{problems}`. Nothing is written.

## `PUT /plugins/workbench/source/:name`

`{manifest, script, versions: {"plugin.toml", "main.lua"}, force}` → `{problems, message}`.
403 editor off or not editable, 409 changed on disk since it was read (unless `force`),
422 write failed. Saved even with problems; reloads that plugin only.

## `POST /plugins/workbench/copy/:name`

Bundled plugin → folder of the same name. 403 editor off, 409 folder exists, 422 not bundled.

## `POST /plugins/workbench/run`

A run request → a run (200 even when the plugin failed). 404 unknown plugin, 409 another run or
action of that plugin is running, 400 bad request.

## `GET /plugins/workbench/runs`

`{runs: [run]}`, newest first, at most 20.

## `POST /plugins/workbench/runs/:id`

Runs that request again → a new run. 404 when it is no longer kept.
