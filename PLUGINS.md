# Lua plugins

A plugin is a small Lua script that putio-sync runs on its events: a download finished or
failed, a Put.io transfer was added, finished or failed, a sync run ended, or Put.io storage
runs low. The events are the same as for [event hooks](EVENTS.md), but a plugin runs inside
putio-sync. It needs no other program, it is limited in what it may do, and it is set up
from Settings › Plugins.

putio-sync comes with four plugins that send notifications: **ntfy**, **Discord**,
**Telegram** and **Pushover**. They are off until you turn them on.

## Turning a plugin on

Open Settings › Plugins. Each plugin shows its state, version, events and the hosts it will
contact. Tick **Enabled**, fill in its settings and save. Saving approves the hosts shown
under "Contacts". **Send test event** sends one `plugin.test` event and shows the result,
with any lines the plugin logged.

The same settings in `config.toml`:

```toml
# Optional; default is a "plugins" folder next to this file
plugins_dir = "/config/plugins"

[[plugins]]
name = "ntfy"
enabled = true
approved_hosts = ["ntfy.sh"]   # written by Settings when you save
timeout_secs = 10              # 1 to 60, per call. Default: 10
memory_mb = 32                 # 4 to 256. Default: 32

[plugins.config]
topic = "my-nas-8f3k"
group_minutes = 5

[plugins.secrets]
token = "tk_..."
```

If you edit the file by hand, list the plugin's hosts in `approved_hosts` yourself. Until
you do, the plugin shows **Needs approval** and does not run. A plugin with a required value
missing shows **Needs settings**. Settings for a plugin that is not found are kept, in case
its folder comes back, and the doctor reports them.

Changes made in Settings apply at once. **Reload plugins** reads the plugins folder again
without a restart.

## The bundled notifiers

| Plugin | Settings | Contacts |
|--------|----------|----------|
| `ntfy` | Server (default `https://ntfy.sh`), Topic, Access token (secret), Priority | The server's host |
| `discord` | Webhook URL (secret), Name shown | `discord.com` (from the URL) |
| `telegram` | Bot token (secret), Chat ID, Bot API server (default `https://api.telegram.org`) | The server's host |
| `pushover` | Application token (secret), User key (secret), Priority, API server (default `https://api.pushover.net`) | The API server's host |

Each one also has **Events** (default: download finished and failed, transfer failed, a
failed sync run, low storage) and **Group repeats for (minutes)** (default 5). The first
event of a kind is sent at once. Later events of the same kind within that time are counted
and sent as one summary, for example "9 more downloads finished: File 1, …, File 5 and 4
more". Set it to 0 to send every event.

There is no email plugin. A plugin can only make HTTP requests.

## Writing a plugin

A plugin is a folder in the plugins folder:

```
plugins/
  home-assistant/
    plugin.toml
    main.lua
```

A folder plugin with the same name as a bundled one replaces it.

### `plugin.toml`

```toml
name = "home-assistant"        # letters, digits, - and _; up to 40
version = "1.0.0"
description = "Tell Home Assistant when a download lands"
events = ["download.finished", "storage.low"]   # or ["*"]; see EVENTS.md

[permissions]
store = true                   # may keep values between calls

[permissions.http]
hosts = []                     # fixed hosts it may call
host_from_config = ["url"]     # hosts taken from these url settings

[[config]]
key = "url"
type = "url"                   # string, integer, number, boolean, url, string_list
label = "Webhook URL"
help = "Settings › Automations › Webhook"
required = true
secret = true                  # string or url only; masked and never logged

[[config]]
key = "quiet_hours"
type = "boolean"
default = false
```

Up to 32 config fields. Mistakes name the file and the field, for example
`plugins/ha/plugin.toml: events: unknown event type "download.done"`. The plugin shows as
**Broken** with that message until it is fixed and reloaded.

### `main.lua`

The script returns a table. Only `on_event` is needed.

