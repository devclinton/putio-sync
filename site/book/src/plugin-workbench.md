# Writing and debugging plugins

The plugin workbench (feature 055) lets you write a Lua plugin and try it from the web UI,
without a terminal or a restart. Open **Settings › Plugins › Plugin workbench**. It has
three parts:

- **New plugin** makes a plugin folder from a starter.
- **The editor** shows a plugin's `plugin.toml` and `main.lua`, checks them and saves them.
- **The Run panel** calls one plugin function and shows what it did.

The workbench is a basic editor and debugger. It has no breakpoints or stepping. For the
plugin format itself (the manifest, the `putio` API, permissions, sync rules), see the
[Plugin reference](plugins.md). For the events a plugin gets, see the
[Event reference](events.md).

## Turning the editor on

The editor is off by default. With it off, you can read any plugin's files and run saved
plugins, but nothing can be written. Tick **Edit plugins here** to turn it on. This saves
`plugin_editor = true` in the config file:

```toml
# A top-level key: set it above the first [section]
plugin_editor = true
```

`PUT /plugins/workbench/editor` with `{"enabled": true}` does the same. The API refuses to
make, save or copy plugins (403), or to run editor text, while the editor is off.

Anyone who can edit plugins can run code inside the plugin sandbox, with the hosts and
folders you approved for that plugin. Turn the editor on only on a putio-sync you trust
everyone signed in to.

## Starters

**New plugin** writes a folder into the plugins folder from one of three starters. Each
starter is also in `examples/plugins/` in the repository, with a README.

| Starter | What it shows |
|---|---|
| `webhook-notifier` | `on_event`, a secret URL from the settings (`host_from_config`), `putio.http.request` and an error when the server answers with an error |
| `store-counter` | `putio.store`, counting finished downloads per day, and `health()` reporting the count |
| `tick-poller` | `on_tick` every 5 minutes with `tick_seconds`, the same check after each sync, and the store to remember what it saw last |

The name takes letters, digits, `-` and `_`, and must not be in use. A new plugin starts
turned off: enable it in the Plugins list when it does what you want. The Run panel works
on a plugin that is off.

## The editor

Pick a plugin. Its `plugin.toml` and entry script show in plain code areas with line
numbers.

- **Check** reads the manifest and compiles the script without saving or running them.
  Problems show with the same text a reload gives: `file: field: message` for the
  manifest, and the line for a Lua syntax error, such as
  `main.lua:6: ')' expected near '('`. **Go to line** selects that line.
- **Save and reload** writes both files and reloads only that plugin. The old files are
  kept as `plugin.toml.bak` and `main.lua.bak`. A plugin saved with problems is written but
  shows as broken until you fix it.
- If a file changed on disk after you opened it (another tab, or an editor on the NAS),
  Save stops and asks. **Save over the changes** writes yours; **Load the files again**
  drops them.

What can be edited:

- **A folder Lua plugin** can be edited.
- **A bundled plugin** (ntfy, Discord, Telegram, Pushover, Plex, Jellyfin, Emby) is
  read-only. **Copy to edit** puts a copy in the plugins folder; a folder plugin with a
  bundled plugin's name replaces it. Delete the folder to go back to the bundled one.
- **A WebAssembly plugin** is built elsewhere, so only its `plugin.toml` shows. Use
  **Install or replace a plugin** with **Replace** ticked to put a new build over it, from a
  URL or files.

## The Run panel

The Run panel calls one function of the plugin picked above:

- **An event**: `on_event` with an event of the type you pick. Each type has a sample
  `data` you can edit; the link beside it opens that event's fields in the
  [Event reference](events.md). Only the types the plugin's `events` take are listed.
- **on_tick** and **health**.
- **A settings action**: `on_action(action, input)` for one of the plugin's `[[actions]]`,
  with the input you type as JSON.
- **A sync rule**: the plugin's `filter`, `rename` or `folder` function with a sample file
  (id, name, size, extension, folder, decision, reason). The result shows what the sync
  would make of the answer, such as the new name.

### Dry runs

A run is a dry run unless **Send for real** is ticked:

- HTTP requests are recorded, not sent. Each one gets the reply you set (status 200 and
  an empty body unless you change them). Requests to a host the plugin may not call
  fail as they would for real.
- The store is a scratch copy of the plugin's store. What the run writes is shown, then
  thrown away.
- Files the plugin writes with `putio.fs.write` are listed, not written. Reads use the
  folders you approved.

**Send for real** sends requests to the approved hosts and uses the real store, as the
plugin would when an event arrives.

**Run the text in the editor, not the saved files** runs what is in the code areas without
saving it. It needs the editor on, and works for Lua plugins only.

### What a run shows

- The result: what the function returned, or the error with its line and a **Go to line**
  link.
- The time it took and the memory in use at the end.
- The log lines (up to 200).
- Each HTTP request: method, URL, headers and body, and for a real run its status and
  time. Authorization, cookie and token headers, any header whose name contains `token`,
  `key`, `secret` or `password`, and the plugin's secret values are shown as `********`.
- Files a dry run would have written.
- The store before and after.

**Last runs** keeps the last 20 runs, newest first, until the daemon restarts. **Run
again** repeats one with the same input. Workbench runs are not counted in the plugin's
own runs, metrics or history.

One run of a plugin happens at a time: a run while another run, settings action or health
check of it is going answers 409. Runs use the plugin's time and memory limits.

## The API

The workbench is under `/plugins/workbench`; the README's API table lists each route.
`POST /plugins/workbench/run` takes:

```json
{
  "name": "store-counter",
  "call": { "kind": "event", "event_type": "download.finished", "data": null },
  "real": false,
  "reply": { "status": 200, "body": "" }
}
```

`call.kind` is `event`, `tick`, `health`, `action` (`action`, `input`) or `hook` (`hook`,
`file`). Add `manifest` and `script` to run editor text.
