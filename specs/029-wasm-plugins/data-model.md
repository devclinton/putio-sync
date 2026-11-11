# Data Model: WebAssembly Plugins

## Manifest (`plugin.toml`) additions

| Field | Type | Rule |
|---|---|---|
| `runtime` | `"lua"` \| `"wasm"` | default `"lua"` |
| `entry` | file name | default `main.lua` / `plugin.wasm`; a plain name in the folder |
| `permissions.files.read` | list of folders | absolute, or `{download_dir}` and below; no `..` |
| `permissions.files.write` | list of folders | same; a write folder is readable too |
| `permissions.files.read_from_config` / `write_from_config` | config keys | string fields holding a folder |

`paths_needed(settings)`: sorted `r:<folder>` / `w:<folder>` strings from the fixed folders
and the config values; `{download_dir}` stays as written.

## Plugin settings (`[[plugins]]`)

| Field | Type | Rule |
|---|---|---|
| `approved_paths` | list of `r:`/`w:` strings | written by the enable-and-save; all of `paths_needed` must be in it, or the plugin is **Needs approval** |

## Plugin status (`GET /plugins`) additions

`runtime`, `paths_needed`, `approved_paths`; `permissions.files` inside `permissions`.

## Package (install)

| Variant | Contents |
|---|---|
| `Files` | `plugin.toml` text and the entry bytes |
| `Wasm` | a module with a `putio-plugin` custom section holding `plugin.toml`; entry `plugin.wasm` |

Checks: the manifest parses, the entry is under the size cap (Lua 1 MiB, wasm 16 MiB),
WebAssembly starts with the magic, Lua is UTF-8. Errors: no folder, invalid (with reason),
exists (needs `replace`), I/O.

## WebAssembly instance (per plugin worker)

- Built on the first event or job; `init` runs once per instance.
- State: store with fuel, limiter (`memory_mb`), the host (shared with Lua), the last host
  reply.
- Rebuilt after a timeout, a memory-limit failure, a trap or `proc_exit`.