```lua
local plugin = {}

function plugin.init()            -- once, after loading
  putio.log.info("ready")
end

function plugin.on_event(event)   -- each event it subscribed to
  if event.type ~= "download.finished" then return end
  local r, err = putio.http.request({
    method = "POST",
    url = putio.config.url,
    headers = { ["Content-Type"] = "application/json" },
    body = putio.json.encode({ name = event.data.name, size = event.data.size_bytes }),
  })
  if not r then error(err) end
  if r.status >= 300 then error("HTTP " .. r.status) end
  putio.store.set("last", event.data.name)
end

function plugin.on_tick()         -- about once a minute, if defined
end

return plugin
```

A function that returns is a success. One that raises an error is a failure, with that
message. `event` has the JSON shape documented in [EVENTS.md](EVENTS.md): `id`, `type`,
`time`, `data` and so on. A test from Settings sends `plugin.test`, whatever events the
plugin subscribes to.

### The `putio` API (version 1)

| Call | Returns | Needs |
|------|---------|-------|
| `putio.log.debug/info/warn/error(message, fields?)` | — | — |
| `putio.config` | The plugin's settings and secrets, defaults applied | — |
| `putio.http.request{method?, url, headers?, body?, timeout?}` | `{status, headers, body, truncated}`, or `nil, error` | `http` |
| `putio.store.get(key)` / `set(key, value)` / `delete(key)` / `keys()` | value / `true` or `nil, error` / — / list | `store` |
| `putio.time.now()` | Unix seconds, with fractions | — |
| `putio.time.iso(seconds?)` | RFC 3339 UTC text | — |
| `putio.json.encode(value)` / `decode(text)` | text / value, or `nil, error` | — |
| `putio.plugin.name`, `putio.plugin.version`, `putio.version`, `putio.api_version` | — | — |

- **HTTP** goes only to http(s) hosts the manifest names and the user approved. Any other
  host returns `nil, "host not approved: example.com"`. Up to 3 redirects to the same host
  are followed. Bodies over 1 MiB are cut, with `truncated = true`. A non-2xx status is not
  an error. Errors give the host, never the URL's path or query.
- **Store** is a JSON object per plugin, kept in `plugin-data/<name>.json` next to the config
  and kept across reloads and restarts. Keys can be up to 128 bytes, values up to 64 KiB,
  and the whole store up to 1 MiB.
- **Logs** go to putio-sync's log with the plugin's name. `print` logs at info. A test run
  also shows up to 20 of its lines in Settings.

## Limits and safety

- **Sandbox.** Only `string`, `table`, `math` and `utf8` are loaded. `os`, `io`, `debug`,
  `coroutine`, `package`, `require`, `dofile`, `loadfile`, `collectgarbage` and
  `string.dump` are missing, and `load` takes text only. A plugin cannot read files, run
  programs or open sockets.
- **Time.** Each call (`init`, `on_event`, `on_tick`) may run for `timeout_secs`, including
  the HTTP requests it makes. A call that runs longer is stopped and recorded as a timeout.
- **Memory.** The plugin's whole Lua state may use `memory_mb`. Past that, the call fails
  and the state is built again for the next event.
- **Never holds up a sync.** Each plugin has its own thread and queue. A slow or failing
  plugin delays only its own events. Past 1 000 waiting events, the oldest are dropped and
  counted.
- **Secrets** are shown as `********` in the API, Settings and the export. Saving the mask
  keeps the stored value. Secret values are replaced with `********` in a plugin's errors
  and log lines, in case it prints one.

## Watching plugins

- `GET /plugins` lists each plugin with its state, manifest, approved hosts, waiting events
  and last 20 runs. `POST /plugins/test` and `POST /plugins/reload` do what the Settings
  buttons do.
- Metrics: `putio_sync_plugin_runs_total{plugin, event, result}`,
  `putio_sync_plugin_run_duration_seconds{plugin}` and
  `putio_sync_plugin_events_dropped_total{plugin}`.
- The doctor's Plugins check reports an enabled plugin that cannot load as a problem. A
  plugin that needs approval or a value, settings for a plugin that is not found, and a
  failed last run in the past 24 hours are warnings.

## Later

- Plugins run on events only. Plugins that decide things during a sync, such as file
  filters, rename rules and folder rules, are a follow-up.
- WebAssembly plugins (roadmap 029) are meant to use the same `plugin.toml`, with
  `runtime = "wasm"`, and the same API as JSON imports. `runtime = "wasm"` is refused for
  now.
