# Plugins

A plugin is a small Lua script, or a WebAssembly module built from Rust, Go or another
language (see [WebAssembly plugins](#webassembly-plugins)), that putio-sync runs on its events: a download finished or
failed, a Put.io transfer was added, finished or failed, a sync run ended, or Put.io storage
runs low. The events are the same as for [event hooks](EVENTS.md), but a plugin runs inside
putio-sync. It needs no other program, it is limited in what it may do, and it is set up
from Settings › Plugins.

putio-sync comes with four plugins that send notifications: **ntfy**, **Discord**,
**Telegram** and **Pushover**. They are off until you turn them on.

## Turning a plugin on

Open Settings › Plugins. Plugins that are off are listed under **Available plugins**, one
line each: name, version and what it does. Press **Enable**: putio-sync names the hosts the
plugin will contact and turns it on when you approve them. The plugin then has its own tab
with its state, events, the hosts it contacts ("Contacts"), its settings, its buttons and
**Send test event**, which sends one `plugin.test` event and shows the result with any lines
the plugin logged. Fill in its settings and save; saving approves the hosts of any address
you entered. **Disable** on the tab turns it off and puts it back in the list; its settings
are kept for next time. A tab marked **!** is broken, waits for approval or needs settings.

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
transfer stuck, a failed sync run, low storage, a doctor problem) and **Group repeats for (minutes)** (default 5). The first
event of a kind is sent at once. Later events of the same kind within that time are counted
and sent as one summary, for example "9 more downloads finished: File 1, …, File 5 and 4
more". Set it to 0 to send every event.

Settings › **Notifications** shows the four on one page: turn each on or off, pick its
events and send a test. Their accounts and tokens stay on their tab in Settings › Plugins.

There is no email plugin, as a plugin can only make HTTP requests. Email is built in:
`[notifications.smtp]` in `config.toml`, or Settings › Notifications.

## Media servers: Plex, Jellyfin and Emby

Three bundled plugins tell a media server to rescan only the folder of each finished
download, so new episodes and movies show up within seconds instead of at the next full
library scan. They are off until turned on.

| Plugin | Sign-in | Contacts |
|--------|---------|----------|
| `plex` | **Sign in with Plex**: shows a code with **Copy** and **Copy and open plex.tv/link**; the token is saved for you once you enter it | `plex.tv` and your server's host |
| `jellyfin` | An API key (Dashboard › API Keys), pasted as a secret | Your server's host |
| `emby` | An API key (Settings › Advanced › API Keys), pasted as a secret | Your server's host |

Each has these settings:

- **Server**: the server's address, such as `http://192.168.1.10:32400`. For Plex,
  **Load** lists the servers of your account (local and remote addresses; relays are
  left out).
- **Libraries**: **Load** lists the server's libraries to tick. None ticked means any
  library that holds the folder.
- **Path rules** (`path_map`): one `this machine's folder => the server's folder` per line,
  for when the server sees the download folder under another path, for example
  `/downloads => /data/media` when both run in Docker with different mounts. The longest
  matching folder wins. A Windows path on the right (`D:\Media`) is joined with `\`.
- **Wait for more files** (`delay_seconds`, default 20) and **Longest wait**
  (`max_wait_seconds`, default 300).

How a refresh is sent:

1. A finished download's folder, after the path rules, waits. Each further file in the
   same folder restarts the wait, so a season of 20 episodes gives one rescan of that
   folder, not 20.
2. A folder is sent once no file has landed in it for **Wait for more files**, once it has
   waited **Longest wait**, or as soon as a sync run completes.
3. The folder goes to the chosen library whose folder holds it, by the longest match. A
   folder in no chosen library is skipped and logged. Plex gets one rescan per folder;
   Jellyfin and Emby get every due folder in one request.
4. A failed refresh is tried once more after another wait, then dropped and counted.

Waiting folders are kept in the plugin's store, so a restart or reload does not lose them.
Up to 500 folders wait at once.

**Send test event** and the doctor check the server: whether it answers, whether the token
or API key is accepted, and whether the download folder (after the path rules) is inside a
chosen library. Tokens and API keys are sent only in request headers, never in a URL, and
are never logged or shown.

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
type = "url"                   # string, integer, number, boolean, url, string_list, path_map
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

Say who made the plugin and where to read more. Each is optional; Settings › Plugins shows
the author and license as text and the rest as links on the plugin's tab and in the list:

```toml
author = "your-github-login"     # up to 100 characters
license = "MIT"                  # an SPDX identifier
docs = "https://github.com/you/putio-sync-home-assistant#readme"
homepage = "https://example.com/home-assistant"
repository = "https://github.com/you/putio-sync-home-assistant"
issues = "https://github.com/you/putio-sync-home-assistant/issues"
```

Links must be `https`, or `http` to a local address (`localhost`, a private IP, a `.local`
name); up to 300 characters. When the plugin needs settings, its **Needs settings** line
links to `docs`.

Plugins that need setting up with the server (feature 031) can also declare:

```toml
tick_seconds = 5                 # on_tick this often, 5 to 3600; default 60

[[actions]]                      # buttons in Settings; at most 8
id = "sign_in"                   # a-z, 0-9 and _; up to 40
label = "Sign in"                # up to 40 characters
help = "Opens the sign-in page"  # optional
hidden = true                    # optional: no button; reached through `next` or `choices`

[[config]]
key = "library"
type = "string"
choices = "load_libraries"       # an action id: Settings shows a Load button and a list

[[config]]
key = "path_map"
type = "path_map"                # a list of {from, to}; cannot be secret
```

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

Two more functions are optional:

```lua
-- A button in Settings, or the `next` step of one. `input` is what the last answer put in
-- `next.input` ({} for a button). Settings values typed but not saved yet count for the call.
function plugin.on_action(id, input)
  return {
    message = "Enter this code at example.com/link",   -- up to 500 characters
    code = "AB12",                                      -- shown large with Copy; up to 32
    open_url = "https://example.com/link",              -- a link, and with a code "Copy and open"
    choices = { {value = "1", label = "TV Shows"} },    -- for fields with this choices action
    next = { action = "poll", input = {id = 7}, after_seconds = 2 },   -- 1 to 30 s
    done = false,
    save = { token = "..." },   -- saved to the plugin's settings at once; secrets masked
  }
end

-- The doctor and Send test event ask this. Each item is "ok", "warn" or "fail".
function plugin.health()
  return { {status = "ok", message = "Server reachable"} }
end
```

Settings runs `next` steps on its own for up to 10 minutes. While they run, a `code` or
`open_url` from an earlier step stays on screen when a later step leaves it out. A code gets a
**Copy** button, and with `open_url` also **Copy and open**, which copies the code and opens the
link in a new tab. Copying needs HTTPS or localhost; over plain http the page asks you to select
the code (one click selects all of it) and press Ctrl+C. `on_action` and `health` run
on a worker of their own, with the same sandbox and limits, and may call every host the
plugin's settings need (approved or not, since the person is setting it up). One action or
health check per plugin runs at a time. Values in `save` must be the plugin's own settings;
they are checked like a Settings save and do not turn the plugin on.

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
| `putio.path.map(path, rules)` | `path` with the longest matching `{from, to}` rule applied, or unchanged | — |
| `putio.path.parent(path)` / `within(path, folder)` | The folder above / whether `path` is `folder` or below it | — |
| `putio.metrics.count(name, n?)` | Adds `n` (default 1) to `putio_sync_plugin_counter_total{plugin, name}`; up to 16 names, `[a-z0-9_]` | — |
| `putio.download_dir` | The daemon's download folder, or `""` | — |
| `putio.fs.read(path)` / `write(path, text)` / `list(folder)` / `exists(path)` | text / `true` / `{name, dir, size}` list / bool, or `nil, error` | `files` |

- **HTTP** goes only to http(s) hosts the manifest names and the user approved. Any other
  host returns `nil, "host not approved: example.com"`. Up to 3 redirects to the same host
  are followed. Bodies over 1 MiB are cut, with `truncated = true`. A non-2xx status is not
  an error. Errors give the host, never the URL's path or query.
- **Store** is a JSON object per plugin, kept in `plugin-data/<name>.json` next to the config
  and kept across reloads and restarts. Keys can be up to 128 bytes, values up to 64 KiB,
  and the whole store up to 1 MiB.
- **Files** (feature 029) are reached only inside the folders the manifest asks for and the
  user approved; see [File access](#file-access). Reads and writes are up to 1 MiB, and a
  list gives at most 1 000 entries.
- **Logs** go to putio-sync's log with the plugin's name. `print` logs at info. A test run
  also shows up to 20 of its lines in Settings.

### File access

A plugin that reads or writes files names its folders in the manifest:

```toml
[permissions.files]
read = ["{download_dir}"]            # absolute folders, or {download_dir} and below
write = ["{download_dir}/.cache"]    # a write folder can be read too
read_from_config = ["library"]       # string settings that hold a folder
write_from_config = []
```

Settings › Plugins lists these folders on the plugin ("May read /downloads; read and
write /media/notes"), and turning the plugin on and saving approves them, as with hosts.
They are kept in `approved_paths` on the plugin's `[[plugins]]` entry. When a setting that
names a folder changes, the plugin waits in **Needs approval** until the next save. Paths
are checked after `..` and links are resolved, so a plugin cannot step or link out of its
folders.

## WebAssembly plugins

A plugin can be a WebAssembly module instead of Lua. It uses the same `plugin.toml`, with
two more lines, and gets the same events, sync rules, settings, actions, store, limits and
run history:

```toml
runtime = "wasm"            # "lua" by default
entry = "plugin.wasm"       # the default for wasm
```

The module talks to putio-sync in JSON through its memory, with three imports from a
`putio` module: `call` makes one API call (`{"fn": "http.request", "args": [...]}`, the same
calls as the Lua table above) and returns the reply's length, `reply` copies the reply out,
and `fail` ends the call with a message. Each plugin function (`init`, `on_event`,
`filter`, `rename`, `folder`, `on_tick`, `on_action`, `health`) takes a JSON array of the
Lua function's arguments and returns JSON. The full ABI is in
[specs/029-wasm-plugins/contracts/wasm-abi.md](specs/029-wasm-plugins/contracts/wasm-abi.md).

- **Examples.** [`rust-clean-names`](examples/plugins/rust-clean-names/) (Rust, rename) and
  [`go-min-size-filter`](examples/plugins/go-min-size-filter/) (Go, filter) each have a
  small `putio` binding you can copy, and build steps in their README. Tests build or load
  both, so they keep working.
- **WASI.** Modules built for `wasm32-wasip1` load: clocks and random numbers are real,
  stdout and stderr become log lines, and every other WASI call (files, sockets,
  environment) answers "not supported". The only way out of the sandbox is the `putio` API.
- **Limits.** Modules are up to 16 MiB. The plugin's `memory_mb` covers its whole memory,
  and `timeout_secs` (or the sync-rule limit) each call. Over either, or a trap or
  `proc_exit`, the call fails and the next one starts a fresh instance. A module that
  imports something the host does not offer, or exports `_start`, is **Broken** with a
  message that says why.
- **One file.** A module can carry its `plugin.toml` in a custom section named
  `putio-plugin`, so it installs as a single `.wasm` (the Rust example does this).
- **JavaScript** and other languages work through a WebAssembly build of their runtime
  (such as Javy for JavaScript); no example ships for them.

### Checking a plugin

```bash
putio-sync plugins check ./home-assistant
```

checks a plugin folder the way **Reload plugins** does (the manifest, the entry file, and
the Lua script's syntax, compiled but not run) and prints each problem, then
`home-assistant 1.0.0: ok` or the number of problems. It needs no config or Put.io token.
`--json` prints `{"name", "version", "problems"}`. Exit code: 0 no problems, 1 problems,
2 not a folder.

### Installing a plugin

Settings › Plugins › Plugin workbench › **Install or replace a plugin** takes a URL, or files: one `.wasm` that carries
its manifest, or a `plugin.toml` with its entry file. A URL can point at a `.wasm` or at a
`plugin.toml`, whose entry is fetched from the same folder. The plugin is written to its
own folder in the plugins folder and starts off, with its hosts and folders listed for you
to check before turning it on. Installing over a plugin of the same name needs **Replace a plugin of the same name**,
which keeps its settings and store. `POST /plugins/install` does the same from scripts; see
the [README](README.md#http-api).

## Sync rules

A plugin can also take part in a sync with up to three **sync rules**. They are called
while the sync runs, so each call has a short time limit, and a plugin that fails or is
slow is passed over: the built-in rules apply.

| Rule | Asked | Returns |
|---|---|---|
| `filter` | for each file of a sync pass, after the folder rules and type filter | `"download"`, `"stream"` or `"skip"` (`true` and `false` mean download and skip), or `nil` to keep the built-in decision |
| `rename` | before a download starts | a file name (no `/` or `\`, up to 255 bytes), or `nil` |
| `folder` | before a download starts | a folder inside the download folder, such as `"TV/Show/Season 01"` (`""` for the folder itself, no `..`), or `nil` |

Declare them in `plugin.toml` and return a function of the same name. `events` may then be
left out, and `on_event` is needed only for the events listed:

```toml
name = "min-size-filter"
version = "1.0.0"
hooks = ["filter"]            # filter, rename, folder
```

```lua
return {
  filter = function(file)
    if file.extension == "mkv" and file.size < 50 * 1024 * 1024 then return "skip" end
    return nil
  end,
}
```

Each function gets one `file` table:

| Field | |
|---|---|
| `id` | Put.io file id |
| `name` | Put.io name |
| `size` | bytes |
| `extension` | lowercase, without the dot; `""` when none |
| `folder` | Put.io folder relative to the synced folder, `""` at the top |
| `decision` | the built-in decision: `download`, `stream`, `blocked` or `skipped` (`download` for rename and folder) |
| `reason` | the built-in reason in words, `""` for a download |

How answers are used:

- Plugins are asked in name order, and the first answer that is not `nil` wins.
- A filter's `"download"` or `"stream"` overrides a block or a skipped folder. `"skip"`
  shows in the file list as "skipped by plugin <name>". A file a plugin skips is never
  deleted from Put.io, even where **Delete blocked files** is on.
- A folder answer replaces the Put.io folder; the processing pipeline may still stage the
  file. A rename answer replaces the name, except on a retry that already chose a numbered
  name after a clash. Names and folder parts are made safe as Put.io names are.
- What happens when a file exists still goes by the file's Put.io folder rules.
- Files Radarr or Sonarr asked for are never filtered, renamed or moved.
- The preview and the dry run show the built-in rules only; plugins are not asked.
- Files stream mode keeps on Put.io are placed too when `[organise]` is on and
  `use_plugins` is true: the folder answer (read from the Put.io root) and the rename
  answer become the file's planned move on Put.io, shown on the Organise panel before
  anything moves unless `apply = "auto"`. `decision` is `stream` for these calls.

Limits:

- **Time.** Each call may take `hook_timeout_ms` (500 ms by default, 50 to 5000; Settings
  shows it as **Sync rule time limit**). A slower call is stopped and passed over.
- **Own worker.** A plugin with sync rules gets a second worker for them, sharing its
  store and settings, so a sync never waits behind the plugin's queued events. `init` runs
  in both. That worker loads the plugin as soon as it starts, and the plugin is asked only
  once it has loaded: until then, and while it loads again after a timeout, the built-in
  rules apply.
- **Pause.** An error, a timeout or an answer that does not check out is logged as a
  warning and kept with the plugin's runs. After 3 in a row, the plugin's sync rules rest
  for 5 minutes. An answer resets the count, and so does a reload or a settings change.
- **Send test event** calls `on_event`, which a plugin with only sync rules does not have,
  so its test fails. `putio-sync-plugins/tests/examples.rs` shows how to test sync rules.

Working examples, one for each rule, are in [`examples/plugins/`](examples/plugins/):
[`min-size-filter`](examples/plugins/min-size-filter/) (filter),
[`clean-names`](examples/plugins/clean-names/) (rename) and
[`tv-folders`](examples/plugins/tv-folders/) (folder). Copy a folder into the plugins
folder and reload.

## The plugin workbench

Settings › Plugins › **Plugin workbench** (feature 055) is a basic editor and debugger in
the web UI, with no breakpoints or stepping:

- **New plugin** writes a folder from a starter: `webhook-notifier`, `store-counter` or
  `tick-poller`. The same starters are in `examples/plugins/`.
- **The editor** shows `plugin.toml` and `main.lua` with line numbers. Save checks the
  manifest (with the same `file: field: message` errors as a reload) and the Lua syntax,
  keeps the old files as `.bak`, and reloads only that plugin. Bundled plugins are
  read-only; **Copy to edit** makes a folder copy that replaces the bundled one. WebAssembly
  plugins show their manifest only; install a new build over them.
- **The Run panel** calls `on_event` with a sample of any event type, `on_tick`, `health`,
  a settings action, or a sync rule (`filter`, `rename`, `folder`) with a sample file, for
  Lua and WebAssembly plugins. A run is a dry run by default: HTTP requests are recorded
  and not sent, `putio.fs.write` is recorded and not written, and the store is a scratch
  copy. **Send for real** uses the approved hosts and the real store. It shows the result
  or the error with its line, the time, memory, log lines, each request with its secrets
  masked, and the store before and after. The last 20 runs can be run again.

The editor is off by default: set `plugin_editor = true`, or tick **Edit plugins here**.
Without it, plugins can be read and run but not changed. The guide,
[Writing and debugging plugins](site/book/src/plugin-workbench.md), has the details.

## Limits and safety

- **Sandbox.** Only `string`, `table`, `math` and `utf8` are loaded. `os`, `io`, `debug`,
  `coroutine`, `package`, `require`, `dofile`, `loadfile`, `collectgarbage` and
  `string.dump` are missing, and `load` takes text only. A plugin cannot run programs or
  open sockets, and reaches files only through `putio.fs` in its approved folders.
  WebAssembly plugins get only the `putio` API and a WASI layer with no real access.
- **Time.** Each call (`init`, `on_event`, `on_tick`, `on_action`, `health`) may run for `timeout_secs`, including
  the HTTP requests it makes. A call that runs longer is stopped and recorded as a timeout.
- **Memory.** The plugin's whole Lua state, or a module's memory, may use `memory_mb`. Past that, the call fails
  and the state is built again for the next event.
- **Never holds up a sync.** Each plugin has its own thread and queue. A slow or failing
  plugin delays only its own events. Sync rules have a separate worker and a limit of
  their own; see [Sync rules](#sync-rules). Past 1 000 waiting events, the oldest are dropped and
  counted.
- **Secrets** are shown as `********` in the API, Settings and the export. Saving the mask
  keeps the stored value. Secret values are replaced with `********` in a plugin's errors
  and log lines, in case it prints one.

## Watching plugins

- `GET /plugins` lists each plugin with its state, manifest (actions and fields with
  choices included), approved hosts, waiting events and last 20 runs. `POST /plugins/test`,
  `POST /plugins/reload`, `POST /plugins/action`, `POST /plugins/install` and the
  `/plugins/workbench` routes do what the Settings buttons do.
- Metrics: `putio_sync_plugin_runs_total{plugin, event, result}`,
  `putio_sync_plugin_run_duration_seconds{plugin}` and
  `putio_sync_plugin_events_dropped_total{plugin}`, and each plugin's own counters as
  `putio_sync_plugin_counter_total{plugin, name}`. The media plugins count `refreshes`
  (folders sent), `folders_grouped` (files that joined a waiting folder), `skipped` and
  `refresh_failures`.
- The doctor's Plugins check reports an enabled plugin that cannot load as a problem. A
  plugin that needs approval or a value, settings for a plugin that is not found, and a
  failed last run in the past 24 hours are warnings. Each enabled plugin with a `health()`
  is asked too: a `fail` item is a problem and a `warn` item a warning. A plugin busy with
  a settings action is skipped.

## Later

- Sync rules (feature 028) run in the sync pass and the queue worker only. The preview and
  dry run do not ask plugins.
