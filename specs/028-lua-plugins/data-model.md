# Data Model: Lua Plugins

## Manifest (`plugin.toml`, plugins crate)

| Field | Type | Rules |
|-------|------|-------|
| `name` | string | `[A-Za-z0-9_-]{1,40}`, unique across bundled and folder plugins (a folder plugin replaces a bundled one of the same name) |
| `version` | string | 1–32 printable characters |
| `description` | string | optional, ≤ 300 characters |
| `runtime` | string | `lua` (default); `wasm` is reserved and refused with "needs 029" |
| `entry` | string | default `main.lua`; a plain file name inside the plugin folder |
| `events` | list of strings | non-empty; event types from the schema or `*`; not `hook.test`/`plugin.test` |
| `permissions.http.hosts` | list of strings | host names, lowercase, no scheme or port |
| `permissions.http.host_from_config` | list of strings | keys of `url` config fields |
| `permissions.store` | bool | default false |
| `config` | list of fields | ≤ 32 |

Config field: `key` (`[a-z0-9_]{1,40}`, unique), `type` (`string`, `integer`, `number`,
`boolean`, `url`, `string_list`), `label`, `help`, `default` (must match the type),
`required` (default false), `secret` (default false; `string` or `url` only).

## Plugin settings (`[[plugins]]`, core)

| Field | Type | Default | Rules |
|-------|------|---------|-------|
| `name` | string | — | as manifest name; unique; ≤ 32 entries |
| `enabled` | bool | false | |
| `approved_hosts` | list of strings | [] | host names, written by Settings saves |
| `timeout_secs` | integer | 10 | 1–60 |
| `memory_mb` | integer | 32 | 4–256 |
| `config` | table | {} | values: string, integer, float, bool, list of strings |
| `secrets` | table of strings | {} | masked as `********` in views and exports |

Values are checked against the manifest by the plugins crate on a Settings save (type,
required, http(s) URL, unknown key refused, secret field outside `secrets` refused). Config
loading only checks structure, so a plugin folder that is missing does not stop the daemon.

`plugins_dir` (top-level, optional path): default `<config dir>/plugins`.

## Plugin (runtime, plugins crate)

- `source`: `bundled` or `folder` (with its path)
- `manifest`: parsed manifest, or `None` with `error`
- `state`: `disabled`, `enabled`, `broken` (manifest or load error), `needs_approval`
  (a needed host is not in `approved_hosts`), `missing_config` (a required value is unset)
- `queue`: ≤ 1 000 jobs; `generation`: bumped by reload and configure
- `runs`: last 20 `PluginRun`

State transitions: discovered → `disabled` until settings say `enabled`; `enabled` →
`broken` when loading fails (stays until reload); `enabled` → `needs_approval` when the
needed hosts grow; any → removed when its folder is gone at reload (settings kept).

## Plugin run

| Field | Type |
|-------|------|
| `event_type` | string: an event type, `tick` or `init` |
| `event_id` | string (empty for tick/init) |
| `started_at` | RFC 3339 |
| `duration_ms` | integer |
| `result` | `success`, `failure`, `timeout` |
| `error` | string or null (secrets scrubbed) |
| `logs` | up to 20 lines `{level, message}` (test runs only; scrubbed) |

## Store

JSON object per plugin: key (≤ 128 bytes) → JSON value (≤ 64 KiB); whole file ≤ 1 MiB.
