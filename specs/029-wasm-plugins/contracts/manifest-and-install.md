# Contract: manifest additions and plugin install

## `plugin.toml`

```toml
name = "go-min-size-filter"
version = "1.0.0"
runtime = "wasm"          # "lua" (default) or "wasm"
entry = "plugin.wasm"     # default: main.lua for lua, plugin.wasm for wasm
hooks = ["filter"]

[permissions.files]
read = ["{download_dir}"]            # absolute folders, or {download_dir} and below
write = ["{download_dir}/.cache"]
read_from_config = ["library"]       # keys of string config fields holding a folder
write_from_config = []
```

- Folders must be absolute after `{download_dir}` is put in; `..` parts are refused.
- A write folder is also readable.
- `paths_needed(settings)`: the fixed folders plus each `*_from_config` value, as
  `r:<folder>` or `w:<folder>` strings, sorted. `{download_dir}` stays as written so a
  download folder change does not need a new approval.

## `[[plugins]]` settings

```toml
approved_paths = ["r:{download_dir}", "w:/media/notes"]   # written by Settings saves
```

An enabled plugin whose needed folders are not all approved is **Needs approval**, like hosts.

## `GET /plugins` additions

Each plugin gains `runtime`, `paths_needed` and `approved_paths`; `permissions.files` is in
`permissions`.

## `POST /plugins/install`

Request (JSON), one of:

```json
{"url": "https://example.com/my-plugin/plugin.toml", "replace": false}
{"url": "https://example.com/my-plugin.wasm"}
{"manifest": "<plugin.toml text>", "entry": "<base64 of the entry file>", "replace": false}
{"wasm": "<base64 of a .wasm carrying a putio-plugin custom section>"}
```

- A URL ending in `.wasm`, or bytes starting with the WebAssembly magic, is a single-file
  plugin: its `putio-plugin` custom section holds the `plugin.toml` text; `entry` becomes
  `plugin.wasm`.
- Any other URL is a manifest; the entry is fetched from the same folder of the URL.
- http and https only; redirects to the same host only (at most 3); 16 MiB per file; 30 s.

Responses:

| Status | When |
|---|---|
| 200 `{"name", "message"}` | Written to `<plugins folder>/<name>/` and reloaded; the plugin is off |
| 400 | Not one of the shapes above, bad base64 |
| 409 | A plugin of that name exists and `replace` is false |
| 422 | The manifest or entry does not check out (the message names the problem), a fetch failed, or no plugins folder is set |

The files are written to a hidden `.<name>-install-<pid>` folder first and moved into place, so a
failed install leaves nothing behind. Replacing keeps the plugin's settings and store.
