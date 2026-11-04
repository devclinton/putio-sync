# Contract: Plugin manifest (`plugin.toml`)

A plugin is a folder `<plugins_dir>/<folder>/` holding `plugin.toml` and the entry file.
The same manifest is meant for 029 with `runtime = "wasm"` and a `.wasm` entry.

```toml
name = "ntfy"
version = "1.0.0"
description = "Push notifications through ntfy"
runtime = "lua"            # default
entry = "main.lua"         # default
events = ["download.finished", "download.failed", "transfer.failed",
          "sync.completed", "storage.low"]

[permissions]
store = false

[permissions.http]
hosts = []                          # fixed hosts
host_from_config = ["server"]       # hosts taken from URL config fields

[[config]]
key = "server"
type = "url"
label = "Server"
default = "https://ntfy.sh"

[[config]]
key = "topic"
type = "string"
label = "Topic"
required = true

[[config]]
key = "token"
type = "string"
label = "Access token"
secret = true
help = "Only for protected topics"
```

Field rules are in [data-model.md](../data-model.md). Errors name the file and the field,
e.g. `plugins/ha/plugin.toml: events: unknown event type "download.done"`.

## Entry script (Lua)

The entry file returns a table. Every function is optional except `on_event`.

```lua
local plugin = {}
function plugin.init() end              -- once after loading
function plugin.on_event(event) end     -- each matching event (027 JSON shape)
function plugin.on_tick() end           -- about every 60 s
return plugin
```

A function returning normally is a success; raising an error (`error("...")`) is a failure
with that message.
